//! Integration tests for configuration loading and validation.

mod common;

use litho_generator::config::{
    CacheConfig, ChunkingConfig, Config, DocumentCategory, KnowledgeConfig, LLMConfig, LLMProvider,
    LocalDocsConfig, QmdRetrieverConfig,
};
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

// `toml` is a [dependencies] crate — available to integration tests automatically.

// ---------------------------------------------------------------------------
// LLMProvider::from_str
// ---------------------------------------------------------------------------

#[test]
fn llm_provider_from_str_openai() {
    assert_eq!(
        LLMProvider::from_str("openai").unwrap(),
        LLMProvider::OpenAI
    );
    assert_eq!(
        LLMProvider::from_str("OpenAI").unwrap(),
        LLMProvider::OpenAI
    );
}

#[test]
fn llm_provider_from_str_ollama() {
    assert_eq!(
        LLMProvider::from_str("ollama").unwrap(),
        LLMProvider::Ollama
    );
    assert_eq!(
        LLMProvider::from_str("OLLAMA").unwrap(),
        LLMProvider::Ollama
    );
}

#[test]
fn llm_provider_from_str_all_variants() {
    let cases = [
        ("openai", LLMProvider::OpenAI),
        ("moonshot", LLMProvider::Moonshot),
        ("deepseek", LLMProvider::DeepSeek),
        ("mistral", LLMProvider::Mistral),
        ("openrouter", LLMProvider::OpenRouter),
        ("anthropic", LLMProvider::Anthropic),
        ("gemini", LLMProvider::Gemini),
        ("ollama", LLMProvider::Ollama),
        ("codexrs", LLMProvider::CodexRs),
        ("codex-rs", LLMProvider::CodexRs),
        ("codex", LLMProvider::CodexRs),
    ];
    for (input, expected) in cases {
        assert_eq!(
            LLMProvider::from_str(input).unwrap(),
            expected,
            "failed for input '{}'",
            input
        );
    }
}

#[test]
fn llm_provider_from_str_unknown_returns_err() {
    let err = LLMProvider::from_str("does-not-exist").unwrap_err();
    assert!(
        err.contains("Unknown provider"),
        "unexpected error: {}",
        err
    );
}

#[test]
fn llm_provider_display_round_trip() {
    let variants = [
        LLMProvider::OpenAI,
        LLMProvider::Moonshot,
        LLMProvider::DeepSeek,
        LLMProvider::Mistral,
        LLMProvider::OpenRouter,
        LLMProvider::Anthropic,
        LLMProvider::Gemini,
        LLMProvider::Ollama,
        LLMProvider::CodexRs,
    ];
    for variant in variants {
        let s = variant.to_string();
        let parsed = LLMProvider::from_str(&s).unwrap();
        assert_eq!(parsed, variant, "round-trip failed for {:?}", variant);
    }
}

// ---------------------------------------------------------------------------
// LLMProvider serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn llm_provider_serde_round_trip() {
    let variants = [
        LLMProvider::OpenAI,
        LLMProvider::Moonshot,
        LLMProvider::DeepSeek,
        LLMProvider::Mistral,
        LLMProvider::OpenRouter,
        LLMProvider::Anthropic,
        LLMProvider::Gemini,
        LLMProvider::Ollama,
        LLMProvider::CodexRs,
    ];
    for variant in variants {
        let json = serde_json::to_string(&variant).unwrap();
        let parsed: LLMProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, variant, "serde round-trip failed for {:?}", variant);
    }
}

#[test]
fn llm_provider_default_is_openai() {
    assert_eq!(LLMProvider::default(), LLMProvider::OpenAI);
}

// ---------------------------------------------------------------------------
// Config::default()
// ---------------------------------------------------------------------------

#[test]
fn config_default_has_valid_paths() {
    let cfg = Config::default();
    assert_eq!(cfg.project_path, PathBuf::from("."));
    assert_eq!(cfg.output_path, PathBuf::from("./litho.docs"));
    assert_eq!(cfg.internal_path, PathBuf::from("./.litho"));
}

#[test]
fn config_default_excluded_dirs_non_empty() {
    let cfg = Config::default();
    assert!(
        !cfg.excluded_dirs.is_empty(),
        "excluded_dirs should not be empty"
    );
    assert!(cfg.excluded_dirs.contains(&"target".to_string()));
    assert!(cfg.excluded_dirs.contains(&".git".to_string()));
    assert!(cfg.excluded_dirs.contains(&"node_modules".to_string()));
}

#[test]
fn config_default_max_depth_reasonable() {
    let cfg = Config::default();
    assert!(cfg.max_depth >= 5, "max_depth should be at least 5");
}

#[test]
fn config_default_cache_enabled() {
    let cfg = Config::default();
    assert!(cfg.cache.enabled, "cache should be enabled by default");
}

#[test]
fn config_default_include_tests_false() {
    let cfg = Config::default();
    assert!(!cfg.include_tests, "include_tests should default to false");
}

#[test]
fn config_default_include_hidden_false() {
    let cfg = Config::default();
    assert!(
        !cfg.include_hidden,
        "include_hidden should default to false"
    );
}

#[test]
fn config_default_no_project_name() {
    let cfg = Config::default();
    assert!(
        cfg.project_name.is_none(),
        "project_name should default to None"
    );
}

// ---------------------------------------------------------------------------
// CacheConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn cache_config_default_enabled() {
    let cache = CacheConfig::default();
    assert!(cache.enabled);
}

#[test]
fn cache_config_default_expire_hours_non_zero() {
    let cache = CacheConfig::default();
    assert!(cache.expire_hours > 0);
}

#[test]
fn cache_config_default_cache_dir() {
    let cache = CacheConfig::default();
    assert_eq!(cache.cache_dir, PathBuf::from(".litho/cache"));
}

// ---------------------------------------------------------------------------
// LLMConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn llm_config_default_max_tokens_non_zero() {
    let llm = LLMConfig::default();
    assert!(llm.max_tokens > 0, "max_tokens must be > 0");
}

#[test]
fn llm_config_default_temperature_some() {
    let llm = LLMConfig::default();
    // temperature may be None for reasoning models; default uses Some(0.1)
    if let Some(t) = llm.temperature {
        assert!(t >= 0.0 && t <= 2.0, "temperature out of expected range");
    }
}

#[test]
fn llm_config_default_retry_attempts_non_zero() {
    let llm = LLMConfig::default();
    assert!(llm.retry_attempts > 0);
}

#[test]
fn llm_config_default_timeout_non_zero() {
    let llm = LLMConfig::default();
    assert!(llm.timeout_seconds > 0);
}

#[test]
fn llm_config_default_max_parallels_at_least_one() {
    let llm = LLMConfig::default();
    assert!(llm.max_parallels >= 1);
}

// ---------------------------------------------------------------------------
// QmdRetrieverConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn qmd_config_default_disabled() {
    let qmd = QmdRetrieverConfig::default();
    assert!(!qmd.enabled, "qmd retriever should be disabled by default");
}

#[test]
fn qmd_config_default_bin_is_qmd() {
    let qmd = QmdRetrieverConfig::default();
    assert_eq!(qmd.bin, "qmd");
}

#[test]
fn qmd_config_default_mode_is_query() {
    let qmd = QmdRetrieverConfig::default();
    assert_eq!(qmd.mode, "query");
}

#[test]
fn qmd_config_default_limit_positive() {
    let qmd = QmdRetrieverConfig::default();
    assert!(qmd.limit > 0, "default limit should be positive");
}

#[test]
fn qmd_config_default_store_key_non_empty() {
    let qmd = QmdRetrieverConfig::default();
    assert!(!qmd.store_key.is_empty());
}

#[test]
fn qmd_config_default_queries_empty() {
    let qmd = QmdRetrieverConfig::default();
    assert!(qmd.queries.is_empty());
}

// ---------------------------------------------------------------------------
// TOML parsing — write temp files to validate round-trips
// ---------------------------------------------------------------------------

fn write_temp_toml(content: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("litho.toml");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    (dir, path)
}

/// Serialize a `Config` to TOML and write to a temp file.
/// This guarantees all required fields are present without hand-coding them.
fn config_to_temp_toml(cfg: &Config) -> (tempfile::TempDir, PathBuf) {
    let toml_str = toml::to_string(cfg).expect("Config should be serializable");
    write_temp_toml(&toml_str)
}

#[test]
fn config_from_file_minimal_ollama() {
    // Build a default config, override just the fields we care about, then
    // round-trip through TOML — avoids hand-coding every required field.
    let mut cfg = Config::default();
    cfg.llm.provider = LLMProvider::Ollama;
    cfg.llm.api_base_url = "http://localhost:11434".to_string();
    cfg.llm.model_efficient = "qwen2.5-coder:7b".to_string();
    cfg.llm.model_powerful = "qwen2.5-coder:7b".to_string();
    cfg.cache.enabled = false;

    let (_dir, path) = config_to_temp_toml(&cfg);
    let loaded = Config::from_file(&path).expect("serialized Config should round-trip");
    assert_eq!(loaded.llm.provider, LLMProvider::Ollama);
    assert_eq!(loaded.llm.model_efficient, "qwen2.5-coder:7b");
    assert!(!loaded.cache.enabled);
}

#[test]
fn config_from_file_with_project_name() {
    let mut cfg = Config::default();
    cfg.project_name = Some("my-test-project".to_string());
    cfg.llm.provider = LLMProvider::Ollama;

    let (_dir, path) = config_to_temp_toml(&cfg);
    let loaded = Config::from_file(&path).unwrap();
    assert_eq!(loaded.get_project_name(), "my-test-project");
}

#[test]
fn config_from_file_nonexistent_path_returns_err() {
    let missing = PathBuf::from("/no/such/file/litho.toml");
    let result = Config::from_file(&missing);
    assert!(result.is_err(), "expected error for missing file");
}

#[test]
fn config_from_file_invalid_toml_returns_err() {
    let bad_toml = "this is {{{ not valid toml |||";
    let (_dir, path) = write_temp_toml(bad_toml);
    let result = Config::from_file(&path);
    assert!(result.is_err(), "expected error for invalid TOML");
}

#[test]
fn config_from_file_all_providers() {
    // Verify each provider value is accepted by the TOML parser.
    // We round-trip through Config::default() to guarantee all required
    // fields are present without hand-coding every one of them.
    use std::str::FromStr;

    let provider_strs = [
        "openai", "moonshot", "deepseek", "mistral", "openrouter",
        "anthropic", "gemini", "ollama", "codexrs",
    ];
    for p_str in provider_strs {
        let provider = LLMProvider::from_str(p_str).unwrap();
        let mut cfg = Config::default();
        cfg.llm.provider = provider;

        let (_dir, path) = config_to_temp_toml(&cfg);
        let result = Config::from_file(&path);
        assert!(
            result.is_ok(),
            "provider '{}' failed TOML round-trip: {:?}",
            p_str,
            result.err()
        );
        assert_eq!(
            result.unwrap().llm.provider,
            LLMProvider::from_str(p_str).unwrap(),
            "provider mismatch for '{}'",
            p_str
        );
    }
}

// ---------------------------------------------------------------------------
// ChunkingConfig defaults
// ---------------------------------------------------------------------------

#[test]
fn chunking_config_default_enabled() {
    let c = ChunkingConfig::default();
    assert!(c.enabled);
}

#[test]
fn chunking_config_default_max_chunk_size() {
    let c = ChunkingConfig::default();
    assert!(c.max_chunk_size > 0);
}

#[test]
fn chunking_config_default_strategy_non_empty() {
    let c = ChunkingConfig::default();
    assert!(!c.strategy.is_empty());
}

// ---------------------------------------------------------------------------
// KnowledgeConfig / LocalDocsConfig
// ---------------------------------------------------------------------------

#[test]
fn knowledge_config_default_no_local_docs() {
    let k = KnowledgeConfig::default();
    assert!(k.local_docs.is_none());
}

#[test]
fn local_docs_config_disabled_by_default_serde() {
    let cfg: LocalDocsConfig =
        serde_json::from_str(r#"{ "enabled": false, "watch_for_changes": true }"#).unwrap();
    assert!(!cfg.enabled);
    assert!(cfg.categories.is_empty());
}

#[test]
fn document_category_serde_round_trip() {
    let cat = DocumentCategory {
        name: "architecture".to_string(),
        description: "C4 architecture docs".to_string(),
        paths: vec!["docs/arch/*.md".to_string()],
        target_agents: vec!["architect".to_string()],
        chunking: None,
    };
    let json = serde_json::to_string(&cat).unwrap();
    let parsed: DocumentCategory = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "architecture");
    assert_eq!(parsed.paths.len(), 1);
}

// ---------------------------------------------------------------------------
// Config::get_project_name fallback logic
// ---------------------------------------------------------------------------

#[test]
fn get_project_name_uses_explicit_name() {
    let mut cfg = Config::default();
    cfg.project_name = Some("explicit-name".to_string());
    assert_eq!(cfg.get_project_name(), "explicit-name");
}

#[test]
fn get_project_name_ignores_whitespace_only_name() {
    let mut cfg = Config::default();
    cfg.project_name = Some("   ".to_string());
    cfg.project_path = PathBuf::from("/some/path/my-project");
    // Falls back to infer_project_name which uses path's file_name
    let name = cfg.get_project_name();
    assert_eq!(name, "my-project");
}

#[test]
fn get_project_name_uses_path_when_no_name() {
    let mut cfg = Config::default();
    cfg.project_name = None;
    cfg.project_path = PathBuf::from("/home/user/my-rust-app");
    let name = cfg.get_project_name();
    assert_eq!(name, "my-rust-app");
}

// ---------------------------------------------------------------------------
// extract_from_cargo_toml
// ---------------------------------------------------------------------------

#[test]
fn extract_from_cargo_toml_finds_name() {
    let dir = tempfile::TempDir::new().unwrap();
    let cargo_path = dir.path().join("Cargo.toml");
    std::fs::write(
        &cargo_path,
        "[package]\nname = \"my-crate\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();

    let name = cfg.extract_from_cargo_toml().unwrap();
    assert_eq!(name, "my-crate");
}

#[test]
fn extract_from_cargo_toml_returns_none_when_absent() {
    let dir = tempfile::TempDir::new().unwrap();
    // No Cargo.toml in the temp dir
    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();
    assert!(cfg.extract_from_cargo_toml().is_none());
}

// ---------------------------------------------------------------------------
// extract_from_package_json
// ---------------------------------------------------------------------------

#[test]
fn extract_from_package_json_finds_name() {
    let dir = tempfile::TempDir::new().unwrap();
    // The parser reads line-by-line looking for lines starting with `"name"`,
    // so the key must be at the start of a line (standard npm JSON layout).
    std::fs::write(
        dir.path().join("package.json"),
        "{\n  \"name\": \"my-npm-package\",\n  \"version\": \"1.0.0\"\n}\n",
    )
    .unwrap();

    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();
    let name = cfg.extract_from_package_json().unwrap();
    assert_eq!(name, "my-npm-package");
}

#[test]
fn extract_from_package_json_returns_none_when_absent() {
    let dir = tempfile::TempDir::new().unwrap();
    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();
    assert!(cfg.extract_from_package_json().is_none());
}

// ---------------------------------------------------------------------------
// extract_from_pyproject_toml
// ---------------------------------------------------------------------------

#[test]
fn extract_from_pyproject_toml_finds_name_in_project_section() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"my-python-pkg\"\n",
    )
    .unwrap();

    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();
    let name = cfg.extract_from_pyproject_toml().unwrap();
    assert_eq!(name, "my-python-pkg");
}

#[test]
fn extract_from_pyproject_toml_finds_name_in_poetry_section() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("pyproject.toml"),
        "[tool.poetry]\nname = \"my-poetry-pkg\"\n",
    )
    .unwrap();

    let mut cfg = Config::default();
    cfg.project_path = dir.path().to_path_buf();
    let name = cfg.extract_from_pyproject_toml().unwrap();
    assert_eq!(name, "my-poetry-pkg");
}
