//! Tests for MoE-style model routing preferences.
//!
//! Verifies the `ModelPreference` enum default, the `resolve_model_for_agent`
//! helper, and the round-trip behaviour when both models are identical.

use litho_generator::config::LLMConfig;
use litho_generator::generator::step_forward_agent::ModelPreference;
use litho_generator::llm::client::utils::resolve_model_for_agent;

// ---------------------------------------------------------------------------
// ModelPreference default
// ---------------------------------------------------------------------------

#[test]
fn model_preference_default_is_auto() {
    assert_eq!(ModelPreference::default(), ModelPreference::Auto);
}

// ---------------------------------------------------------------------------
// resolve_model_for_agent — Efficient preference
// ---------------------------------------------------------------------------

#[test]
fn resolve_model_efficient_preference() {
    let config = LLMConfig {
        model_efficient: "small:7b".to_string(),
        model_powerful: "large:70b".to_string(),
        ..Default::default()
    };
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Efficient, "system", "user");
    assert_eq!(primary, "small:7b");
    assert_eq!(fallback, Some("large:70b".to_string()));
}

// ---------------------------------------------------------------------------
// resolve_model_for_agent — Powerful preference
// ---------------------------------------------------------------------------

#[test]
fn resolve_model_powerful_preference() {
    let config = LLMConfig {
        model_efficient: "small:7b".to_string(),
        model_powerful: "large:70b".to_string(),
        ..Default::default()
    };
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Powerful, "system", "user");
    assert_eq!(primary, "large:70b");
    assert_eq!(fallback, Some("small:7b".to_string()));
}

// ---------------------------------------------------------------------------
// resolve_model_for_agent — same model, no fallback
// ---------------------------------------------------------------------------

#[test]
fn resolve_model_same_model_no_fallback_efficient() {
    let config = LLMConfig {
        model_efficient: "qwen:7b".to_string(),
        model_powerful: "qwen:7b".to_string(),
        ..Default::default()
    };
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Efficient, "system", "user");
    assert_eq!(primary, "qwen:7b");
    assert!(fallback.is_none());
}

#[test]
fn resolve_model_same_model_no_fallback_powerful() {
    let config = LLMConfig {
        model_efficient: "qwen:7b".to_string(),
        model_powerful: "qwen:7b".to_string(),
        ..Default::default()
    };
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Powerful, "system", "user");
    assert_eq!(primary, "qwen:7b");
    assert!(fallback.is_none());
}

// ---------------------------------------------------------------------------
// resolve_model_for_agent — Auto delegates to size heuristic
// ---------------------------------------------------------------------------

#[test]
fn resolve_model_auto_short_prompt_uses_efficient() {
    let config = LLMConfig {
        model_efficient: "small:7b".to_string(),
        model_powerful: "large:70b".to_string(),
        ..Default::default()
    };
    // A short prompt (well under 32 KiB) should select the efficient model.
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Auto, "sys", "short user prompt");
    assert_eq!(primary, "small:7b");
    assert_eq!(fallback, Some("large:70b".to_string()));
}

#[test]
fn resolve_model_auto_long_prompt_uses_powerful() {
    let config = LLMConfig {
        model_efficient: "small:7b".to_string(),
        model_powerful: "large:70b".to_string(),
        ..Default::default()
    };
    // A prompt exceeding 32 KiB should select the powerful model.
    let long_prompt = "x".repeat(33 * 1024);
    let (primary, fallback) =
        resolve_model_for_agent(&config, ModelPreference::Auto, "sys", &long_prompt);
    assert_eq!(primary, "large:70b");
    assert!(fallback.is_none());
}

// ---------------------------------------------------------------------------
// resolve_model_for_agent — empty model strings (edge cases)
// ---------------------------------------------------------------------------

#[test]
fn resolve_model_efficient_empty_efficient_falls_back_to_powerful() {
    let config = LLMConfig {
        model_efficient: String::new(),
        model_powerful: "large:70b".to_string(),
        ..Default::default()
    };
    let (primary, fallback) = resolve_model_for_agent(&config, ModelPreference::Efficient, "", "");
    // When efficient is empty, powerful is used as primary; no further fallback.
    assert_eq!(primary, "large:70b");
    assert!(fallback.is_none());
}

#[test]
fn resolve_model_powerful_empty_powerful_falls_back_to_efficient() {
    let config = LLMConfig {
        model_efficient: "small:7b".to_string(),
        model_powerful: String::new(),
        ..Default::default()
    };
    let (primary, fallback) = resolve_model_for_agent(&config, ModelPreference::Powerful, "", "");
    // When powerful is empty, efficient is used as primary; no further fallback.
    assert_eq!(primary, "small:7b");
    assert!(fallback.is_none());
}
