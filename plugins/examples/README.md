# Example plugins

Copy any of these into your plugins folder (see [docs/PLUGINS.md](../../docs/PLUGINS.md)
for where it is on your system) and it appears the next time you open a clip.

| File | Shows |
|---|---|
| `uppercase.json` | The smallest plugin: a command and its arguments |
| `prefix.json` | A parameter, read from `LAPACHO_PARAM_TEXT` |
| `word-count.json` | Replacing the text with something computed from it |

They use `tr`, `sh`, `wc` and `awk`, so they work as they are on Linux and
macOS; on Windows they need Git Bash or WSL on the `PATH`.
