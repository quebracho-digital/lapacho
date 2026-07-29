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

impl PersistLevel {
    /// Whether an item of this sensitivity is written to disk at this level,
    /// ignoring any per-item override.
    ///
    /// The single source of truth for the policy: `save` uses it to decide
    /// whether to write, and un-vaulting uses it to decide whether the item may
    /// stay. Two copies of this rule would drift into an item that survives a
    /// level the user believes forbids it.
    pub fn persists(self, sensitivity: Sensitivity) -> bool {
        match self {
            PersistLevel::None => sensitivity == Sensitivity::None,
            PersistLevel::Sensitive => sensitivity != Sensitivity::Secret,
            PersistLevel::All => true,
        }
    }
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
    /// User-given name for the item, so it can be found by what it *is* rather
    /// than by its content. Encrypted at rest like the content itself.
    pub title: Option<String>,
    /// User asked to keep this item: exempt from the max-items cap. Does
    /// **not** override `PersistLevel` nor the sensitive TTL — for that, see
    /// `vaulted`.
    pub pinned: bool,
    /// User explicitly put this item in the vault: it is written to disk even
    /// when the active `PersistLevel` would refuse it, and it is exempt from
    /// the sensitive TTL. Still encrypted at rest like everything else.
    ///
    /// This is the one deliberate hole in the persist policy, and it only
    /// opens per item, by an explicit user act. Clearing it re-applies the
    /// active level immediately (see `PersistLevel::persists`).
    pub vaulted: bool,
    /// Sync placeholder fields for multi-device sync (P0-P4 mobile roadmap)
    pub sync_id: Option<String>,
    pub sync_eligible: bool,
    pub sync_state: String,
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
    pub title: Option<String>,
    pub pinned: bool,
    pub vaulted: bool,
}

impl From<ClipboardItem> for UIClipboardItem {
    fn from(item: ClipboardItem) -> Self {
        // For sensitive items, always compute a fresh short preview from the raw
        // so the UI list and modal show distinguishable values (e.g. ••••1234 or ghp…1234)
        // even for old history items that were stored with full redaction.
        let display_content = if matches!(item.sensitivity, Sensitivity::Credential | Sensitivity::Secret) {
            crate::ingest::sensitive_display(&item.raw_content, item.sensitivity)
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
            title: item.title,
            pinned: item.pinned,
            vaulted: item.vaulted,
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
            title: None,
            pinned: false,
            vaulted: false,
            sync_id: None,
            sync_eligible: false,
            sync_state: "LocalOnly".to_string(),
        };
        let ui: UIClipboardItem = item.clone().into();
        // For secrets the projection forces a safe hinted display from the raw.
        assert!(ui.display_content.starts_with("••••"));
        assert_eq!(item.raw_content, "SECRET");
    }
}
