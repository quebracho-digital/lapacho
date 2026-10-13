#!/usr/bin/env python3
"""Analyse terms with a model you run: a Lapacho plugin (desktop).

The clip arrives on standard input; the report goes to standard output, and
Lapacho saves it as a new clip. Settings live in `terms-assistant.local.json`
next to this script, written once, so nothing is typed per run:

  {"endpoint": "http://localhost:8080", "model": "qwen3.8-27b", "token": ""}

  endpoint  where to send it:
            - an OpenAI-compatible server (llama.cpp, Ollama, LM Studio, vLLM):
              its base URL, e.g. http://localhost:8080
            - a Drupal site with ai_provider_universal_terms: its analyze URL,
              e.g. https://example.org/api/terms/analyze
  model     the model, for an OpenAI-compatible server (llama.cpp's router
            needs it): its id, or any part of it ("qwen3.8-27b"), matched
            against the server's /v1/models; ignored by Drupal, which has
            its own setting
  token     optional. "user:password" is sent as Basic auth, anything else as
            a Bearer token. Never written by this script; if you keep it in
            the settings file, make that file readable only by you.

To an OpenAI-compatible server it sends the same request as the built-in
"Analyse terms" plugin (it runs `$LAPACHO_BIN plugin terms`), and returns the
model's report as it is. Drupal checks every quote against the text and
drops invented ones; this script formats what it keeps.

Standard library only. Errors go to standard error, which Lapacho shows.
"""
import base64
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request


def fail(message):
    print(message, file=sys.stderr)
    sys.exit(1)


def post(url, body, token):
    """POSTs `body` as JSON, or GETs when it is None."""
    headers = {"Content-Type": "application/json", "Accept": "application/json"}
    if token:
        if ":" in token:
            headers["Authorization"] = "Basic " + base64.b64encode(token.encode()).decode()
        else:
            headers["Authorization"] = "Bearer " + token
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data, headers)
    try:
        with urllib.request.urlopen(req, timeout=590) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", "replace")
        try:
            detail = json.loads(detail).get("error", detail)
        except (ValueError, AttributeError):
            pass
        fail(f"{url}: HTTP {e.code}: {str(detail)[:300]}")
    except (urllib.error.URLError, OSError) as e:
        fail(f"Could not reach {url}: {e}")


def drupal(endpoint, text, token):
    result = post(endpoint, {"text": text}, token)
    findings = result.get("findings") or {}
    lines = [result.get("notice", "Not legal advice."), ""]
    if not findings:
        lines.append("No clause of the analysed kinds was found.")
    for category, found in findings.items():
        lines.append(f"## {found.get('description') or category}")
        lines += [f"> {quote}" for quote in found.get("quotes", [])]
        lines.append("")
    if result.get("dropped_quotes"):
        n = result["dropped_quotes"]
        lines.append(f"({n} quote{'' if n == 1 else 's'} from the model {'was' if n == 1 else 'were'} not in the text and dropped.)")
    lines.append(f"Document: {result.get('hash', '')}")
    return "\n".join(lines)


def resolve_model(v1, model, token):
    """The server's id for `model`: itself if listed, else the one id that
    contains it (case aside). A server that doesn't list models gets it as is."""
    try:
        ids = [m["id"] for m in post(v1 + "/models", None, token)["data"]]
    except (KeyError, TypeError):
        return model
    if not model or model in ids:
        return model
    found = [i for i in ids if model.lower() in i.lower()]
    if len(found) == 1:
        return found[0]
    listed = "\n  ".join(found or ids)
    fail(f"{'Several models match' if found else 'No model matches'} '{model}' at {v1}:\n  {listed}")


def openai_compatible(endpoint, model, text, token):
    lapacho = os.environ.get("LAPACHO_BIN")
    if not lapacho:
        fail("LAPACHO_BIN is not set: run this as a Lapacho plugin.")
    built = subprocess.run([lapacho, "plugin", "terms"], input=text, capture_output=True, text=True)
    if built.returncode != 0:
        fail(built.stderr.strip() or "Could not build the request.")
    base = endpoint.rstrip("/")
    v1 = base if base.endswith("/v1") else base + "/v1"
    model = resolve_model(v1, model, token)
    url = v1 + "/chat/completions"
    body = {
        "messages": [{"role": "user", "content": built.stdout}],
        "temperature": 0,
        # Qwen-style models think out loud unless told not to.
        "chat_template_kwargs": {"enable_thinking": False},
    }
    if model:
        body["model"] = model
    reply = post(url, body, token)
    try:
        content = reply["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError):
        fail(f"Unexpected answer from {url}: {str(reply)[:300]}")
    return f"{content.strip()}\n\n({reply.get('model') or model or 'model'}, at {base})"


def settings():
    """endpoint, model, token. A LAPACHO_PARAM_<NAME> wins over the file, for
    a copy of the plugin's .json that asks for a value in its form."""
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "terms-assistant.local.json")
    try:
        with open(path, encoding="utf-8") as f:
            saved = json.load(f)
    except FileNotFoundError:
        saved = {}
    except ValueError as e:
        fail(f"{path}: {e}")
    return [os.environ.get(f"LAPACHO_PARAM_{k.upper()}", "").strip() or str(saved.get(k, "")).strip()
            for k in ("endpoint", "model", "token")]


def main():
    endpoint, model, token = settings()
    text = sys.stdin.read()
    if not text.strip():
        fail("Nothing to analyse")
    if not endpoint.startswith(("http://", "https://")):
        fail("Set an http(s) endpoint in terms-assistant.local.json, next to terms-assistant.py")
    if endpoint.rstrip("/").endswith("/api/terms/analyze"):
        print(drupal(endpoint, text, token))
    else:
        print(openai_compatible(endpoint, model, text, token))


if __name__ == "__main__":
    main()
