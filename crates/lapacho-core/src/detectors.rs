use crate::types::DetectedType;

/// Classifies a text string into one of the `DetectedType` variants.
/// Heuristic, evaluated in priority order.
pub fn classify_text(text: &str) -> DetectedType {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return DetectedType::Text;
    }

    // SVG — tolerate trailing whitespace after `</svg>` (common when copying from editors).
    if (trimmed.starts_with("<svg") || (trimmed.starts_with("<?xml") && trimmed.contains("<svg")))
        && trimmed.contains("</svg>")
    {
        return DetectedType::Svg;
    }

    // URL (no whitespace)
    if (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        && !trimmed.contains(char::is_whitespace)
    {
        return DetectedType::Url;
    }

    // JSON
    if ((trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']')))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return DetectedType::Json;
    }

    // Mermaid
    let lower = trimmed.to_lowercase();
    if lower.starts_with("graph ")
        || lower.starts_with("flowchart ")
        || lower.starts_with("sequencediagram")
        || lower.starts_with("gantt")
        || lower.starts_with("classdiagram")
        || lower.starts_with("statediagram")
        || lower.starts_with("erdiagram")
        || lower.starts_with("journey")
        || lower.starts_with("pie")
        || lower.starts_with("gitgraph")
        || trimmed.starts_with("```mermaid")
    {
        return DetectedType::Mermaid;
    }

    // Markdown
    if trimmed.starts_with("# ")
        || trimmed.starts_with("## ")
        || trimmed.starts_with("### ")
        || trimmed.contains("\n# ")
        || trimmed.contains("\n## ")
        || trimmed.contains("**")
        || (trimmed.contains('[') && trimmed.contains("](") && trimmed.contains(')'))
        || trimmed.starts_with("- ")
        || trimmed.contains("\n- ")
        || trimmed.starts_with("* ")
        || trimmed.contains("\n* ")
        || (trimmed.starts_with("```") && trimmed.ends_with("```"))
    {
        return DetectedType::Markdown;
    }

    DetectedType::Text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_basic_types() {
        assert_eq!(classify_text("hello"), DetectedType::Text);
        assert_eq!(classify_text("https://example.com/a"), DetectedType::Url);
        assert_eq!(classify_text(r#"{"a": 1}"#), DetectedType::Json);
        assert_eq!(classify_text("<svg></svg>"), DetectedType::Svg);
        assert_eq!(classify_text("flowchart TD\n A --> B"), DetectedType::Mermaid);
        assert_eq!(classify_text("# Título\ntexto"), DetectedType::Markdown);
    }
}
