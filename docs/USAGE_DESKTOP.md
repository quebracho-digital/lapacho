# Using Lapacho on the desktop

Lapacho watches your clipboard, keeps a searchable history, and makes sure
passwords and other secrets are never shown in plain text. It lives in the
system tray.

## Install and start

```bash
./install.sh
```

Installs the binary, icons and a desktop entry under `~/.local` (no root).
Start it from your application menu; it goes straight to the tray. Building
from source is covered in the [README](../README.md#development).

## Shortcuts

| Shortcut | Action |
|----------|--------|
| **Ctrl+Shift+Alt+L** | Show / hide the main window. |
| **Ctrl+Shift+Alt+V** | Quick search: opens a search box over your apps. Type to filter, ↑/↓ to move, **Enter** copies the highlighted clip and closes it, **Esc** closes. |

## Tray menu

Click the tray icon for your recent clips — pick one to copy it back to the
clipboard — plus **Buscar…** (quick search), **Open Lapacho…** and **Quit**.
Pinned (and vaulted) clips come first, then a separator, then the rest in the
order you copied them. When the last copy is an image, the tray icon shows its
thumbnail — pinned items don't change that.

## The main window

Every clip is captured automatically. Each row has these actions:

| Button | Action |
|--------|--------|
| ⧉ | Copy the original content back to the clipboard. |
| ⤢ | Show it maximized, with rendering for Markdown, JSON, SVG and Mermaid. |
| 🔒 | "This is a secret": mask it, and remember that for this content. |
| 🏷 | Give it a name, so you can find it by what it is (Enter saves, Esc cancels). |
| 📍 / 📌 | Pin: exempt from the history size limit. |
| 💾 / 🗄 | Vault: keep it on disk even if the persistence mode would drop it (asks for a second click on secrets). |
| ✕ | Delete. |

**Search** filters the whole history, names included.

## Secrets

Lapacho classifies every clip as none / personal / credential / secret, using
the same rules on desktop and Android: passwords, API tokens, private keys,
card numbers, recovery codes, emails and phone numbers.

- Credentials show as `first3…last4`, secrets as `••••last4`: enough to tell
  two apart, never the full value or its length.
- Copying an item back (⧉, tray, quick search) always copies the **original**
  content — masking only affects what is shown.
- If Lapacho missed one, mark it with 🔒.

## Persistence

Choose what may be written to disk (the history is always encrypted):

| Mode | Stores |
|------|--------|
| **Paranoia** (default) | Only non-sensitive clips. |
| **Balanced** | Everything except secrets; credentials expire after the TTL. |
| **All** | Everything. |

**Sensitive TTL** (30 min, 2 h, 8 h or no limit) sets how long credentials
stay. Clips the mode does not store still appear during the session; they are
wiped from memory when they leave it. The vault (💾) is the per-item exception.

## Plugins

Plugins transform a clip with an external command. Each is a JSON file in
`~/.local/share/digital.quebracho.lapacho/plugins/`:

```json
{
  "id": "uppercase",
  "name": "Uppercase Converter",
  "description": "Converts text to uppercase",
  "command": "tr",
  "args": ["a-z", "A-Z"]
}
```

The clip is passed on standard input (never as an argument), the command has a
timeout, and its output is sanitized before it is shown. The result is a new
clip; the original is left as it was.

Open a clip (⤢), pick the plugin, **Run**. If it fails — a wrong regular
expression, nothing matched — the reason shows next to the button.

### Search and replace

Built in, always first in the list. Type what to **Search** and what to
**Replace with**; every occurrence is replaced. Tick **Regular expression** to
search with a pattern, and use its groups in the replacement: search
`(\d+)-(\d+)-(\d+)`, replace with `$3/$2/$1`, and `2026-10-02` becomes
`02/10/2026`. If nothing matches it says so instead of making a copy.

### Plugins that ask for values

A plugin can declare `params`; the window shows a field for each before it
runs, and the command gets each value as the environment variable
`LAPACHO_PARAM_<NAME>` — again never as an argument, so a value can't turn
into a flag or a second command:

```json
{
  "id": "prefix",
  "name": "Prefix every line",
  "description": "Puts a text before each line",
  "command": "sh",
  "args": ["-c", "while IFS= read -r l; do printf '%s%s\\n' \"$LAPACHO_PARAM_TEXT\" \"$l\"; done"],
  "params": [{ "name": "text", "label": "Prefix" }]
}
```

`name` is lowercase letters, digits and `_`. `kind` is `"text"` (the default)
or `"flag"`, a checkbox that arrives as `1` or `0`.

## Where things live (Linux)

| What | Where |
|------|-------|
| History (encrypted) | `~/.local/share/digital.quebracho.lapacho/history.db` |
| Encryption key | OS keyring (`digital.quebracho.lapacho` / `history-encryption-key`); fallback `history.key` next to the DB |
| Log | `~/.local/state/lapacho/lapacho.log` (previous run: `.log.1`) |

The log never contains clipboard content. For per-capture timings, start with
`LAPACHO_TRACE=1 lapacho`.
