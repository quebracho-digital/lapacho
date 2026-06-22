use crate::types::Sensitivity;
use ammonia::Builder;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Sanitizes plain text by removing null bytes and control chars (preserving \n, \r, \t).
pub fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\r' || c == '\t' || !c.is_control())
        .collect()
}

/// Sanitizes SVG using a real HTML parser (`ammonia`) instead of regex.
/// This is much more robust against XSS vectors (scripts, event handlers,
/// javascript: urls, foreignObject, etc.).
///
/// Only a safe subset of SVG tags and attributes is allowed. Unknown or
/// dangerous content is stripped. The function still requires the content
/// to look like an SVG (starts with or contains `<svg`).
pub fn sanitize_svg(svg_content: &str) -> Result<String, String> {
    let trimmed = svg_content.trim();
    if !trimmed.starts_with("<svg") && !trimmed.contains("<svg") {
        return Err("Not a valid SVG format".to_string());
    }

    // Safe, commonly used SVG tags for diagrams and simple graphics.
    // We deliberately omit <script>, <foreignObject>, <iframe>, <object>,
    // <embed>, <form>, and similar risky elements.
    let svg_tags: HashSet<&str> = [
        "svg", "g", "path", "rect", "circle", "ellipse", "line", "polyline", "polygon",
        "text", "tspan", "defs", "clipPath", "mask", "linearGradient", "radialGradient",
        "stop", "title", "desc", "use", "symbol", "marker", "pattern", "a",
    ]
    .iter()
    .copied()
    .collect();

    // Safe attributes that are useful for rendering without allowing code execution.
    let svg_attrs: HashSet<&str> = [
        "id",
        "class",
        "style",
        "transform",
        "viewBox",
        "width",
        "height",
        "x",
        "y",
        "cx",
        "cy",
        "r",
        "rx",
        "ry",
        "d",
        "fill",
        "stroke",
        "stroke-width",
        "opacity",
        "font-size",
        "font-family",
        "text-anchor",
        "xlink:href",
        "href",
        "xmlns",
        "xmlns:xlink",
        "version",
        "preserveAspectRatio",
    ]
    .iter()
    .copied()
    .collect();

    // Build a per-tag attribute map (all tags get the same safe attr set).
    let tag_attr_map: HashMap<&str, HashSet<&str>> = svg_tags
        .iter()
        .map(|&tag| (tag, svg_attrs.clone()))
        .collect();

    let cleaned = Builder::default()
        .tags(svg_tags)
        .tag_attributes(tag_attr_map)
        .generic_attributes(svg_attrs.clone())
        // Only allow safe URL schemes. "javascript:" and "vbscript:" are rejected by ammonia.
        .url_schemes(
            ["http", "https", "data", ""]
                .iter()
                .copied()
                .map(Into::into)
                .collect(),
        )
        // Do not inject rel="noopener noreferrer" on <a> tags (we control the content).
        .link_rel(None)
        .clean(svg_content)
        .to_string();

    Ok(cleaned)
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

/// Like [`classify_sensitivity`], but skips email/phone heuristics — for SVG and
/// Mermaid payloads where coordinates look like phone numbers.
pub fn classify_sensitivity_graphics(text: &str) -> Sensitivity {
    let level = classify_sensitivity(text);
    if level == Sensitivity::Personal {
        Sensitivity::None
    } else {
        level
    }
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

    // Long hex strings (common for keys, hashes, tokens) are high sensitivity.
    // Pure hex of 32+ chars has max entropy ~4.0, so won't hit the >4.2 general rule.
    // Treat as Secret (more paranoid than Credential) because these are typically
    // cryptographic material, not "just an API token".
    if !trimmed.contains(char::is_whitespace)
        && trimmed.len() >= 32
        && trimmed.chars().all(|c| c.is_ascii_hexdigit())
        && shannon_entropy(trimmed) > 3.5
    {
        return Sensitivity::Secret;
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

    if email_re.is_match(trimmed) {
        return Sensitivity::Personal;
    }
    if phone_re.find_iter(trimmed).any(|m| looks_like_phone(m.as_str())) {
        return Sensitivity::Personal;
    }

    Sensitivity::None
}

/// Filters [`PHONE_RE`] hits so SVG path coordinates (e.g. `"440 148"`) are not
/// treated as phone numbers. Real phones have 7–15 digits and/or explicit phone
/// punctuation (+, parentheses, dashes).
fn looks_like_phone(candidate: &str) -> bool {
    let digits = candidate.chars().filter(|c| c.is_ascii_digit()).count();
    if !(7..=15).contains(&digits) {
        return false;
    }
    let parts: Vec<&str> = candidate.split_whitespace().collect();
    if parts.len() == 2
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_digit()) && (2..=4).contains(&p.len()))
        && !candidate.contains('+')
        && !candidate.contains('(')
        && !candidate.contains(')')
        && !candidate.contains('-')
        && !candidate.contains('.')
    {
        return false;
    }
    true
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
        assert_eq!(
            classify_sensitivity("+54 9 11 1234-5678"),
            Sensitivity::Personal
        );
        // Long hex (common for keys/hashes) should be Secret even without upper/special
        assert_eq!(
            classify_sensitivity(
                "4242b3f0d97f17ff12766df9812bad75ee0005761fb985011a8aa4ae130e15bc"
            ),
            Sensitivity::Secret
        );
        // 32-char hex too
        assert_eq!(
            classify_sensitivity("0123456789abcdef0123456789abcdef"),
            Sensitivity::Secret
        );
    }

    #[test]
    fn svg_coordinates_are_not_phones() {
        assert_eq!(classify_sensitivity("440 148"), Sensitivity::None);
        assert_eq!(classify_sensitivity("352 180"), Sensitivity::None);
    }
}
