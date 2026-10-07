use crate::security::{classify_sensitivity, sanitize_svg, sanitize_text};
use crate::types::{ParamKind, PluginDefinition, PluginParam, PluginResponse, Sensitivity};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a plugin process may run before being killed, unless it sets
/// `timeout_secs` (up to [`MAX_TIMEOUT_SECS`]).
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_TIMEOUT_SECS: u64 = 600;

/// A plugin `command` that means "this program". The built-in plugins run as
/// `<lapacho> plugin <id>` ([`run_builtin`]), across the same process
/// boundary, timeout and output sanitizing as anyone else's — and with no
/// interpreter to install on any of the three systems.
pub const SELF_COMMAND: &str = "@lapacho";

/// The plugins that ship with Lapacho, listed before the user's.
pub fn builtin_plugins() -> Vec<PluginDefinition> {
    let param = |name: &str, label: &str, kind| PluginParam { name: name.into(), label: label.into(), kind };
    let builtin = |id: &str, name: &str, description: &str, params| PluginDefinition {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        command: SELF_COMMAND.to_string(),
        args: vec!["plugin".to_string(), id.to_string()],
        max_chars: None,
        max_words: None,
        applies_to: None,
        params,
        timeout_secs: None,
    };
    vec![
        builtin(
            "replace",
            "Search and replace",
            "Replaces every match of a text, or of a regular expression",
            vec![
                param("search", "Search", ParamKind::Text),
                param("replace", "Replace with", ParamKind::Text),
                param("regex", "Regular expression", ParamKind::Flag),
            ],
        ),
        builtin(
            "terms",
            "Analyse terms",
            "Turns terms and conditions or a privacy policy into a request to paste into your AI assistant",
            Vec::new(),
        ),
    ]
}

/// The environment variable a parameter reaches the plugin as.
fn param_env(name: &str) -> String {
    format!("LAPACHO_PARAM_{}", name.to_ascii_uppercase())
}

/// `[a-z][a-z0-9_]*`: it becomes part of an environment variable's name.
fn valid_param_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `text` with every `search` replaced by `with` — literally, or as a
/// regular expression whose groups `with` can use (`$1`, `${name}`). Finding
/// nothing is an error, so the window says so instead of storing a copy of
/// the item. The `regex` crate runs in linear time: no pattern can hang it.
pub fn replace(text: &str, search: &str, with: &str, regex: bool) -> Result<String, String> {
    if search.is_empty() {
        return Err("Nothing to search for".to_string());
    }
    if regex {
        let re = regex::Regex::new(search).map_err(|e| format!("Invalid regular expression: {e}"))?;
        if !re.is_match(text) {
            return Err("No matches".to_string());
        }
        return Ok(re.replace_all(text, with).into_owned());
    }
    if !text.contains(search) {
        return Err("No matches".to_string());
    }
    Ok(text.replace(search, with))
}

/// Runs the built-in plugin `id` over `input` in this process, each parameter
/// read through `param`. One dispatch for both hosts: desktop reaches it as
/// `lapacho plugin <id>` ([`run_builtin`]), the Android app through the
/// bridge, with no process at all.
pub fn run_builtin_with(id: &str, input: &str, param: impl Fn(&str) -> String) -> Result<String, String> {
    match id {
        "replace" => replace(input, &param("search"), &param("replace"), param("regex") == "1"),
        "terms" if input.trim().is_empty() => Err("Nothing to analyse".to_string()),
        "terms" => Ok(crate::terms::assistant_request(input)),
        other => Err(format!("No built-in plugin '{other}'")),
    }
}

/// Runs a built-in plugin when this program was started as one —
/// `<lapacho> plugin <id>`, the input on stdin, the parameters in the
/// environment — and returns its exit code. `None` when the arguments are
/// not a plugin call, so the app starts as usual.
pub fn run_builtin(args: &[String]) -> Option<i32> {
    let [_, call, id] = args else { return None };
    if call != "plugin" {
        return None;
    }
    let mut input = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("Could not read the input: {e}");
        return Some(1);
    }
    let out = run_builtin_with(id, &input, |name| std::env::var(param_env(name)).unwrap_or_default());
    match out {
        Ok(text) => {
            let mut stdout = std::io::stdout();
            if stdout.write_all(text.as_bytes()).and_then(|_| stdout.flush()).is_err() {
                return Some(1);
            }
            Some(0)
        }
        Err(e) => {
            eprintln!("{e}");
            Some(1)
        }
    }
}

/// Creates the plugins directory and seeds an example plugin if it doesn't exist yet.
pub fn init_plugins_dir(plugins_dir: &Path) -> Result<(), String> {
    if plugins_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(plugins_dir).map_err(|e| e.to_string())?;

    let example = PluginDefinition {
        id: "uppercase".to_string(),
        name: "Uppercase Converter".to_string(),
        description: "Converts text to uppercase".to_string(),
        command: "tr".to_string(),
        args: vec!["a-z".to_string(), "A-Z".to_string()],
        max_chars: None,
        max_words: None,
        applies_to: None,
        params: Vec::new(),
        timeout_secs: None,
    };
    let content = serde_json::to_string_pretty(&example).map_err(|e| e.to_string())?;
    fs::write(plugins_dir.join("uppercase.json"), content).map_err(|e| e.to_string())?;
    Ok(())
}

/// The built-in plugins, then every definition (`*.json`) in the plugins
/// directory. A file can't take a built-in's id.
pub fn load_plugins(plugins_dir: &Path) -> Result<Vec<PluginDefinition>, String> {
    let mut plugins = builtin_plugins();
    if let Ok(entries) = fs::read_dir(plugins_dir) {
        for entry in entries.flatten() {
            let path: PathBuf = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Ok(content) = fs::read_to_string(&path)
                && let Ok(plugin) = serde_json::from_str::<PluginDefinition>(&content)
                && !plugins.iter().any(|p| p.id == plugin.id)
            {
                plugins.push(plugin);
            }
        }
    }
    Ok(plugins)
}

/// Executes a plugin by passing `input_text` via stdin and sanitizing the output.
///
/// Security model: raw content goes in via stdin and `params` via the
/// environment (not args — no command injection). Only the parameters the
/// plugin declares are passed; one it does not declare is refused. Output is
/// sanitized before it can reach the UI.
pub fn execute_plugin(
    plugins_dir: &Path,
    plugin_id: &str,
    input_text: &str,
    params: &HashMap<String, String>,
) -> Result<PluginResponse, String> {
    let plugins = load_plugins(plugins_dir)?;
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;

    if let Some(name) = params.keys().find(|k| !plugin.params.iter().any(|p| &p.name == *k)) {
        return Err(format!("Plugin '{}' has no parameter '{name}'", plugin.name));
    }
    let mut command = if plugin.command == SELF_COMMAND {
        Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
    } else {
        Command::new(&plugin.command)
    };
    for p in &plugin.params {
        if !valid_param_name(&p.name) {
            return Err(format!("Plugin '{}' has an invalid parameter name '{}'", plugin.name, p.name));
        }
        let value = params.get(&p.name).map(String::as_str).unwrap_or("");
        // The one thing an environment variable cannot hold.
        if value.contains('\0') {
            return Err(format!("'{}' contains a NUL character", p.label));
        }
        command.env(param_env(&p.name), value);
    }

    // Where Lapacho is, so a plugin can run a built-in (`$LAPACHO_BIN plugin
    // terms`) and build on it.
    if let Ok(exe) = std::env::current_exe() {
        command.env("LAPACHO_BIN", exe);
    }
    let timeout = plugin.timeout_secs.map_or(PLUGIN_TIMEOUT, |s| Duration::from_secs(s.clamp(1, MAX_TIMEOUT_SECS)));
    let mut child = command
        .args(&plugin.args)
        .current_dir(plugins_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn '{}': {}", plugin.command, e))?;

    // Write stdin in a thread to avoid deadlock on large payloads
    if let Some(mut stdin) = child.stdin.take() {
        let input = input_text.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }

    // Drain stdout/stderr in dedicated threads so buffers never fill up during polling
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = stdout_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut p) = stderr_pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Plugin '{}' timed out after {}s",
                        plugin.name,
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    let stdout_bytes = stdout_h.join().unwrap_or_default();
    let stderr_bytes = stderr_h.join().unwrap_or_default();

    if status.success() {
        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let trimmed = stdout.trim();
        let (display_content, sensitivity) =
            if trimmed.starts_with("<svg") && trimmed.ends_with("</svg>") {
                match sanitize_svg(&stdout) {
                    Ok(clean) => (clean, Sensitivity::None),
                    Err(_) => (sanitize_text(&stdout), classify_sensitivity(&stdout)),
                }
            } else {
                (sanitize_text(&stdout), classify_sensitivity(&stdout))
            };

        Ok(PluginResponse {
            success: true,
            result_raw_content: stdout,
            result_display_content: display_content,
            sensitivity,
            error: None,
        })
    } else {
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
        let message = if stderr.trim().is_empty() {
            format!("Plugin '{}' exited with {} and produced no output", plugin.name, status)
        } else {
            stderr
        };
        Ok(PluginResponse {
            success: false,
            result_raw_content: String::new(),
            result_display_content: String::new(),
            sensitivity: Sensitivity::None,
            error: Some(message),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_seeds_example_and_loads_it() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        init_plugins_dir(&dir).unwrap();
        let plugins = load_plugins(&dir).unwrap();
        assert!(plugins.iter().any(|p| p.id == "uppercase"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn executes_uppercase_via_stdin() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_exec_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        init_plugins_dir(&dir).unwrap();
        let resp = execute_plugin(&dir, "uppercase", "hola", &HashMap::new()).unwrap();
        assert!(resp.success);
        assert_eq!(resp.result_raw_content.trim(), "HOLA");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_plugin_sets_its_own_time_limit() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_timeout_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("slow.json"), r#"{"id":"slow","name":"Slow","description":"","command":"sleep","args":["3"],"timeout_secs":1}"#).unwrap();
        let err = execute_plugin(&dir, "slow", "x", &HashMap::new()).unwrap_err();
        assert!(err.contains("timed out after 1s"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_plugin_can_find_lapacho_to_run_a_built_in() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_bin_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bin.json"), r#"{"id":"bin","name":"Bin","description":"","command":"sh","args":["-c","printf %s \"$LAPACHO_BIN\""]}"#).unwrap();
        let resp = execute_plugin(&dir, "bin", "", &HashMap::new()).unwrap();
        assert_eq!(resp.result_raw_content, std::env::current_exe().unwrap().to_string_lossy());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_literal_and_regex() {
        assert_eq!(replace("a.b.c", ".", "-", false).unwrap(), "a-b-c");
        // As a regex the dot is any character.
        assert_eq!(replace("ab", ".", "-", true).unwrap(), "--");
        assert_eq!(
            replace("2026-10-02", r"(\d+)-(\d+)-(\d+)", "$3/$2/$1", true).unwrap(),
            "02/10/2026"
        );
    }

    #[test]
    fn replace_reports_what_went_wrong() {
        assert_eq!(replace("hola", "x", "y", false).unwrap_err(), "No matches");
        assert_eq!(replace("hola", "", "y", false).unwrap_err(), "Nothing to search for");
        assert!(replace("hola", "(", "y", true).unwrap_err().starts_with("Invalid regular expression"));
    }

    #[test]
    fn builtins_are_listed_first_and_a_file_cannot_take_their_id() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_shadow_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("evil.json"), r#"{"id":"replace","name":"x","description":"","command":"true","args":[]}"#).unwrap();
        let plugins = load_plugins(&dir).unwrap();
        let replaces: Vec<_> = plugins.iter().filter(|p| p.id == "replace").collect();
        assert_eq!(replaces.len(), 1);
        assert_eq!(replaces[0].command, SELF_COMMAND);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn params_reach_the_plugin_as_environment_and_undeclared_ones_are_refused() {
        let dir = std::env::temp_dir().join(format!("lp_plugins_params_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("echo.json"),
            r#"{"id":"echo","name":"Echo","description":"","command":"sh","args":["-c","printf %s \"$LAPACHO_PARAM_WORD\""],
                "params":[{"name":"word","label":"Word"}]}"#,
        )
        .unwrap();
        // A value that would be a flag or a second command as an argument.
        let word = HashMap::from([("word".to_string(), "--x; rm -rf ~".to_string())]);
        let resp = execute_plugin(&dir, "echo", "", &word).unwrap();
        assert_eq!(resp.result_raw_content, "--x; rm -rf ~");

        let other = HashMap::from([("path".to_string(), "/".to_string())]);
        assert!(execute_plugin(&dir, "echo", "", &other).unwrap_err().contains("no parameter 'path'"));
        let _ = fs::remove_dir_all(&dir);
    }
}
