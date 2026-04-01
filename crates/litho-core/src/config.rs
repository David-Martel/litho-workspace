use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;

/// Top-level configuration for a litho documentation run.
///
/// Loaded from a TOML file (typically `.litho/litho.toml`) and merged with
/// built-in defaults.  All fields are optional in the file; unspecified keys
/// fall back to the [`Default`] implementation.
///
/// # Example (TOML)
/// ```toml
/// [llm]
/// provider = "ollama"
/// api_base_url = "http://localhost:11434"
/// model_efficient = "qwen2.5-coder:3b"
/// ```
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LithoConfig {
    /// Root of the project being documented.
    pub project_path: PathBuf,
    /// Where generated documentation is written.
    pub output_path: PathBuf,
    /// Internal working directory for litho state.
    pub internal_path: PathBuf,
    /// BCP-47 language tag for generated prose (e.g. `"en"`).
    pub target_language: String,
    /// Maximum directory recursion depth when walking the project tree.
    pub max_depth: u8,
    /// Files larger than this (bytes) are skipped during extraction.
    pub max_file_size: u64,
    /// Directory names that are never descended into.
    pub excluded_dirs: Vec<String>,
    /// File extensions (without leading `.`) that are always skipped.
    pub excluded_extensions: Vec<String>,
    /// Backend selection for AST extraction.
    pub extract_backend: ExtractBackend,
    /// Optional ast-grep (`sg`) binary path override.
    pub ast_grep_binary: Option<String>,
    /// LLM provider settings.
    pub llm: LlmConfig,
    /// Response-cache settings.
    pub cache: CacheConfig,
}

/// Backend used by litho-extract for AST/symbol extraction.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExtractBackend {
    /// Prefer ast-grep when available, otherwise use built-in tree-sitter flow.
    #[default]
    Auto,
    /// Always use built-in tree-sitter extractors.
    TreeSitter,
    /// Prefer ast-grep queries, then gracefully fallback to tree-sitter.
    AstGrep,
}

/// Identifies which LLM backend will handle generation requests.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Codex,
    Ollama,
    OpenAI,
    Anthropic,
    Gemini,
}

/// LLM connection and model parameters.
///
/// `api_base_url` intentionally defaults to an empty string so that no
/// external server is contacted unless the user explicitly configures one.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LlmConfig {
    /// Which backend to target.
    pub provider: LlmProvider,
    /// Bearer token or API key; may also be supplied via the environment.
    pub api_key: String,
    /// Base URL of the inference endpoint.  **Must not default to any external
    /// host** — callers are responsible for populating this field.
    pub api_base_url: String,
    /// Smaller/faster model identifier used for classification tasks.
    pub model_efficient: String,
    /// Larger/slower model identifier used for deep analysis.
    pub model_powerful: String,
    /// Hard cap on tokens the model may emit per request.
    pub max_tokens: u32,
    /// Sampling temperature; `None` lets the provider use its own default.
    pub temperature: Option<f64>,
    /// Per-request timeout in seconds.
    pub timeout_seconds: u64,
    /// When `true`, the built-in tool/function preset is not sent to the LLM.
    pub disable_preset_tools: bool,
}

/// Settings for the on-disk LLM-response cache.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct CacheConfig {
    /// Enable or disable the cache entirely.
    pub enabled: bool,
    /// Directory in which cache entries are stored.
    pub cache_dir: PathBuf,
    /// Number of hours before a cached entry is considered stale.
    pub expire_hours: u64,
}

// ---------------------------------------------------------------------------
// Default implementations
// ---------------------------------------------------------------------------

impl Default for LithoConfig {
    fn default() -> Self {
        Self {
            project_path: PathBuf::from("."),
            output_path: PathBuf::from("./litho.docs"),
            internal_path: PathBuf::from(".litho"),
            target_language: "en".into(),
            max_depth: 10,
            max_file_size: 65536,
            excluded_dirs: [
                ".git",
                "target",
                "node_modules",
                ".litho",
                "__pycache__",
                "dist",
                "build",
                ".next",
                "vendor",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            excluded_extensions: [
                "jpg", "png", "gif", "pdf", "exe", "dll", "so", "dylib", "wasm", "lock", "sum",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            extract_backend: ExtractBackend::default(),
            ast_grep_binary: None,
            llm: LlmConfig::default(),
            cache: CacheConfig::default(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::default(),
            api_key: String::new(),
            // Intentionally empty — no hardcoded external endpoint.
            api_base_url: String::new(),
            model_efficient: String::new(),
            model_powerful: String::new(),
            max_tokens: 8192,
            temperature: Some(0.1),
            timeout_seconds: 300,
            disable_preset_tools: true,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_dir: PathBuf::from(".litho/cache"),
            expire_hours: 720,
        }
    }
}

// ---------------------------------------------------------------------------
// Construction helpers
// ---------------------------------------------------------------------------

impl LithoConfig {
    /// Load configuration from a TOML file, merging with [`Default`] values
    /// for any keys that are absent.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or if the TOML is
    /// malformed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use litho_core::config::LithoConfig;
    /// use std::path::Path;
    ///
    /// let cfg = LithoConfig::from_file(Path::new(".litho/litho.toml")).unwrap();
    /// println!("provider: {:?}", cfg.llm.provider);
    /// ```
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let mut content = String::new();
        std::fs::File::open(path)?.read_to_string(&mut content)?;
        Ok(toml::from_str(&content)?)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_external_urls() {
        let config = LithoConfig::default();
        assert!(
            config.llm.api_base_url.is_empty(),
            "Default must not point to external servers"
        );
    }

    #[test]
    fn config_roundtrip_toml() {
        let config = LithoConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: LithoConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.llm.provider, config.llm.provider);
        assert_eq!(parsed.extract_backend, config.extract_backend);
    }

    #[test]
    fn config_from_file_overrides_defaults() {
        let toml = r#"
extract_backend = "ast_grep"
ast_grep_binary = "/usr/local/bin/sg"

[llm]
provider = "ollama"
api_base_url = "http://localhost:11434"
model_efficient = "qwen2.5-coder:3b"
"#;
        let config: LithoConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.llm.api_base_url, "http://localhost:11434");
        assert_eq!(config.extract_backend, ExtractBackend::AstGrep);
        assert_eq!(config.ast_grep_binary.as_deref(), Some("/usr/local/bin/sg"));
    }

    #[test]
    fn extract_backend_defaults_to_auto() {
        let config = LithoConfig::default();
        assert_eq!(config.extract_backend, ExtractBackend::Auto);
        assert!(config.ast_grep_binary.is_none());
    }
}
