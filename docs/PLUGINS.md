# Writing a plugin

A plugin takes a clip's text and gives back a new text, which Lapacho saves
as a new clip; the original stays. How to use one is in
[USAGE_DESKTOP.md](USAGE_DESKTOP.md#plugins) and
[USAGE_MOBILE.md](USAGE_MOBILE.md#plugins). This page is for writing one.

There are two kinds:

| | External | Built-in |
|---|---|---|
| What it is | Any program, declared by a JSON file | Rust in `lapacho-core`, shipped with the app |
| Desktop | ✅ | ✅ |
| Android | ❌ (an Android app can't run programs of its own) | ✅ |
| Where | the plugins folder (below) | `crates/lapacho-core/src/plugins.rs` |

## External plugins (desktop)

### Where they go

One JSON file per plugin in the app's data folder:

| System | Folder |
|---|---|
| Linux | `~/.local/share/digital.quebracho.lapacho/plugins/` |
| macOS | `~/Library/Application Support/digital.quebracho.lapacho/plugins/` |
| Windows | `%APPDATA%\digital.quebracho.lapacho\plugins\` |

The first time the app starts it creates the folder with one example,
`uppercase.json`. More examples are in [`plugins/examples/`](../plugins/examples/):
copy one into the folder and it shows up the next time you open a clip.

### The definition

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

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | Unique. A file can't take a built-in's id (`replace`, `terms`): it is ignored. |
| `name` | yes | What the list shows. |
| `description` | yes | One line under the name. |
| `command` | yes | The program to run, looked up in `PATH`. `@lapacho` means Lapacho itself (that is how the built-ins run). |
| `args` | yes | Its arguments, fixed. The clip never goes here. |
| `params` | no | Values asked before running, see below. |
| `timeout_secs` | no | Longest it may run, in seconds: 30 if omitted, at most 600. A plugin that asks a model about a whole document needs minutes. |
| `max_chars`, `max_words`, `applies_to` | no | **Read but not enforced yet**: a plugin gets any text clip. Don't rely on them. |

### The contract

- **Input:** the clip's **raw** text on standard input, never as an argument.
  Raw means unmasked: a plugin sees a password if it is run on one, so only
  install plugins you trust. Images don't go to plugins.
- **Parameters:** each declared param arrives as the environment variable
  `LAPACHO_PARAM_<NAME>` (uppercased), never as an argument, so a value can't
  become a flag or a second command. `name` is `[a-z][a-z0-9_]*`. `kind` is
  `"text"` (the default) or `"flag"`, a checkbox that arrives as `1` or `0`.
  Only declared params are passed. A value can't contain a NUL character.
- **Working directory:** the plugins folder.
- **`LAPACHO_BIN`:** the path to Lapacho itself, so a plugin can run a
  built-in and build on it: `"$LAPACHO_BIN" plugin terms < clip` prints the
  request the *Analyse terms* plugin would make.
- **Output:** standard output, read as UTF-8 (invalid bytes become `�`). It
  goes through the same pipeline as anything copied: it is sanitized,
  classified (a plugin that outputs a password gets a masked clip), and saved
  under the active persistence level. Output that is a whole `<svg>…</svg>`
  is sanitized as SVG.
- **Failure:** a non-zero exit code. Whatever the plugin wrote on standard
  error is shown next to the Run button, so write the reason there.
- **Time limit:** 30 seconds, or the plugin's `timeout_secs`, then the
  process is killed.

### Testing one without the app

The contract is stdin to stdout, so a shell is enough:

```sh
printf 'hola\nmundo\n' | LAPACHO_PARAM_TEXT='> ' sh -c 'while IFS= read -r l; do printf "%s%s\n" "$LAPACHO_PARAM_TEXT" "$l"; done'
```

## Built-in plugins

They live in `crates/lapacho-core/src/plugins.rs` and run the same on both
systems: desktop calls `lapacho plugin <id>` (same process boundary,
timeout and sanitizing as an external one), and Android calls the same
Rust function through the bridge, with no process at all.

| id | Name | What it does |
|---|---|---|
| `replace` | Search and replace | Every match of a text or a regular expression (`replace()`) |
| `terms` | Analyse terms | Turns terms and conditions or a privacy policy into a request to paste into an AI assistant (`terms.rs`) |

To add one:

1. A `PluginDefinition` in `builtin_plugins()`, with `command: "@lapacho"`.
2. A branch in `run_builtin_with()` that calls your function. Return
   `Err(reason)` to refuse; keep the reason short and in English.
3. Tests next to the function.
4. Android shows built-ins by id: add the name, the param labels and each
   refusal reason to `pluginName()`, `paramLabel()` and `pluginRefusal()` in
   `MainActivity.kt`, with strings in English and Spanish.
