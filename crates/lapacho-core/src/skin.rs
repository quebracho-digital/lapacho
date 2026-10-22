//! Skins: the colours and block order both apps read from one `skin.toml`.
//!
//! ```toml
//! [colors]           # each optional; a missing one keeps the app's own
//! background = "#1E1E1E"
//! key = "#3C3C3C"
//! text = "#FFFFFF"
//! accent = "#2E7D32"
//!
//! [layout]           # top to bottom; a block left out is hidden,
//!                    # except the keys and the action row (space, enter)
//! keyboard = ["strip", "keys", "actions"]
//! ```

use serde::Deserialize;

/// A parsed, valid skin. Colours are ARGB (`0xFFRRGGBB`), what Android takes as is.
#[derive(Debug, Clone, PartialEq)]
pub struct Skin {
    pub background: Option<u32>,
    pub key: Option<u32>,
    pub text: Option<u32>,
    pub accent: Option<u32>,
    pub keyboard: Vec<String>,
}

pub const KEYBOARD_BLOCKS: [&str; 3] = ["strip", "keys", "actions"];

// deny_unknown_fields: a typo ("backgound") should say so, not be ignored.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    colors: Colors,
    #[serde(default)]
    layout: Layout,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Colors {
    background: Option<String>,
    key: Option<String>,
    text: Option<String>,
    accent: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Layout {
    keyboard: Option<Vec<String>>,
}

/// Parses and validates a `skin.toml`. The error is meant for the user.
pub fn parse(text: &str) -> Result<Skin, String> {
    let f: File = basic_toml::from_str(text).map_err(|e| e.to_string())?;
    let keyboard = blocks("keyboard", f.layout.keyboard, &KEYBOARD_BLOCKS)?;
    // Without the action row there is no space or enter: a keyboard, not a skin choice.
    for must in ["keys", "actions"] {
        if !keyboard.iter().any(|b| b == must) {
            return Err(format!("layout.keyboard must keep \"{must}\""));
        }
    }
    Ok(Skin {
        background: color("background", f.colors.background)?,
        key: color("key", f.colors.key)?,
        text: color("text", f.colors.text)?,
        accent: color("accent", f.colors.accent)?,
        keyboard,
    })
}

/// `#RRGGBB` → `0xFFRRGGBB`. Only that form: no alpha, no names.
fn color(name: &str, value: Option<String>) -> Result<Option<u32>, String> {
    let Some(v) = value else { return Ok(None) };
    let hex = v.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    match hex {
        Some(h) => Ok(Some(0xFF00_0000 | u32::from_str_radix(h, 16).unwrap())),
        None => Err(format!("colors.{name}: \"{v}\" is not #RRGGBB")),
    }
}

fn blocks(name: &str, value: Option<Vec<String>>, known: &[&str]) -> Result<Vec<String>, String> {
    let Some(list) = value else { return Ok(known.iter().map(|s| s.to_string()).collect()) };
    for (i, b) in list.iter().enumerate() {
        if !known.contains(&b.as_str()) {
            return Err(format!("layout.{name}: unknown block \"{b}\" (known: {})", known.join(", ")));
        }
        if list[..i].contains(b) {
            return Err(format!("layout.{name}: \"{b}\" twice"));
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_today() {
        let s = parse("").unwrap();
        assert_eq!(s.background, None);
        assert_eq!(s.keyboard, KEYBOARD_BLOCKS);
    }

    #[test]
    fn colours_and_order() {
        let s = parse("[colors]\nkey = \"#0a0B0c\"\n[layout]\nkeyboard = [\"keys\", \"actions\"]").unwrap();
        assert_eq!(s.key, Some(0xFF0A0B0C));
        assert_eq!(s.text, None);
        assert_eq!(s.keyboard, ["keys", "actions"]);
    }

    #[test]
    fn refusals() {
        for bad in [
            "[colors]\nkey = \"red\"",
            "[colors]\nkey = \"#12345\"",
            "[colors]\nbackgound = \"#000000\"",
            "[layout]\nkeyboard = [\"strip\", \"actions\"]",
            "[layout]\nkeyboard = [\"keys\", \"strip\"]",
            "[layout]\nkeyboard = [\"keys\", \"keys\"]",
            "[layout]\nkeyboard = [\"keys\", \"actions\", \"emoji\"]",
            "[layout]\ndesktop = [\"history\"]",
            "[colors",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
