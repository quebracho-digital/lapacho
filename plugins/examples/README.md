# Example plugins

Copy any of these into your plugins folder (see [docs/PLUGINS.md](../../docs/PLUGINS.md)
for where it is on your system) and it appears the next time you open a clip.

| File | Shows |
|---|---|
| `uppercase.json` | The smallest plugin: a command and its arguments |
| `prefix.json` | A parameter, read from `LAPACHO_PARAM_TEXT` |
| `word-count.json` | Replacing the text with something computed from it |
| `terms-assistant.json` + `.py` | A long-running plugin (`timeout_secs`) that reuses a built-in through `$LAPACHO_BIN`: it sends *Analyse terms*' request to a model you run (llama.cpp, Ollama, LM Studio), or the text to a Drupal site with `ai_provider_universal_terms`, and keeps the report. Copy both files |

They use `tr`, `sh`, `wc` and `awk` (and `python3` for `terms-assistant`), so
they work as they are on Linux and macOS; on Windows they need Git Bash or WSL
on the `PATH`.

**`terms-assistant`, the endpoint:** for llama.cpp, Ollama or LM Studio, the
server's base URL (`http://localhost:8080`, `http://localhost:11434`), plus
the model id when the server hosts several. For Drupal, the full analyze URL
(`https://example.org/api/terms/analyze`), with `user:password` in the token
field if the site uses Basic auth. Drupal checks every quote against the
text and drops invented ones; a local model's report is passed on as it
is.
