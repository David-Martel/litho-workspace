//! Shared test helpers for litho-generator integration tests.
//!
//! All helpers are pure in-memory construction — no running Ollama instance
//! or network connection is required.

use litho_generator::config::{
    CacheConfig, Config, LLMConfig, LLMProvider, PreprocessingConfig, QualityConfig, ReviewConfig,
};
use std::path::PathBuf;

/// Return a minimal [`LLMConfig`] suitable for unit tests.
///
/// Points at `http://localhost:11434` (Ollama) so no real API key is needed,
/// but tests must NOT actually call out to the network.
pub fn test_llm_config() -> LLMConfig {
    LLMConfig {
        provider: LLMProvider::Ollama,
        api_key: String::new(),
        api_base_url: "http://localhost:11434".to_string(),
        model_efficient: "qwen2.5-coder:7b".to_string(),
        model_powerful: "qwen2.5-coder:7b".to_string(),
        max_tokens: 4096,
        temperature: Some(0.1),
        retry_attempts: 1,
        retry_delay_ms: 0,
        timeout_seconds: 10,
        disable_preset_tools: false,
        max_parallels: 1,
        codex_binary_path: None,
        codex_as_fallback: false,
        context_window: 32768,
        ollama_auto_detect_models: false,
        ollama_required_models: vec![],
        ollama_auto_pull_missing_models: false,
        ollama_warm_models_on_start: false,
    }
}

/// Return a minimal [`Config`] suitable for integration tests.
///
/// Uses a temporary project path so file-system operations resolve safely.
pub fn test_config() -> Config {
    Config {
        project_name: Some("test-project".to_string()),
        project_path: PathBuf::from("."),
        output_path: PathBuf::from("./test-output"),
        internal_path: PathBuf::from("./.litho-test"),
        target_language: Default::default(),
        analyze_dependencies: false,
        identify_components: false,
        max_depth: 3,
        core_component_percentage: 20.0,
        max_file_size: 64 * 1024,
        include_tests: false,
        include_hidden: false,
        excluded_dirs: vec!["target".to_string(), ".git".to_string()],
        excluded_files: vec![],
        excluded_extensions: vec![],
        included_extensions: vec![],
        architecture_meta_path: None,
        llm: test_llm_config(),
        cache: CacheConfig {
            enabled: false,
            cache_dir: PathBuf::from(".litho-test/cache"),
            expire_hours: 1,
        },
        knowledge: Default::default(),
        qmd: Default::default(),
        output_format: "md".to_string(),
        quality: QualityConfig::default(),
        preprocessing: PreprocessingConfig::default(),
        review: ReviewConfig::default(),
    }
}
