# Example plugins

Copy any of these into your plugins folder (see [docs/PLUGINS.md](../../docs/PLUGINS.md)
for where it is on your system) and it appears the next time you open a clip.

| File | Shows |
|---|---|
| `uppercase.json` | The smallest plugin: a command and its arguments |
| `prefix.json` | A parameter, read from `LAPACHO_PARAM_TEXT` |
| `word-count.json` | Replacing the text with something computed from it |
| `fabric-summarize.json` | One [Fabric](https://github.com/danielmiessler/fabric) pattern, fixed: no typing, so no typo |
| `fabric-pattern.json` | Any Fabric pattern, by name |
| `terms-assistant.json` + `.py` | A long-running plugin (`timeout_secs`) that reuses a built-in through `$LAPACHO_BIN`: it sends *Analyse terms*' request to a model you run (llama.cpp, Ollama, LM Studio), or the text to a Drupal site with `ai_provider_universal_terms`, and keeps the report. Copy both files |

They use `tr`, `sh`, `wc` and `awk` (and `python3` for `terms-assistant`, `fabric` for the Fabric ones), so
they work as they are on Linux and macOS; on Windows they need Git Bash or WSL
on the `PATH`.

**`terms-assistant`, the settings:** it has no form; it reads
`terms-assistant.local.json` next to the script, written once:

```json
{"endpoint": "http://localhost:8080", "model": "qwen3.8-27b", "token": ""}
```

For llama.cpp, Ollama or LM Studio, `endpoint` is the server's base URL
(`http://localhost:8080`, `http://localhost:11434`), and `model` any part of
its id when the server hosts several (`qwen3.8-27b` finds
`unsloth/Qwen3.8-27B-GGUF:Q4_K_M`; if several match, the error lists them).
For Drupal, the full analyze URL (`https://example.org/api/terms/analyze`),
with `user:password` as the token if the site uses Basic auth. With a token
in it, make the file readable only by you (`chmod 600`). Drupal checks every quote against the
text and drops invented ones; a local model's report is passed on as it
is.

**Fabric:** install it and pick a model once with `fabric --setup`; the
plugin uses that, and Lapacho can't see where it points. If it's a provider
rather than a server you run, the clip goes there. `~/go/bin`, where
`go install` puts `fabric`, is added to the `PATH`, since an autostarted app
usually doesn't have it. A typo in a pattern's name only shows up as
Fabric's error, so give your favourite patterns a file each, like
`fabric-summarize.json`. Masked clips never reach a plugin.
