//! Lightweight mirrors of `lapacho-core`'s UI-facing types.
//!
//! The WASM frontend can't depend on `lapacho-core` (it would pull rusqlite and
//! its C deps into the browser), so these structs duplicate the *safe
//! projection* shapes — field names and enum variants must match the backend's
//! serde output exactly. They only ever carry sanitized data; `raw_content`
//! never reaches here except as the explicit result of an export.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Sensitivity {
    None,
    Personal,
    Credential,
    Secret,
}

impl Sensitivity {
    /// CSS class suffix (`s-None`, `s-Secret`, …) and label.
    pub fn label(&self) -> &'static str {
        match self {
            Sensitivity::None => "None",
            Sensitivity::Personal => "Personal",
            Sensitivity::Credential => "Credential",
            Sensitivity::Secret => "Secret",
        }
    }
    pub fn is_sensitive(&self) -> bool {
        matches!(self, Sensitivity::Credential | Sensitivity::Secret)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum DetectedType {
    Text,
    Svg,
    Url,
    Json,
    Mermaid,
    Markdown,
}

impl DetectedType {
    pub fn label(&self) -> &'static str {
        match self {
            DetectedType::Text => "Text",
            DetectedType::Svg => "Svg",
            DetectedType::Url => "Url",
            DetectedType::Json => "Json",
            DetectedType::Mermaid => "Mermaid",
            DetectedType::Markdown => "Markdown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct UIClipboardItem {
    pub id: String,
    pub display_content: String,
    pub content_type: String,
    pub sensitivity: Sensitivity,
    pub detected_type: DetectedType,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum ThreatSeverity {
    Info,
    Warning,
    Danger,
}

impl ThreatSeverity {
    pub fn css(&self) -> &'static str {
        match self {
            ThreatSeverity::Info => "sev-Info",
            ThreatSeverity::Warning => "sev-Warning",
            ThreatSeverity::Danger => "sev-Danger",
        }
    }
    pub fn icon(&self) -> &'static str {
        match self {
            ThreatSeverity::Info => "ℹ",
            ThreatSeverity::Warning => "⚠",
            ThreatSeverity::Danger => "⛔",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum ThreatKind {
    ActiveContent,
    EmbeddedFrame,
    BidiOverride,
    ZeroWidth,
    ControlChars,
    SensitiveData,
    PromptInjection,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Threat {
    pub kind: ThreatKind,
    pub severity: ThreatSeverity,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ExportResult {
    pub content: String,
    pub threats: Vec<Threat>,
}

/// Subset of the backend `PluginDefinition` — serde ignores the rest.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct PluginDef {
    pub id: String,
    pub name: String,
    pub description: String,
}
