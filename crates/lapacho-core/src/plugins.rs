use crate::security::{classify_sensitivity, sanitize_svg, sanitize_text};
use crate::types::{PluginDefinition, PluginResponse, Sensitivity};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Maximum time a plugin process may run before being killed.
const PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);

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
    };
    let content = serde_json::to_string_pretty(&example).map_err(|e| e.to_string())?;
    fs::write(plugins_dir.join("uppercase.json"), content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Loads all plugin definitions (`*.json`) from the plugins directory.
pub fn load_plugins(plugins_dir: &Path) -> Result<Vec<PluginDefinition>, String> {
    let mut plugins = Vec::new();
    if let Ok(entries) = fs::read_dir(plugins_dir) {
        for entry in entries.flatten() {
            let path: PathBuf = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Ok(content) = fs::read_to_string(&path)
                && let Ok(plugin) = serde_json::from_str::<PluginDefinition>(&content)
            {
                plugins.push(plugin);
            }
        }
    }
    Ok(plugins)
}

/// Executes a plugin by passing `input_text` via stdin and sanitizing the output.
///
/// Security model: raw content goes in via stdin (not args — no command injection).
/// Output is sanitized before it can reach the UI.
pub fn execute_plugin(
    plugins_dir: &Path,
    plugin_id: &str,
    input_text: &str,
) -> Result<PluginResponse, String> {
    let plugins = load_plugins(plugins_dir)?;
    let plugin = plugins
        .into_iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| format!("Plugin '{}' not found", plugin_id))?;

    let mut child = Command::new(&plugin.command)
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
                if start.elapsed() >= PLUGIN_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "Plugin '{}' timed out after {}s",
                        plugin.name,
                        PLUGIN_TIMEOUT.as_secs()
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
        let resp = execute_plugin(&dir, "uppercase", "hola").unwrap();
        assert!(resp.success);
        assert_eq!(resp.result_raw_content.trim(), "HOLA");
        let _ = fs::remove_dir_all(&dir);
    }
}
