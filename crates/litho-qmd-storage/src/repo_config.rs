use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

pub const REPO_QMD_CONFIG_CANDIDATES: &[&str] = &["qmd.config.json", "config/qmd.config.json"];
pub const REPO_DOTENV_CANDIDATES: &[&str] = &[".env", "config/.env"];
pub const REPO_LITHO_TOML_CANDIDATES: &[&str] = &["litho.toml", ".litho/litho.toml"];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoQmdConfig {
    #[serde(default)]
    pub database: RepoDatabaseConfig,
    #[serde(default)]
    pub paths: RepoPathsConfig,
    #[serde(default)]
    pub llm: RepoLlmConfig,
}

impl RepoQmdConfig {
    fn fill_missing_from(&mut self, fallback: Self) {
        self.database.fill_missing_from(fallback.database);
        self.paths.fill_missing_from(fallback.paths);
        self.llm.fill_missing_from(fallback.llm);
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoPathsConfig {
    #[serde(default)]
    pub collections_config_path: Option<String>,
    #[serde(default)]
    pub search_telemetry_path: Option<String>,
}

impl RepoPathsConfig {
    fn fill_missing_from(&mut self, fallback: Self) {
        if self.collections_config_path.is_none() {
            self.collections_config_path = fallback.collections_config_path;
        }
        if self.search_telemetry_path.is_none() {
            self.search_telemetry_path = fallback.search_telemetry_path;
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoDatabaseConfig {
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sqlite_path: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sslmode: Option<String>,
    #[serde(default)]
    pub admin_db: Option<String>,
    #[serde(default)]
    pub bootstrap: Option<bool>,
    #[serde(default)]
    pub pool_max: Option<u32>,
    #[serde(default)]
    pub pool_min_idle: Option<u32>,
    #[serde(default)]
    pub pool_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub pool_idle_timeout_ms: Option<u64>,
    #[serde(default)]
    pub pool_max_lifetime_ms: Option<u64>,
    #[serde(default)]
    pub pool_test_on_check_out: Option<bool>,
    #[serde(default)]
    pub db_connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub allow_tls: Option<bool>,
    #[serde(default)]
    pub allow_insecure_tls: Option<bool>,
}

impl RepoDatabaseConfig {
    fn fill_missing_from(&mut self, fallback: Self) {
        if self.backend.is_none() {
            self.backend = fallback.backend;
        }
        if self.url.is_none() {
            self.url = fallback.url;
        }
        if self.sqlite_path.is_none() {
            self.sqlite_path = fallback.sqlite_path;
        }
        if self.host.is_none() {
            self.host = fallback.host;
        }
        if self.port.is_none() {
            self.port = fallback.port;
        }
        if self.user.is_none() {
            self.user = fallback.user;
        }
        if self.password.is_none() {
            self.password = fallback.password;
        }
        if self.name.is_none() {
            self.name = fallback.name;
        }
        if self.sslmode.is_none() {
            self.sslmode = fallback.sslmode;
        }
        if self.admin_db.is_none() {
            self.admin_db = fallback.admin_db;
        }
        if self.bootstrap.is_none() {
            self.bootstrap = fallback.bootstrap;
        }
        if self.pool_max.is_none() {
            self.pool_max = fallback.pool_max;
        }
        if self.pool_min_idle.is_none() {
            self.pool_min_idle = fallback.pool_min_idle;
        }
        if self.pool_connect_timeout_ms.is_none() {
            self.pool_connect_timeout_ms = fallback.pool_connect_timeout_ms;
        }
        if self.pool_idle_timeout_ms.is_none() {
            self.pool_idle_timeout_ms = fallback.pool_idle_timeout_ms;
        }
        if self.pool_max_lifetime_ms.is_none() {
            self.pool_max_lifetime_ms = fallback.pool_max_lifetime_ms;
        }
        if self.pool_test_on_check_out.is_none() {
            self.pool_test_on_check_out = fallback.pool_test_on_check_out;
        }
        if self.db_connect_timeout_ms.is_none() {
            self.db_connect_timeout_ms = fallback.db_connect_timeout_ms;
        }
        if self.allow_tls.is_none() {
            self.allow_tls = fallback.allow_tls;
        }
        if self.allow_insecure_tls.is_none() {
            self.allow_insecure_tls = fallback.allow_insecure_tls;
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoLlmConfig {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub ollama_query_model: Option<String>,
    #[serde(default)]
    pub ollama_rerank_model: Option<String>,
    #[serde(default)]
    pub ollama_required_models: Vec<String>,
    #[serde(default)]
    pub model_dirs: Vec<String>,
    #[serde(default)]
    pub ollama_complexity_threshold: Option<usize>,
    #[serde(default)]
    pub force_ollama: Option<bool>,
}

impl RepoLlmConfig {
    fn fill_missing_from(&mut self, fallback: Self) {
        if self.provider.is_none() {
            self.provider = fallback.provider;
        }
        if self.ollama_query_model.is_none() {
            self.ollama_query_model = fallback.ollama_query_model;
        }
        if self.ollama_rerank_model.is_none() {
            self.ollama_rerank_model = fallback.ollama_rerank_model;
        }
        if self.ollama_required_models.is_empty() {
            self.ollama_required_models = fallback.ollama_required_models;
        }
        if self.model_dirs.is_empty() {
            self.model_dirs = fallback.model_dirs;
        }
        if self.ollama_complexity_threshold.is_none() {
            self.ollama_complexity_threshold = fallback.ollama_complexity_threshold;
        }
        if self.force_ollama.is_none() {
            self.force_ollama = fallback.force_ollama;
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LithoTomlConfig {
    #[serde(default)]
    qmd: RepoQmdConfig,
}

pub fn load_repo_qmd_config() -> RepoQmdConfig {
    let mut config = load_repo_toml_qmd_config();
    let legacy = load_repo_json_config(REPO_QMD_CONFIG_CANDIDATES);
    config.fill_missing_from(legacy);
    config
}

pub fn load_repo_json_config<T>(candidates: &[&str]) -> T
where
    T: DeserializeOwned + Default,
{
    let config_path = candidates
        .iter()
        .find_map(|relative| discover_repo_file(relative));
    let Some(path) = config_path else {
        return T::default();
    };

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return T::default(),
    };
    serde_json::from_str::<T>(&raw).unwrap_or_default()
}

pub fn load_repo_dotenv_values() -> BTreeMap<String, String> {
    let env_path = REPO_DOTENV_CANDIDATES
        .iter()
        .find_map(|relative| discover_repo_file(relative));
    let Some(path) = env_path else {
        return BTreeMap::new();
    };

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return BTreeMap::new(),
    };

    let mut out = BTreeMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let mut value = value.trim().to_string();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = value[1..value.len() - 1].to_string();
        }
        out.insert(key, value);
    }
    out
}

pub fn get_dotenv_or_env(dotenv: &BTreeMap<String, String>, key: &str) -> Option<String> {
    dotenv.get(key).cloned().or_else(|| env::var(key).ok())
}

pub fn discover_repo_file(relative: &str) -> Option<PathBuf> {
    let mut cursor = env::current_dir().ok()?;
    loop {
        let candidate = cursor.join(relative);
        if candidate.exists() {
            return Some(candidate);
        }
        if !cursor.pop() {
            break;
        }
    }

    let known = PathBuf::from("C:/codedev/litho-workspace").join(relative);
    if known.exists() {
        return Some(known);
    }
    None
}

fn load_repo_toml_qmd_config() -> RepoQmdConfig {
    let config_path = REPO_LITHO_TOML_CANDIDATES
        .iter()
        .find_map(|relative| discover_repo_file(relative));
    let Some(path) = config_path else {
        return RepoQmdConfig::default();
    };

    let raw = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return RepoQmdConfig::default(),
    };

    parse_litho_toml_qmd_config(&raw)
}

fn parse_litho_toml_qmd_config(raw: &str) -> RepoQmdConfig {
    toml::from_str::<LithoTomlConfig>(raw)
        .map(|parsed| parsed.qmd)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        RepoQmdConfig, get_dotenv_or_env, load_repo_json_config, parse_litho_toml_qmd_config,
    };
    use serde::Deserialize;
    use std::collections::BTreeMap;

    #[derive(Debug, Default, Deserialize, PartialEq, Eq)]
    struct TestConfig {
        name: String,
    }

    #[test]
    fn load_repo_json_config_defaults_when_no_candidate_exists() {
        let config =
            load_repo_json_config::<TestConfig>(&["missing/file/that/does/not/exist.json"]);
        assert_eq!(config, TestConfig::default());
    }

    #[test]
    #[serial_test::serial]
    fn get_dotenv_or_env_prefers_dotenv_values() {
        let mut dotenv = BTreeMap::new();
        dotenv.insert("QMD_TEST_KEY".to_string(), "from-dotenv".to_string());
        unsafe { std::env::set_var("QMD_TEST_KEY", "from-env") };

        let value = get_dotenv_or_env(&dotenv, "QMD_TEST_KEY");

        unsafe { std::env::remove_var("QMD_TEST_KEY") };
        assert_eq!(value.as_deref(), Some("from-dotenv"));
    }

    #[test]
    fn parse_litho_toml_qmd_config_reads_qmd_section() {
        let raw = r#"
[qmd.database]
backend = "sqlite"
sqlite_path = ".litho/qmd/custom.sqlite3"

[qmd.paths]
collections_config_path = ".litho/qmd/collections.yaml"

[qmd.llm]
provider = "ollama"
ollama_query_model = "qwen2.5-coder:7b"
"#;

        let cfg = parse_litho_toml_qmd_config(raw);
        assert_eq!(cfg.database.backend.as_deref(), Some("sqlite"));
        assert_eq!(
            cfg.database.sqlite_path.as_deref(),
            Some(".litho/qmd/custom.sqlite3")
        );
        assert_eq!(
            cfg.paths.collections_config_path.as_deref(),
            Some(".litho/qmd/collections.yaml")
        );
        assert_eq!(cfg.llm.provider.as_deref(), Some("ollama"));
        assert_eq!(
            cfg.llm.ollama_query_model.as_deref(),
            Some("qwen2.5-coder:7b")
        );
    }

    #[test]
    fn qmd_config_prefers_litho_toml_values_over_legacy_fallback() {
        let mut from_toml = parse_litho_toml_qmd_config(
            r#"
[qmd.database]
backend = "sqlite"

[qmd.llm]
provider = "ollama"
"#,
        );
        let legacy = RepoQmdConfig {
            database: super::RepoDatabaseConfig {
                backend: Some("postgres".to_string()),
                url: Some("postgres://example".to_string()),
                ..Default::default()
            },
            llm: super::RepoLlmConfig {
                provider: Some("legacy-provider".to_string()),
                ollama_query_model: Some("legacy-model".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        from_toml.fill_missing_from(legacy);

        assert_eq!(from_toml.database.backend.as_deref(), Some("sqlite"));
        assert_eq!(
            from_toml.database.url.as_deref(),
            Some("postgres://example")
        );
        assert_eq!(from_toml.llm.provider.as_deref(), Some("ollama"));
        assert_eq!(
            from_toml.llm.ollama_query_model.as_deref(),
            Some("legacy-model")
        );
    }
}
