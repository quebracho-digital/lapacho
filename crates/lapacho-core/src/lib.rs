pub mod crypto;
pub mod detectors;
pub mod ingest;
pub mod llm;
pub mod plugins;
pub mod security;
pub mod storage;
pub mod threats;
pub mod types;

pub use ingest::{mask_display, process_text};
pub use llm::{Spotlight, image_guard, spotlight_text};
pub use threats::{Detector, Threat, ThreatKind, ThreatSeverity, assess, requires_confirmation};
pub use storage::{HistoryRepo, RetentionPolicy, SqliteRepo};
pub use types::{
    ClipboardItem, DetectedType, PersistLevel, PluginDefinition, PluginResponse, Sensitivity,
    UIClipboardItem,
};
