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

/// True for `http(s)://…` without embedded credentials (`user:pass@host`).
///
/// Public URLs (YouTube short links, commits, docs) often mix upper/lower/digits
/// and punctuation, which trips the high-entropy "password" heuristic. Those
/// are not secrets unless they carry userinfo credentials.
fn is_plain_public_url(text: &str) -> bool {
    let t = text.trim();
    let rest = if let Some(r) = t.strip_prefix("https://") {
        r
    } else if let Some(r) = t.strip_prefix("http://") {
        r
    } else {
        return false;
    };
    // Authority ends at first `/`, `?`, or `#`.
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest);
    !authority.contains('@')
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

/// Strips a leading "recovery key / backup codes / …" label so the body can
/// be classified. Returns the remainder (trimmed); if no label, returns `text`.
fn strip_recovery_label(text: &str) -> &str {
    static LABEL_RE: OnceLock<Regex> = OnceLock::new();
    let re = LABEL_RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(recovery\s*keys?|backup\s*codes?|emergency\s*(kit|key)|account\s*recovery(\s*key)?)\s*[:\-–—]?\s*",
        )
        .unwrap()
    });
    if let Some(m) = re.find(text) {
        text[m.end()..].trim()
    } else {
        text
    }
}

/// True for standard UUID string (public ids — not treated as recovery keys).
fn is_uuid_shape(text: &str) -> bool {
    static UUID_RE: OnceLock<Regex> = OnceLock::new();
    let re = UUID_RE.get_or_init(|| {
        Regex::new(
            r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        )
        .unwrap()
    });
    re.is_match(text)
}

/// BitLocker-style: 8 groups of 6 digits separated by hyphens (48 digits total).
fn is_bitlocker_recovery_key(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^\d{6}(?:-\d{6}){7}$").unwrap());
    re.is_match(text)
}

/// Dashed / spaced recovery keys (FileVault, Auth0, many 2FA emergency kits):
/// several alphanumeric groups of similar length, with digits, not a UUID.
///
/// Examples that match:
/// - `A1B2-C3D4-E5F6-G7H8-I9J0-K1L2` (FileVault-like)
/// - `aB3d-eF6g-hI9j-kL2m-nO5p-qR8s`
///
/// Examples that do **not**:
/// - UUID `8d37aa90-8f51-4477-6d42-6315df474e76`
/// - slug `well-known-host-name` (no digits / low structure)
fn looks_like_dashed_recovery_key(text: &str) -> bool {
    // Normalize space-separated groups to dashed (Apple sometimes shows spaces).
    let normalized: String = if text.contains(' ') && !text.contains('-') {
        text.split_whitespace().collect::<Vec<_>>().join("-")
    } else {
        text.to_string()
    };
    let t = normalized.as_str();

    // Public UUID shape — not treated as a recovery key.
    if is_uuid_shape(t) {
        return false;
    }

    let parts: Vec<&str> = t.split('-').collect();
    if parts.len() < 4 {
        return false;
    }
    if parts.iter().any(|p| p.is_empty()) {
        return false;
    }
    // Groups: alphanumeric only, length 4–8 (covers FileVault 4, BitLocker 6, etc.)
    if !parts
        .iter()
        .all(|p| (4..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return false;
    }
    // At least one digit — pure word slugs stay None.
    if !t.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    // Group lengths should be fairly uniform (recovery kits, not free-form prose).
    let lens: Vec<usize> = parts.iter().map(|p| p.len()).collect();
    let min_l = *lens.iter().min().unwrap_or(&0);
    let max_l = *lens.iter().max().unwrap_or(&0);
    if max_l - min_l > 2 {
        return false;
    }
    // Enough material + non-trivial entropy.
    if t.len() < 19 {
        return false;
    }
    if shannon_entropy(t) < 3.0 {
        return false;
    }
    true
}

/// Multi-line backup/recovery codes (GitHub, many IdPs): ≥4 lines of short
/// alphanumeric tokens (optionally bullet-prefixed).
fn looks_like_backup_code_block(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            l.trim_start_matches(|c: char| matches!(c, '*' | '-' | '•' | '·'))
                .trim()
        })
        .filter(|l| !l.is_empty())
        .collect();
    if lines.len() < 4 {
        return false;
    }
    // Drop a header line if present ("Backup codes", etc.)
    let body: Vec<&str> = if lines[0].chars().any(|c| c.is_whitespace())
        || lines[0].ends_with(':')
    {
        lines[1..].to_vec()
    } else {
        lines
    };
    if body.len() < 4 {
        return false;
    }
    body.iter().all(|l| {
        let len = l.len();
        (6..=16).contains(&len) && l.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// Recovery / emergency / backup key material that the older password/hex
/// heuristics miss (uppercase+digits+dashes, BitLocker groups, code lists).
fn looks_like_recovery_key(text: &str) -> bool {
    let body = strip_recovery_label(text.trim());
    if body.is_empty() {
        return false;
    }
    if is_bitlocker_recovery_key(body) {
        return true;
    }
    if looks_like_dashed_recovery_key(body) {
        return true;
    }
    if looks_like_backup_code_block(body) {
        return true;
    }
    // Body may still have a newline after a label; try first non-empty line alone.
    if body.contains('\n') {
        if let Some(first) = body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
        {
            if is_bitlocker_recovery_key(first) || looks_like_dashed_recovery_key(first) {
                return true;
            }
        }
    }
    false
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

    // Recovery / backup / emergency keys (dashed groups, BitLocker, code lists).
    // Checked early: they often lack mixed case or specials the password rule wants.
    if looks_like_recovery_key(trimmed) {
        return Sensitivity::Secret;
    }

    // High-entropy short token without spaces → likely a password/key.
    // Skip plain public URLs (e.g. https://youtu.be/UxmB4nO8qGU): mixed case +
    // path punctuation looks like a password but is not one.
    if !is_plain_public_url(trimmed)
        && !trimmed.contains(char::is_whitespace)
        && trimmed.len() >= 12
        && trimmed.len() <= 64
    {
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
    if !is_plain_public_url(trimmed)
        && !trimmed.contains(char::is_whitespace)
        && trimmed.len() >= 32
        && trimmed.chars().all(|c| c.is_ascii_hexdigit())
        && shannon_entropy(trimmed) > 3.5
    {
        return Sensitivity::Secret;
    }

    if api_key_re.is_match(trimmed) {
        return Sensitivity::Credential;
    }

    // Long high-entropy token without spaces → likely a token/hash.
    // Plain public URLs are exempt; URLs with embedded credentials are not.
    if !trimmed.contains(char::is_whitespace) && trimmed.len() >= 32 && trimmed.len() <= 128 {
        if !is_plain_public_url(trimmed) && shannon_entropy(trimmed) > 4.2 {
            // user:pass@host → Credential; raw high-entropy blobs too.
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
        // Short YouTube / high-entropy-looking path must not be Secret
        assert_eq!(
            classify_sensitivity("https://youtu.be/UxmB4nO8qGU"),
            Sensitivity::None
        );
        assert_eq!(
            classify_sensitivity("https://www.youtube.com/watch?v=UxmB4nO8qGU"),
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
    fn recovery_keys_are_secret() {
        // FileVault-like: uppercase + digits + dashes, no lowercase
        assert_eq!(
            classify_sensitivity("A1B2-C3D4-E5F6-G7H8-I9J0-K1L2"),
            Sensitivity::Secret
        );
        // Mixed-case dashed groups
        assert_eq!(
            classify_sensitivity("aB3d-eF6g-hI9j-kL2m-nO5p-qR8s"),
            Sensitivity::Secret
        );
        // Space-separated groups (normalized like dashed)
        assert_eq!(
            classify_sensitivity("A1B2 C3D4 E5F6 G7H8 I9J0 K1L2"),
            Sensitivity::Secret
        );
        // BitLocker: 8×6 digits
        assert_eq!(
            classify_sensitivity("123456-789012-345678-901234-567890-123456-789012-345678"),
            Sensitivity::Secret
        );
        // Label prefix must not hide the key
        assert_eq!(
            classify_sensitivity("Recovery Key: A1B2-C3D4-E5F6-G7H8-I9J0-K1L2"),
            Sensitivity::Secret
        );
        assert_eq!(
            classify_sensitivity("Backup codes:\n1a2b3c4d\n9e8f7a6b\nab12cd34\nef56gh78"),
            Sensitivity::Secret
        );
    }

    #[test]
    fn recovery_key_false_positives_stay_none() {
        // Public UUID shape — not a recovery key
        assert_eq!(
            classify_sensitivity("8d37aa90-8f51-4477-6d42-6315df474e76"),
            Sensitivity::None
        );
        // Hyphenated words without digits
        assert_eq!(
            classify_sensitivity("well-known-host-name"),
            Sensitivity::None
        );
        // Too few groups
        assert_eq!(
            classify_sensitivity("A1B2-C3D4-E5F6"),
            Sensitivity::None
        );
    }

    #[test]
    fn svg_coordinates_are_not_phones() {
        assert_eq!(classify_sensitivity("440 148"), Sensitivity::None);
        assert_eq!(classify_sensitivity("352 180"), Sensitivity::None);
    }
}

// temporary - we'll add proper tests via search_replace
