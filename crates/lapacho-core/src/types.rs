use serde::{Deserialize, Serialize};

/// Sensitivity level for content. Order matters for masking and persist policies:
/// `None < Personal < Credential < Secret`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Sensitivity {
    None,
    Personal,
    Credential,
    Secret,
}

/// Detected content type (for rendering and plugin filtering).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DetectedType {
    Text,
    Svg,
    Url,
    Json,
    Mermaid,
    Markdown,
}

/// Disk persistence policy.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PersistLevel {
    /// Only persists `Sensitivity::None` (default, "paranoid").
    #[default]
    None,
    /// Persists None, Personal, Credential with TTL — never Secret ("balanced").
    Sensitive,
    /// Persists everything, including Secret ("unrestricted").
    All,
}

/// Raw clipboard item. `raw_content` must never be sent to the UI.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ClipboardItem {
    pub id: String,
    /// Original content, intact. Never sent to the UI layer.
    pub raw_content: String,
    /// Sanitized content for display.
    pub display_content: String,
    /// "text" or "image".
    pub content_type: String,
    pub sensitivity: Sensitivity,
    pub detected_type: DetectedType,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// RGBA base64 thumbnail (18×18) for images (used for native tray icons).
    pub thumbnail: Option<String>,
    /// Approximate size in bytes for images (PNG payload) so UI can show "peso".
    pub size: Option<usize>,
}

/// Safe projection of `ClipboardItem` for the UI (no `raw_content`).
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UIClipboardItem {
    pub id: String,
    pub display_content: String,
    pub content_type: String,
    pub sensitivity: Sensitivity,
    pub detected_type: DetectedType,
    pub timestamp: u64,
    /// For images: byte size of the image (for showing peso in list).
    pub size: Option<usize>,
    /// 18x18 RGBA base64 thumbnail (for list previews or native icons if exposed).
    pub thumbnail: Option<String>,
}

impl From<ClipboardItem> for UIClipboardItem {
    fn from(item: ClipboardItem) -> Self {
        // For sensitive items, always compute a fresh short preview from the raw
        // so the UI list and modal show distinguishable values (e.g. ••••1234)
        // even for old history items that were stored with full redaction.
        let display_content = if matches!(item.sensitivity, Sensitivity::Credential | Sensitivity::Secret) {
            let hint = crate::ingest::safe_credential_hint(&item.raw_content);
            if hint.is_empty() {
                "••••••••".to_string()
            } else {
                format!("••••{}", hint)
            }
        } else {
            item.display_content
        };

        UIClipboardItem {
            id: item.id,
            display_content,
            content_type: item.content_type,
            sensitivity: item.sensitivity,
            detected_type: item.detected_type,
            timestamp: item.timestamp,
            size: item.size,
            thumbnail: item.thumbnail,
        }
    }
}

/// Plugin definition deserialized from a `.json` file in the plugins directory.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PluginDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default)]
    pub max_chars: Option<usize>,
    #[serde(default)]
    pub max_words: Option<usize>,
    /// Content types this plugin applies to ("text", "url", "json", etc.).
    /// If omitted, applies to any text item.
    #[serde(default)]
    pub applies_to: Option<Vec<String>>,
}

/// Result of executing a plugin.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PluginResponse {
    pub success: bool,
    pub result_raw_content: String,
    pub result_display_content: String,
    pub sensitivity: Sensitivity,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitivity_orders_correctly() {
        assert!(Sensitivity::None < Sensitivity::Personal);
        assert!(Sensitivity::Personal < Sensitivity::Credential);
        assert!(Sensitivity::Credential < Sensitivity::Secret);
    }

    #[test]
    fn ui_item_drops_raw_content() {
        let item = ClipboardItem {
            id: "1".into(),
            raw_content: "SECRET".into(),
            display_content: "•••".into(),
            content_type: "text".into(),
            sensitivity: Sensitivity::Secret,
            detected_type: DetectedType::Text,
            timestamp: 0,
            thumbnail: None,
            size: None,
        };
        let ui: UIClipboardItem = item.clone().into();
        // For secrets the projection forces a safe hinted display from the raw.
        assert!(ui.display_content.starts_with("••••"));
        assert_eq!(item.raw_content, "SECRET");
    }
}
