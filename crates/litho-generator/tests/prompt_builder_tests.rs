//! Tests for prompt-building utilities: TokenEstimator, CompressionConfig,
//! ProviderClient construction (Ollama — no API key needed),
//! and the CodexRsClient constructor.
//!
//! No network calls or running LLM are required; every test works offline.

mod common;

use litho_generator::{
    config::LLMProvider,
    llm::client::{codex_provider::CodexRsClient, providers::ProviderClient},
    utils::{
        prompt_compressor::{CompressionConfig, PreservePattern},
        token_estimator::{TokenCalculationRules, TokenEstimator},
    },
};

// ---------------------------------------------------------------------------
// TokenEstimator
// ---------------------------------------------------------------------------

#[test]
fn token_estimator_empty_string() {
    let est = TokenEstimator::new();
    let result = est.estimate_tokens("");
    // Base overhead still applies
    assert!(
        result.estimated_tokens > 0,
        "even empty string has base overhead"
    );
    assert_eq!(result.character_count, 0);
    assert_eq!(result.chinese_char_count, 0);
    assert_eq!(result.english_char_count, 0);
}

#[test]
fn token_estimator_ascii_only() {
    let est = TokenEstimator::new();
    let text = "Hello World 123";
    let result = est.estimate_tokens(text);
    assert!(result.character_count > 0);
    assert_eq!(result.chinese_char_count, 0);
    assert!(result.english_char_count > 0);
    assert!(result.estimated_tokens >= 1);
}

#[test]
fn token_estimator_chinese_text() {
    let est = TokenEstimator::new();
    let text = "你好世界"; // 4 Chinese characters
    let result = est.estimate_tokens(text);
    assert_eq!(result.chinese_char_count, 4);
    assert_eq!(result.english_char_count, 0);
    // Each Chinese char = 1/1.5 tokens, so 4 chars → ceil(4/1.5) = 3 tokens + overhead
    assert!(result.estimated_tokens >= 3);
}

#[test]
fn token_estimator_mixed_text() {
    let est = TokenEstimator::new();
    let text = "Hello 世界 World";
    let result = est.estimate_tokens(text);
    assert_eq!(result.chinese_char_count, 2);
    assert!(result.english_char_count > 0);
    assert!(result.estimated_tokens > result.chinese_char_count);
}

#[test]
fn token_estimator_longer_text_more_tokens() {
    let est = TokenEstimator::new();
    let short = est.estimate_tokens("short");
    let long = est.estimate_tokens("This is a much longer piece of English text with many more words and characters spread across the input.");
    assert!(
        long.estimated_tokens > short.estimated_tokens,
        "longer text should produce more tokens"
    );
}

#[test]
fn token_calculation_rules_default_ratios() {
    let rules = TokenCalculationRules::default();
    assert!(rules.english_char_per_token > 0.0);
    assert!(rules.chinese_char_per_token > 0.0);
    assert!(rules.base_token_overhead > 0);
    // Standard GPT heuristics: ~4 English chars/token, ~1.5 Chinese chars/token
    assert!(
        (rules.english_char_per_token - 4.0).abs() < 0.1,
        "english ratio should be ~4"
    );
    assert!(
        (rules.chinese_char_per_token - 1.5).abs() < 0.1,
        "chinese ratio should be ~1.5"
    );
}

// ---------------------------------------------------------------------------
// CompressionConfig
// ---------------------------------------------------------------------------

#[test]
fn compression_config_default_enabled() {
    let cfg = CompressionConfig::default();
    assert!(cfg.enabled);
}

#[test]
fn compression_config_default_threshold_positive() {
    let cfg = CompressionConfig::default();
    assert!(cfg.compression_threshold > 0);
}

#[test]
fn compression_config_default_ratio_between_zero_and_one() {
    let cfg = CompressionConfig::default();
    assert!(cfg.target_compression_ratio > 0.0);
    assert!(cfg.target_compression_ratio <= 1.0);
}

#[test]
fn compression_config_default_preserve_patterns_non_empty() {
    let cfg = CompressionConfig::default();
    assert!(
        !cfg.preserve_patterns.is_empty(),
        "default should preserve at least one pattern"
    );
}

#[test]
fn preserve_pattern_all_variants_representable() {
    // Ensure all enum variants can be constructed without panic
    let _patterns = [
        PreservePattern::FunctionSignatures,
        PreservePattern::TypeDefinitions,
        PreservePattern::ImportStatements,
        PreservePattern::InterfaceDefinitions,
        PreservePattern::ErrorHandling,
        PreservePattern::Configuration,
    ];
}

#[test]
fn compression_config_serde_round_trip() {
    let cfg = CompressionConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: CompressionConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.enabled, cfg.enabled);
    assert_eq!(parsed.compression_threshold, cfg.compression_threshold);
    assert!((parsed.target_compression_ratio - cfg.target_compression_ratio).abs() < f64::EPSILON);
}

// ---------------------------------------------------------------------------
// ProviderClient::new (Ollama — no API key required)
// ---------------------------------------------------------------------------

#[test]
fn provider_client_new_ollama_succeeds() {
    let llm_cfg = common::test_llm_config();
    assert_eq!(llm_cfg.provider, LLMProvider::Ollama);
    // Construction should never fail for Ollama (no API key validation)
    let result = ProviderClient::new(&llm_cfg);
    assert!(
        result.is_ok(),
        "ProviderClient::new should succeed for Ollama: {:?}",
        result.err()
    );
}

#[test]
fn provider_client_new_openai_with_key_succeeds() {
    let mut cfg = common::test_llm_config();
    cfg.provider = LLMProvider::OpenAI;
    cfg.api_key = "test-key".to_string();
    cfg.api_base_url = "http://localhost:11434/v1".to_string();
    // OpenAI client construction is infallible (no network contact at build time)
    let result = ProviderClient::new(&cfg);
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// CodexRsClient construction
// ---------------------------------------------------------------------------

#[test]
fn codex_client_with_explicit_path_succeeds() {
    // Explicit path — existence check deferred to actual invocation
    let result = CodexRsClient::new(Some("/fake/path/codex"), Some(30), None, None);
    assert!(result.is_ok(), "explicit path should succeed at build time");
    let client = result.unwrap();
    assert_eq!(client.binary_path, "/fake/path/codex");
    assert_eq!(client.timeout_secs, 30);
}

#[test]
fn codex_client_default_timeout_applied_when_none() {
    let client = CodexRsClient::new(Some("/fake/codex"), None, None, None).unwrap();
    // Default timeout is 120 seconds per DEFAULT_TIMEOUT_SECS constant
    assert!(client.timeout_secs > 0, "timeout should be > 0");
    assert!(
        client.timeout_secs >= 60,
        "default timeout should be at least 60s"
    );
}

// ---------------------------------------------------------------------------
// DataSource / file utilities (from utils::sources — indirect via Config)
// ---------------------------------------------------------------------------

#[test]
fn test_config_from_common_helper_has_valid_llm() {
    let cfg = common::test_config();
    assert_eq!(cfg.llm.provider, LLMProvider::Ollama);
    assert!(!cfg.llm.api_base_url.is_empty());
    assert!(!cfg.llm.model_efficient.is_empty());
    assert!(!cfg.llm.model_powerful.is_empty());
}

#[test]
fn test_config_from_common_helper_cache_disabled() {
    let cfg = common::test_config();
    // Test config should disable the cache for isolation
    assert!(!cfg.cache.enabled, "test cache should be disabled");
}

#[test]
fn test_config_from_common_helper_low_depth() {
    let cfg = common::test_config();
    // Limit recursion depth in test configuration
    assert!(cfg.max_depth <= 5, "test config depth should be low");
}
