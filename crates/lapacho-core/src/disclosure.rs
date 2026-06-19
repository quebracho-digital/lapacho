//! Outbound disclosure policy — the "doble vía" (raw vs. sanitized).
//!
//! A stored item has two faces: `raw_content` (the original, intact) and
//! `display_content` (sanitized, with credentials/secrets redacted). Whenever an
//! item is *decoupled* from the history — copied back to the system clipboard,
//! exported to a file, or sent to a plugin — exactly one of those faces crosses
//! the backend boundary. This module is the single place that decides which one,
//! so every outbound channel behaves identically.
//!
//! The decision is **tied to the active [`PersistLevel`]** (a deliberate product
//! choice — no per-item consent prompt):
//!
//! | `PersistLevel` | Non-secret item | `Secret` item |
//! |----------------|-----------------|---------------|
//! | `None`         | sanitized       | sanitized     |
//! | `Sensitive`    | **raw**         | sanitized     |
//! | `All`          | **raw**         | **raw**       |
//!
//! Rationale: the level the user already accepted for *persistence* is the same
//! trust boundary that governs *disclosure*. If the user runs in the paranoid
//! `None` mode, even an explicit "copy" hands back the sanitized view; only by
//! opting into `Sensitive`/`All` does raw content leave the backend. This keeps
//! the rule auditable in one function instead of scattered across each command.

use crate::types::{ClipboardItem, PersistLevel, Sensitivity};

/// Which face of an item is allowed to leave the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disclosure {
    /// The original content, intact (`raw_content`).
    Raw,
    /// The sanitized, redaction-applied content (`display_content`).
    Sanitized,
}

/// Decides whether `item` may be disclosed raw under the active `level`.
///
/// Returns the decision itself (not the content) so callers can log or surface
/// *why* an export came back masked. See [`disclose`] for the content.
pub fn disclosure_for(item: &ClipboardItem, level: PersistLevel) -> Disclosure {
    match level {
        PersistLevel::None => Disclosure::Sanitized,
        PersistLevel::Sensitive => {
            if item.sensitivity == Sensitivity::Secret {
                Disclosure::Sanitized
            } else {
                Disclosure::Raw
            }
        }
        PersistLevel::All => Disclosure::Raw,
    }
}

/// Resolves the content `item` is allowed to expose under the active `level`.
///
/// Borrows from `item`; callers that need an owned value (clipboard, file) clone
/// it. This is the function every outbound channel (copy / export / plugin)
/// must route through.
pub fn disclose(item: &ClipboardItem, level: PersistLevel) -> &str {
    match disclosure_for(item, level) {
        Disclosure::Raw => &item.raw_content,
        Disclosure::Sanitized => &item.display_content,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DetectedType;

    fn item(sensitivity: Sensitivity) -> ClipboardItem {
        ClipboardItem {
            id: "id".into(),
            raw_content: "RAW".into(),
            display_content: "SANITIZED".into(),
            content_type: "text".into(),
            sensitivity,
            detected_type: DetectedType::Text,
            timestamp: 0,
            thumbnail: None,
        }
    }

    #[test]
    fn none_level_always_sanitizes() {
        for s in [
            Sensitivity::None,
            Sensitivity::Personal,
            Sensitivity::Credential,
            Sensitivity::Secret,
        ] {
            assert_eq!(disclose(&item(s), PersistLevel::None), "SANITIZED");
        }
    }

    #[test]
    fn sensitive_level_discloses_raw_except_secret() {
        assert_eq!(disclose(&item(Sensitivity::None), PersistLevel::Sensitive), "RAW");
        assert_eq!(disclose(&item(Sensitivity::Personal), PersistLevel::Sensitive), "RAW");
        assert_eq!(disclose(&item(Sensitivity::Credential), PersistLevel::Sensitive), "RAW");
        // The one exception: a secret stays masked even when copied explicitly.
        assert_eq!(disclose(&item(Sensitivity::Secret), PersistLevel::Sensitive), "SANITIZED");
    }

    #[test]
    fn all_level_always_discloses_raw() {
        for s in [
            Sensitivity::None,
            Sensitivity::Personal,
            Sensitivity::Credential,
            Sensitivity::Secret,
        ] {
            assert_eq!(disclose(&item(s), PersistLevel::All), "RAW");
        }
    }

    #[test]
    fn decision_matches_content() {
        let it = item(Sensitivity::Secret);
        assert_eq!(disclosure_for(&it, PersistLevel::All), Disclosure::Raw);
        assert_eq!(disclosure_for(&it, PersistLevel::Sensitive), Disclosure::Sanitized);
        assert_eq!(disclosure_for(&it, PersistLevel::None), Disclosure::Sanitized);
    }
}
