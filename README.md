# Lapacho

A **secure** desktop clipboard manager. It classifies, sanitizes, and masks
content before it reaches the UI: credentials and secrets are never shown in
plain text, and the original content is only released when you explicitly paste
it back.

Part of the **Quebracho Digital** ecosystem. Replaces the prototypes
`quebracho-client` and `RustyBoard`.

> **Status:** advanced development. The core (`lapacho-core`) is stable with 50+
> tests. The desktop app (Tauri 2 + Leptos/WASM) has a complete backend, native
> tray, global shortcut Ctrl+Shift+Alt+L (Lapacho-exclusive), image support, rich rendering
> (MD/SVG/Mermaid), and a raw-first security model. Verified headless + GUI smoke
> tests.

## Features

- **Automatic capture** of the clipboard via a background monitor.
- **Type classification:** text, URL, JSON, SVG, Mermaid, Markdown, image.
- **Sensitivity classification:** `None` / `Personal` / `Credential` / `Secret`,
  using regex + Shannon entropy (private keys, API tokens, credit cards, emails,
  etc.).
- **Masking:** credentials and secrets are shown redacted (`••••••••`), never in
  the clear. The `raw_content` is never sent to the UI layer.
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

## Security Model

- The `raw_content` (original content) lives **only in the backend**; the UI
  receives a `UIClipboardItem` that never includes it.
- Credentials and secrets are masked with a fixed-width placeholder that does
  not leak the original length.
- Plugins receive their input via `stdin` (not arguments → no injection) and run
  with a timeout; their output is sanitized before reaching the UI.

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

## Roadmap

- [x] `lapacho-core`: classification, sanitization, storage, plugins, ingest (50+ tests)
- [x] Desktop backend: monitor + Tauri commands + keyring + hardening
- [x] Reactive UI refresh on live capture
- [x] At-rest encryption (AES-256-GCM) + key in OS keyring + memory hardening
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
