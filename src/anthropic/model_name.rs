use regex::Regex;
use std::sync::LazyLock;

use crate::openai_types::ModelsResponse;

static MODEL_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(claude-(?:sonnet|opus)-4-)(\d{1,2})(.*)$").unwrap()
});

static DATE_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{8,}$").unwrap()
});

pub fn translate_model_name(model: &str) -> String {
    if let Some(caps) = MODEL_VERSION_RE.captures(model) {
        let prefix = caps.get(1).unwrap().as_str();
        let version = caps.get(2).unwrap().as_str();
        let rest = caps.get(3).unwrap().as_str();

        // If the rest after the prefix is an 8+ digit date, strip it
        // e.g. claude-opus-4-20250514 → claude-opus-4
        if rest.is_empty() && DATE_SUFFIX_RE.is_match(version) {
            return prefix.trim_end_matches('-').to_string();
        }

        // Check if the full suffix (version+rest minus leading dash) is a date
        let full_suffix = format!("{}{}", version, rest);
        if DATE_SUFFIX_RE.is_match(&full_suffix) {
            return prefix.trim_end_matches('-').to_string();
        }

        // Convert dash to dot: claude-opus-4-6 → claude-opus-4.6
        return format!("{}{}{}", prefix.trim_end_matches('-'), ".", format!("{}{}", version, rest));
    }

    // Check for date suffix on other model patterns
    // e.g. claude-opus-4-20250514 where it doesn't match the version regex
    model.to_string()
}

/// Resolve a translated model name against the available models list.
/// If the exact model isn't available, tries common suffixes like `-internal`.
/// Falls back to the original name if no match is found.
pub fn resolve_model_name(model: &str, models: &ModelsResponse) -> String {
    // Exact match — no change needed
    if models.data.iter().any(|m| m.id == model) {
        return model.to_string();
    }

    // Try with `-internal` suffix (e.g. claude-opus-4.7-1m → claude-opus-4.7-1m-internal)
    let with_internal = format!("{}-internal", model);
    if models.data.iter().any(|m| m.id == with_internal) {
        return with_internal;
    }

    // No match found, return original and let the upstream API error
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_name_translation() {
        assert_eq!(translate_model_name("claude-opus-4-6"), "claude-opus-4.6");
        assert_eq!(translate_model_name("claude-opus-4-6-1m"), "claude-opus-4.6-1m");
        assert_eq!(translate_model_name("claude-sonnet-4-1"), "claude-sonnet-4.1");
        assert_eq!(translate_model_name("claude-sonnet-4-1-1m"), "claude-sonnet-4.1-1m");
        assert_eq!(translate_model_name("claude-opus-4-20250514"), "claude-opus-4");
        assert_eq!(translate_model_name("gpt-4o"), "gpt-4o");
        assert_eq!(translate_model_name("claude-3-5-sonnet"), "claude-3-5-sonnet");
    }

    fn make_models(ids: &[&str]) -> ModelsResponse {
        use crate::openai_types::Model;
        ModelsResponse {
            data: ids.iter().map(|id| Model {
                id: id.to_string(),
                object: None,
                created: None,
                owned_by: None,
                capabilities: None,
            }).collect(),
            object: None,
        }
    }

    #[test]
    fn test_resolve_model_name_exact_match() {
        let models = make_models(&["claude-opus-4.7", "claude-opus-4.7-1m-internal"]);
        assert_eq!(resolve_model_name("claude-opus-4.7", &models), "claude-opus-4.7");
    }

    #[test]
    fn test_resolve_model_name_internal_fallback() {
        let models = make_models(&["claude-opus-4.7", "claude-opus-4.7-1m-internal"]);
        // claude-opus-4.7-1m doesn't exist, but claude-opus-4.7-1m-internal does
        assert_eq!(resolve_model_name("claude-opus-4.7-1m", &models), "claude-opus-4.7-1m-internal");
    }

    #[test]
    fn test_resolve_model_name_no_match() {
        let models = make_models(&["claude-opus-4.7", "claude-opus-4.7-1m-internal"]);
        // Unknown model passes through unchanged
        assert_eq!(resolve_model_name("gpt-4o", &models), "gpt-4o");
    }
}
