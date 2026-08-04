# Lapacho

A **secure** desktop clipboard manager. It classifies, sanitizes, and masks
content before it reaches the UI: credentials and secrets are never shown in
plain text, and the original content is only released when you explicitly paste
it back.

Part of the **Quebracho Digital** ecosystem. Replaces the prototypes
`quebracho-client` and `RustyBoard`.

> **Status:** advanced development. 98 tests across the workspace. The desktop
> app (Tauri 2 + Leptos/WASM) has a complete backend, native tray, global
> shortcut Ctrl+Shift+Alt+L (Lapacho-exclusive), image support, rich rendering
> (MD/SVG/Mermaid), and a raw-first security model.
>
> Known gap: the tray menu rebuild costs ~290 ms per capture, which is where
> the end-to-end latency lives (capture and storage together are ~16 ms). See
> [Diagnostics](#diagnostics) for how to measure it yourself.

## Features

- **Automatic capture** of the clipboard via a background monitor.
- **Type classification:** text, URL, JSON, SVG, Mermaid, Markdown, image.
- **Sensitivity classification:** `None` / `Personal` / `Credential` / `Secret`,
  using regex + Shannon entropy (private keys, API tokens, credit cards, emails,
  etc.).
- **Masking:** credentials and secrets are shown redacted — `••••last4` for a
  secret, `first3…last4` for a credential — so two of them are still
  distinguishable in a list without revealing the value or its length. The
  `raw_content` is never sent to the UI layer. See
  [Security Model](#security-model) for what that trade does and does not buy.
- **Persistence policies:**
  - `Paranoia` (default): only stores non-sensitive content.
  - `Balanced`: stores everything except secrets, with TTL for credentials.
  - `All`: stores everything.
- **SVG sanitization:** removes XSS vectors (scripts, `on*` handlers,
  `javascript:`).
- **Transformation plugins:** external commands that receive content via
  `stdin` (no command injection), with timeout, and whose output is sanitized
  before display.
- **SQLite history** (WAL mode) with size limit and TTL-based cleanup.

## Architecture

Cargo workspace (Rust 2024 edition):

```
lapacho/
├─ crates/lapacho-core/    # Pure logic, no UI (lib)
│  ├─ types        # ClipboardItem, UIClipboardItem (safe projection), enums
│  ├─ detectors    # content type classification
│  ├─ security     # sanitization + sensitivity classification
│  ├─ storage      # SQLite: history, persistence levels, TTL
│  ├─ plugins      # external plugin execution
│  └─ ingest       # pipeline that composes everything + masking
└─ apps/desktop/
   ├─ src-tauri/   # Tauri 2 backend (clipboard monitor + commands)
   ├─ ui/          # Leptos/WASM frontend (standalone crate, built with Trunk)
   └─ legacy-ui/   # Original vanilla UI, kept as reference
```

`lapacho-core` does not depend on Tauri or any UI framework: it is reusable
from any frontend.

### Codebase map

A generated knowledge graph lives at `.ua/knowledge-graph.json`: 218 nodes and
361 edges across 7 architectural layers, plus a 15-step guided tour that walks
the security flow end to end (capture → type classification → sensitivity
classification → masking → persistence policy → release on paste). It is meant
as the fastest way in for anyone new to the project.

Explore it with the [understand-anything](https://github.com/Egonex-AI/Understand-Anything)
plugin:

```bash
/understand-dashboard    # interactive graph viewer
/understand              # refresh after changes (incremental)
```

> **A note, and an apology:** the graph's text — summaries, layer names, tour
> steps — is in **Spanish**, while the rest of this project is in English. That
> inconsistency is on us, not on you. It was generated in the maintainer's
> working language and kept rather than discarded, on the grounds that a map in
> the wrong language still beats no map. Regenerating it in English is a single
> command (`/understand --full --language en`) if you would rather have it that
> way, and such a PR is welcome.

The graph is a derived artifact: if it ever contradicts the code, the code wins.

## Security Model

- The `raw_content` (original content) lives **only in the backend**; the UI
  receives a `UIClipboardItem` that never includes it.
- Credentials and secrets are masked before display. The mask is **not** fully
  opaque, deliberately: a secret shows as `••••last4` and a credential as
  `first3…last4`, so you can tell two of them apart in a list of twenty. The
  full value and its length are never shown, but those few characters are. If
  that trade is wrong for your threat model, it is one function —
  `ingest::sensitive_display`.
- Plugins receive their input via `stdin` (not arguments → no injection) and run
  with a timeout; their output is sanitized before reaching the UI.

### Content at rest, and content in memory

These are different problems and it is worth being precise about which is
solved.

**At rest** — history is encrypted with AES-256-GCM in SQLite. The key lives in
the OS keyring (file fallback) and is `mlock`-ed so it cannot be swapped. The
encryption boundary is the storage layer: `save` encrypts, `load` decrypts, and
everything above that line works with plaintext.

**In memory** — the live session buffer necessarily holds plaintext. It has to:
the app searches it, renders it, and pastes it back. Encrypting it in RAM with
the key sitting next to it would be theatre. What can be done, and is:

- The payload is **scrubbed on eviction** (`zeroize`) across every field that
  carries it — not just `raw_content`, but `display_content` (which *is* the
  payload for non-sensitive items) and any user-set `title`.
- The payload is held in a **page-locked arena** (`LockedRing`) that the kernel
  may not swap: 25 slots of 32 KB, 800 KB locked once at startup and never
  grown. Scrubbing RAM accomplishes nothing if a copy reached the swap device
  first, and swap is very often not encrypted.

What that does **not** cover, stated plainly:

- **Images are not locked.** A 32 KB slot is sized for text (across a real
  history, the largest text item was 10.7 KB); images average ~1 MB and would
  require locking >100 MB permanently. Oversized items stay in the buffer
  unlocked rather than being dropped.
- **Copies handed to the UI are not locked.** Rendering a list or a tray menu
  builds ordinary `String`s. Those are short-lived; the copy the arena protects
  is the one that sits idle for hours, which is the one the kernel picks to
  swap.
- **The lock is best-effort.** If `RLIMIT_MEMLOCK` forbids it the app starts
  anyway and says so in the log — a clipboard manager that refuses to start is
  worse than one that reports it could not lock. Check the startup line rather
  than assuming.

Locking the *whole process* with `mlockall` was tried and reverted: under
WebKit it kills the app. `MCL_ONFAULT` controls page population, not
accounting, so the kernel charges WebKit's multi-GB address space against
`RLIMIT_MEMLOCK` and later allocations fail. No finite limit survives that.

## Development

Requirements: Rust ≥ 1.85. For the desktop app, the Tauri 2 toolchain
(on Linux: `gtk3`, `webkit2gtk-4.1`, `libsoup-3.0`).

```bash
# Core tests
cargo test -p lapacho-core

# Build the entire workspace
cargo build --workspace

# Run the desktop app
cd apps/desktop/src-tauri
# On this environment, trunk is sensitive to color env vars; use the prefix:
env -u NO_COLOR -u CARGO_TERM_COLOR TRUNK_COLOR=always CARGO_TERM_COLOR=never \
  cargo tauri dev
```

To install it for real (binary, icons and desktop entry under `~/.local`, no root):

```bash
./install.sh
```

### Diagnostics

Launched from autostart there is no terminal, so `install.sh` writes a desktop
entry that redirects output to a log. The previous run is kept, because the run
you need to read is usually the one that just died:

```
~/.local/state/lapacho/lapacho.log      # current run
~/.local/state/lapacho/lapacho.log.1    # previous run
```

The log is quiet by default: startup state (including whether the session
buffer actually got locked) and anything that went wrong. No clipboard content
is ever written to it — only errors, content types and timings. Keep it that
way if you add a diagnostic; the file is unencrypted, which is precisely what
the session buffer is not allowed to be.

Per-capture timings are off by default, since they fire on every copy. Turn
them on for a session:

```bash
LAPACHO_TRACE=1 lapacho
```

or add `LAPACHO_TRACE=1` to the `Exec` line in
`~/.local/share/applications/lapacho.desktop` to keep them across restarts.

## Roadmap

- [x] `lapacho-core`: classification, sanitization, storage, plugins, ingest
- [x] Desktop backend: monitor + Tauri commands + keyring + hardening
- [x] Reactive UI refresh on live capture
- [x] At-rest encryption (AES-256-GCM) + key in OS keyring + memory hardening
- [x] Session buffer plaintext held in page-locked memory (`LockedRing`, 800 KB)
      + scrubbed on eviction across every field that carries the payload
- [x] Diagnostics that survive autostart (log file + `LAPACHO_TRACE`)
- [ ] Tray rebuild latency (~290 ms/capture): `get_tray_items` decrypts 100 rows
      on every rebuild, and the native menu is rebuilt whole
- [x] Modular threat scanner + per-item actions (copy/export/plugin)
- [x] Leptos/WASM frontend + rich rendering (Markdown, safe SVG, JSON, Mermaid)
- [x] Native system tray + global shortcut (Ctrl+Shift+Alt+L, Lapacho-exclusive) + launch-to-tray + dynamic tray indicator icon (shows last image thumbnail)
- [x] Full image support (capture, tray thumbnails, metadata sanitization)
- [x] Custom app icon (artistic design: Argentine blue halo + dark green hexagon + lapacho leaf as circuit with golden nodes; source in `icons/lapacho-source.svg`)

Full details and minor pending items: see [`ROADMAP.md`](ROADMAP.md) and [`HANDOFF.md`](HANDOFF.md).

## License

The core (`lapacho-core`) and the app are released under **MIT OR Apache-2.0**.
Quebracho Digital's proprietary plugins and integrations are closed source.
See [`LICENSING.md`](LICENSING.md).
