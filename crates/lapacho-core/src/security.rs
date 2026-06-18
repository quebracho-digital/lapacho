use crate::types::Sensitivity;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Sanitizes plain text by removing null bytes and control chars (preserving \n, \r, \t).
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\r' || c == '\t' || !c.is_control())
        .collect()
}

/// Sanitizes SVG by removing scripts, inline handlers, and javascript: URLs.
/// NOTE: regex-based sanitizer is bypassable — TODO replace with a real parser (ammonia).
pub fn sanitize_svg(svg_content: &str) -> Result<String, String> {
    let trimmed = svg_content.trim();
    if !trimmed.starts_with("<svg") && !trimmed.contains("<svg") {
        return Err("Not a valid SVG format".to_string());
    }

    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    static FOREIGN_OBJECT_RE: OnceLock<Regex> = OnceLock::new();
    static IFRAME_RE: OnceLock<Regex> = OnceLock::new();
    static OBJECT_RE: OnceLock<Regex> = OnceLock::new();
    static EMBED_RE: OnceLock<Regex> = OnceLock::new();
    static FORM_RE: OnceLock<Regex> = OnceLock::new();
    static EVENT_HANDLER_RE: OnceLock<Regex> = OnceLock::new();
    static JAVASCRIPT_URL_RE: OnceLock<Regex> = OnceLock::new();

    let mut s = svg_content.to_string();
    s = SCRIPT_RE
        .get_or_init(|| Regex::new(r"(?i)<script\b[^>]*>([\s\S]*?)</script>|<script\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = FOREIGN_OBJECT_RE
        .get_or_init(|| Regex::new(r"(?i)<foreignObject\b[^>]*>([\s\S]*?)</foreignObject>|<foreignObject\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = IFRAME_RE
        .get_or_init(|| Regex::new(r"(?i)<iframe\b[^>]*>([\s\S]*?)</iframe>|<iframe\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = OBJECT_RE
        .get_or_init(|| Regex::new(r"(?i)<object\b[^>]*>([\s\S]*?)</object>|<object\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = EMBED_RE
        .get_or_init(|| Regex::new(r"(?i)<embed\b[^>]*>([\s\S]*?)</embed>|<embed\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = FORM_RE
        .get_or_init(|| Regex::new(r"(?i)<form\b[^>]*>([\s\S]*?)</form>|<form\b[^>]*/>").unwrap())
        .replace_all(&s, "").into_owned();
    s = EVENT_HANDLER_RE
        .get_or_init(|| Regex::new(r#"(?i)\bon[a-z]+\s*=\s*(?:"[^"]*"|'[^']*'|[^\s>]+)"#).unwrap())
        .replace_all(&s, "").into_owned();
    s = JAVASCRIPT_URL_RE
        .get_or_init(|| Regex::new(r#"(?i)\b(href|xlink:href)\s*=\s*(?:"\s*javascript:[^"]*"|'\s*javascript:[^']*')"#).unwrap())
        .replace_all(&s, r##"href="#""##).into_owned();

    Ok(s)
}

fn shannon_entropy(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let mut counts = HashMap::new();
    for c in text.chars() {
        *counts.entry(c).or_insert(0u32) += 1;
    }
    let len = text.chars().count() as f64;
    counts.values().fold(0.0, |acc, &n| {
        let p = n as f64 / len;
        acc - p * p.log2()
    })
}

/// Classifies the sensitivity of text content.
pub fn classify_sensitivity(text: &str) -> Sensitivity {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Sensitivity::None;
    }

    static PRIVATE_KEY_RE: OnceLock<Regex> = OnceLock::new();
    static CREDIT_CARD_RE: OnceLock<Regex> = OnceLock::new();
    static API_KEY_RE: OnceLock<Regex> = OnceLock::new();
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
    static PHONE_RE: OnceLock<Regex> = OnceLock::new();

    let priv_key_re = PRIVATE_KEY_RE
        .get_or_init(|| Regex::new(r"(?i)-----BEGIN [A-Z0-9\s_]+ PRIVATE KEY-----").unwrap());
    let cc_re = CREDIT_CARD_RE.get_or_init(|| {
        Regex::new(r"\b(?:4[0-9]{12}(?:[0-9]{3})?|[25][0-9]{14}|6(?:011|5[0-9][0-9])[0-9]{12}|3[47][0-9]{13})\b").unwrap()
    });
    let api_key_re = API_KEY_RE.get_or_init(|| {
        Regex::new(r"(?i)(sk-[a-zA-Z0-9]{48}|ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9]{82}|AKIA[0-9A-Z]{16})").unwrap()
    });
    let email_re = EMAIL_RE
        .get_or_init(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").unwrap());
    let phone_re = PHONE_RE.get_or_init(|| {
        Regex::new(r"\b(?:\+?[0-9]{1,3})?[-.\s]?(?:\(?[0-9]{2,4}\)?)?[-.\s]?[0-9]{3,4}[-.\s]?[0-9]{3,4}\b").unwrap()
    });

    if priv_key_re.is_match(trimmed) || cc_re.is_match(trimmed) {
        return Sensitivity::Secret;
    }

    // High-entropy short token without spaces → likely a password/key
    if !trimmed.contains(char::is_whitespace) && trimmed.len() >= 12 && trimmed.len() <= 64 {
        let has_digit = trimmed.chars().any(|c| c.is_ascii_digit());
        let has_upper = trimmed.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = trimmed.chars().any(|c| c.is_ascii_lowercase());
        let has_special = trimmed.chars().any(|c| !c.is_alphanumeric());
        if has_digit && has_upper && has_lower && has_special && shannon_entropy(trimmed) > 3.8 {
            return Sensitivity::Secret;
        }
    }

    if api_key_re.is_match(trimmed) {
        return Sensitivity::Credential;
    }

    // Long high-entropy token without spaces → likely a token/hash
    if !trimmed.contains(char::is_whitespace) && trimmed.len() >= 32 && trimmed.len() <= 128 {
        let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
        if (!is_url || trimmed.contains('@')) && shannon_entropy(trimmed) > 4.2 {
            return Sensitivity::Credential;
        }
    }

    if email_re.is_match(trimmed) || phone_re.is_match(trimmed) {
        return Sensitivity::Personal;
    }

    Sensitivity::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_text_strips_control_chars() {
        assert_eq!(sanitize_text("Hello\0 World\u{0007}!"), "Hello World!");
    }

    #[test]
    fn sanitize_svg_removes_xss_vectors() {
        let svg = r#"<svg><script>alert(1)</script><rect onload="alert(2)" href="javascript:do_evil()"/></svg>"#;
        let clean = sanitize_svg(svg).unwrap();
        assert!(!clean.contains("<script"));
        assert!(!clean.contains("onload"));
        assert!(!clean.contains("javascript:"));
    }

    #[test]
    fn classify_sensitivity_levels() {
        assert_eq!(classify_sensitivity("Hello there"), Sensitivity::None);
        assert_eq!(classify_sensitivity("test@example.com"), Sensitivity::Personal);
        assert_eq!(
            classify_sensitivity("ghp_123456789012345678901234567890123456"),
            Sensitivity::Credential
        );
        assert_eq!(
            classify_sensitivity("-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA..."),
            Sensitivity::Secret
        );
        // Long public URLs without auth info are not credentials
        assert_eq!(
            classify_sensitivity(
                "https://github.com/rust-lang/rust/commit/8d37aa908f51a44e6d426315df474e76a6b57cc7"
            ),
            Sensitivity::None
        );
        // URL with embedded credentials is a credential
        assert_eq!(
            classify_sensitivity("https://user:password@github.com/rust-lang/rust"),
            Sensitivity::Credential
        );
    }
}
