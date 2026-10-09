#!/usr/bin/python3
"""The desktop UI against the XSS payloads in test-payloads/, without clicks.

Each payload goes through lapacho-core's real ingest (examples/ui_items.rs)
and comes back as a history item. The built UI is served with a mocked Tauri
that hands those items over and records every command the page invokes, and
loaded in WebKitGTK (the engine Tauri uses on Linux) offscreen. Each item is
opened in the detail view, rendered (Mermaid included), then closed. The
payloads try to call `clear_history`, `copy_item` or `run_plugin`; any command
outside what the page itself asks for while browsing fails the test, and so
does an alert.

Twice: with the app's CSP (tauri.conf.json), and without it, so the
sanitizers have to hold on their own too.

Run from the repo root, with the system Python (it has PyGObject):
  /usr/bin/python3 apps/desktop/ui/tests/xss.py [--dist DIR]
--dist reuses a `trunk build` output instead of building one. Needs a display
(xvfb-run in CI).
"""
import argparse, base64, functools, glob, hashlib, http.server, json, os, re, shutil, subprocess, sys, tempfile, threading

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))))
UI = os.path.join(ROOT, "apps/desktop/ui")
PAYLOADS = sorted(glob.glob(os.path.join(ROOT, "test-payloads/*")))
PAYLOADS = [p for p in PAYLOADS if not p.endswith("README.md")]
# What the page asks for by itself while listing and opening items.
ALLOWED = {"get_history", "search_history", "get_persist_level", "get_sensitive_ttl", "app_version", "list_plugins"}

MOCK = """
window.__calls = [];
window.__errors = [];
window.addEventListener('error', (e) => window.__errors.push(String(e.message)));
window.alert = (m) => window.__calls.push({cmd: 'alert', args: String(m)});
const ITEMS = %s;
const REPLIES = {get_history: ITEMS, search_history: ITEMS, get_persist_level: 'none',
                 get_sensitive_ttl: null, app_version: 'xss-test', list_plugins: []};
window.__TAURI__ = {
  core: {invoke: async (cmd, args) => { window.__calls.push({cmd, args}); return REPLIES[cmd] ?? null; }},
  event: {listen: async () => () => {}},
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
window.addEventListener('load', async () => {
  for (let t = 0; document.querySelectorAll('li.item').length < ITEMS.length && t < 200; t++) await sleep(50);
  const opened = [];
  for (let i = 0; i < ITEMS.length; i++) {
    const li = document.querySelectorAll('li.item')[i];
    const max = li && li.querySelector('button[title="Maximize"]');
    if (!max) continue;
    max.click();
    await sleep(1500);  // Mermaid renders on the next frame, in its sandbox
    opened.push(ITEMS[i].id);
    const close = document.querySelector('.modal .close');
    if (close) close.click();
    await sleep(100);
  }
  const div = document.createElement('div');
  div.id = 'RESULT';
  div.textContent = JSON.stringify({listed: document.querySelectorAll('li.item').length, opened, calls: window.__calls, errors: window.__errors});
  document.body.appendChild(div);
});
"""


def build(dist):
    subprocess.run(["trunk", "build", "--dist", dist], cwd=UI, check=True)


def prepare(dist, out, items, with_csp):
    shutil.copytree(dist, out)
    page = open(os.path.join(out, "index.html")).read()
    head = '<script src="mock.js"></script>'
    if with_csp:
        csp = json.load(open(os.path.join(ROOT, "apps/desktop/src-tauri/tauri.conf.json")))["app"]["security"]["csp"]
        # Tauri allows the page's own inline scripts (trunk's wasm loader) by hash.
        hashes = " ".join(
            "'sha256-%s'" % base64.b64encode(hashlib.sha256(s.encode()).digest()).decode()
            for s in re.findall(r"<script[^>]*>(.*?)</script>", page, re.S) if s.strip())
        csp = csp.replace("script-src 'self'", f"script-src 'self' {hashes}")
        head = f'<meta http-equiv="Content-Security-Policy" content="{csp}">' + head
    page = page.replace("<head>", "<head>" + head, 1)
    open(os.path.join(out, "index.html"), "w").write(page)
    open(os.path.join(out, "mock.js"), "w").write(MOCK % json.dumps(items))


def run(out):
    class Quiet(http.server.SimpleHTTPRequestHandler):
        def log_message(self, *a):
            pass
    handler = functools.partial(Quiet, directory=out)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    win = Gtk.OffscreenWindow()
    view = WebKit2.WebView()
    view.set_size_request(1000, 800)
    win.add(view)
    win.show_all()
    view.load_uri(f"http://127.0.0.1:{server.server_address[1]}/")
    result = {}

    def poll():
        def done(v, res):
            try:
                text = v.evaluate_javascript_finish(res).to_string()
            except Exception:
                text = ""
            if text and text != "null":
                result.update(json.loads(text))
                Gtk.main_quit()
        view.evaluate_javascript("document.getElementById('RESULT')?.textContent ?? null", -1, None, None, None, done)
        return True

    # Both sources go when this run ends: left behind, they would end the next one.
    sources = [GLib.timeout_add(500, poll), GLib.timeout_add_seconds(120, lambda: Gtk.main_quit() or False)]
    Gtk.main()
    for s in sources:
        GLib.source_remove(s)
    win.destroy()
    server.shutdown()
    return result


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dist")
    args = ap.parse_args()
    items = json.loads(subprocess.run(
        ["cargo", "run", "-q", "-p", "lapacho-core", "--example", "ui_items", "--", *PAYLOADS],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    tmp = tempfile.mkdtemp(prefix="lapacho-xss-")
    dist = args.dist or os.path.join(tmp, "dist")
    if not args.dist:
        build(dist)
    failed = False
    for with_csp in (True, False):
        out = os.path.join(tmp, "csp" if with_csp else "no-csp")
        prepare(dist, out, items, with_csp)
        r = run(out)
        label = "with CSP" if with_csp else "without CSP"
        bad = [c for c in r.get("calls", []) if c["cmd"] not in ALLOWED]
        missing = sorted({i["id"] for i in items} - set(r.get("opened", [])))
        ok = r and not bad and not missing
        failed |= not ok
        print(f"{label}: {'ok' if ok else 'FAILED'} — {r.get('listed', 0)} listed, {len(r.get('opened', []))} opened")
        for c in bad:
            print(f"  executed: {c['cmd']} {json.dumps(c.get('args'))}")
        if missing:
            print(f"  not opened: {', '.join(missing)}")
            for e in r.get("errors", []):
                print(f"  page error: {e}")
    shutil.rmtree(tmp, ignore_errors=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
