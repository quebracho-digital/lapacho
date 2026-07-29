//! Ingest pipeline: turns a raw clipboard string into a fully-classified,
//! sanitized [`ClipboardItem`].
//!
//! This is the glue that composes [`detectors`](crate::detectors),
//! [`security`](crate::security) and [`types`](crate::types):
//!
//! 1. Classify the content type (text / url / json / svg / …).
//! 2. Classify the sensitivity (none / personal / credential / secret).
//! 3. Sanitize the content (strip control chars; neutralize SVG XSS vectors).
//! 4. Build the display string, masking credentials and secrets.
//!
//! `raw_content` is always preserved intact (it is what gets pasted back);
//! `display_content` is the only field safe to render, and it never carries
//! an unmasked credential or secret.

use crate::detectors::classify_text;
use crate::security::{classify_sensitivity, classify_sensitivity_graphics, sanitize_svg, sanitize_text};
use crate::types::{ClipboardItem, DetectedType, Sensitivity};

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Extracts a very small, safe suffix (last few visible chars) from a credential
/// so the user can tell different credentials apart in the list/tray without
/// revealing the actual value or its full length.
pub fn safe_credential_hint(s: &str) -> String {
    // Take the last up to 4 non-control chars from the end.
    // This gives just enough to distinguish tokens (e.g. ...abc1 vs ...xyz9)
    // while never exposing the bulk of the secret.
    s.chars()
        .rev()
        .filter(|c| !c.is_control())
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Preview seguro para sensibles. Secret: solo ••••last4. Credential: first3…last4
/// (el prefijo del credential ayuda a distinguirlo; nunca se expone para Secret).
pub fn sensitive_display(raw: &str, sensitivity: Sensitivity) -> String {
    let last4: String = raw.chars().rev().filter(|c| !c.is_control())
        .take(4).collect::<Vec<_>>().into_iter().rev().collect();
    match sensitivity {
        Sensitivity::Credential => {
            let first3: String = raw.chars().filter(|c| !c.is_control()).take(3).collect();
            if first3.is_empty() && last4.is_empty() { "••••••••".into() }
            else { format!("{first3}…{last4}") }
        }
        _ => if last4.is_empty() { "••••••••".into() } else { format!("••••{last4}") },
    }
}

/// Builds the user-facing display string for already-sanitized content.
///
/// `None` and `Personal` content is shown as-is.
/// `Credential` and `Secret` get a short safe hint suffix (last few chars)
/// so different creds/secrets are distinguishable in list and tray.
/// We never show the real value or its full length.
pub fn mask_display(sanitized: &str, sensitivity: Sensitivity) -> String {
    match sensitivity {
        Sensitivity::None | Sensitivity::Personal => sanitized.to_string(),
        Sensitivity::Credential | Sensitivity::Secret => {
            sensitive_display(sanitized, sensitivity)
        }
    }
}

/// Processes a raw text clipboard payload into a complete [`ClipboardItem`].
///
/// Generates a fresh UUID and a current timestamp. The returned item is ready
/// to be handed to [`HistoryRepo::save`](crate::storage::HistoryRepo::save);
/// whether it is actually persisted depends on the configured
/// [`PersistLevel`](crate::types::PersistLevel).
pub fn process_text(raw: &str) -> ClipboardItem {
    let detected_type = classify_text(raw);
    // Vector diagrams carry numeric coordinates; skip email/phone heuristics.
    let sensitivity = match detected_type {
        DetectedType::Svg | DetectedType::Mermaid => {
            classify_sensitivity_graphics(raw)
        }
        _ => classify_sensitivity(raw),
    };

    // SVG is sanitized with the XSS-aware sanitizer; everything else just has
    // control characters stripped. If SVG sanitizing fails, fall back to text.
    let sanitized = match detected_type {
        DetectedType::Svg => sanitize_svg(raw).unwrap_or_else(|_| sanitize_text(raw)),
        _ => sanitize_text(raw),
    };

    let display_content = mask_display(&sanitized, sensitivity);

    ClipboardItem {
        id: uuid::Uuid::new_v4().to_string(),
        raw_content: raw.to_string(),
        display_content,
        content_type: "text".to_string(),
        sensitivity,
        detected_type,
        timestamp: now_secs(),
        thumbnail: None,
        size: None,
        title: None,
        pinned: false,
        vaulted: false,
        sync_id: None,
        sync_eligible: sensitivity != Sensitivity::Secret,
        sync_state: "LocalOnly".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_visible_and_intact() {
        let item = process_text("hello world");
        assert_eq!(item.detected_type, DetectedType::Text);
        assert_eq!(item.sensitivity, Sensitivity::None);
        assert_eq!(item.display_content, "hello world");
        assert_eq!(item.raw_content, "hello world");
        assert_eq!(item.content_type, "text");
        assert!(!item.id.is_empty());
    }

    #[test]
    fn classifies_structured_types() {
        assert_eq!(process_text("https://example.com/a").detected_type, DetectedType::Url);
        assert_eq!(process_text(r#"{"a": 1}"#).detected_type, DetectedType::Json);
        assert_eq!(process_text("# Title\nbody").detected_type, DetectedType::Markdown);
    }

    #[test]
    fn personal_content_is_shown() {
        let item = process_text("contact me at test@example.com");
        assert_eq!(item.sensitivity, Sensitivity::Personal);
        assert!(item.display_content.contains("test@example.com"));
    }

    #[test]
    fn credentials_are_redacted_but_raw_is_kept() {
        let token = "ghp_123456789012345678901234567890123456";
        let item = process_text(token);
        assert_eq!(item.sensitivity, Sensitivity::Credential);
        // Credential: first3…last4 (e.g. ghp…3456); Secret: ••••last4. Never full value.
        assert!(item.display_content.contains("ghp"));
        assert!(item.display_content.contains("3456"));
        assert!(item.display_content.contains("…"));
        assert!(!item.display_content.contains(token));
        // raw_content must survive intact so it can be pasted back.
        assert_eq!(item.raw_content, token);
    }

    #[test]
    fn secrets_are_redacted() {
        let key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...";
        let item = process_text(key);
        assert_eq!(item.sensitivity, Sensitivity::Secret);
        // Secrets now get a short safe hint for distinguishability (consistent with
        // credential distinction feature). Never the real value.
        assert!(item.display_content.starts_with("••••"));
        assert!(!item.display_content.contains("MIIEowIBAAKCAQEA"));
        assert_eq!(item.raw_content, key);
    }

    #[test]
    fn svg_display_is_sanitized() {
        let svg = r#"<svg><script>alert(1)</script><rect/></svg>"#;
        let item = process_text(svg);
        assert_eq!(item.detected_type, DetectedType::Svg);
        assert_eq!(item.sensitivity, Sensitivity::None);
        // Non-sensitive SVG is visible, but the script vector is gone.
        assert!(!item.display_content.contains("<script"));
        // The raw is preserved verbatim (sanitizing happens for display only).
        assert!(item.raw_content.contains("<script"));
    }

    #[test]
    fn leo_source_svg_is_not_personal() {
        let svg = include_str!("../../../apps/desktop/src-tauri/icons/lapacho-source.svg");
        let item = process_text(svg);
        assert_eq!(item.detected_type, DetectedType::Svg);
        assert_eq!(item.sensitivity, Sensitivity::None);
    }

    #[test]
    fn control_chars_are_stripped_from_display() {
        let item = process_text("clean\u{0007}text");
        assert_eq!(item.display_content, "cleantext");
    }

    #[test]
    fn redaction_does_not_leak_length() {
        let short = process_text("ghp_123456789012345678901234567890123456");
        let longer = process_text("ghp_abcdefghijklmnopqrstuvwxyz0123456789AB");
        // Both secrets and credentials now use short fixed-width "••••" + hint.
        // The hint length is bounded so we don't leak the original length.
        let secret = process_text("-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA...");
        assert!(secret.display_content.starts_with("••••"));
        assert_eq!(secret.display_content.len(), "••••".len() + 4); // hint is up to 4
        // Real values never shown.
        assert!(!short.display_content.contains("123456789012345678901234567890123456"));
        assert!(!longer.display_content.contains("abcdefghijklmnopqrstuvwxyz0123456789AB"));
    }
}
