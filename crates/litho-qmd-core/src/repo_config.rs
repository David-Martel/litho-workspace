use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

pub const REPO_QMD_CONFIG_CANDIDATES: &[&str] = &["qmd.config.json", "config/qmd.config.json"];
pub const REPO_DOTENV_CANDIDATES: &[&str] = &[".env", "config/.env"];

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoQmdConfig {
    #[serde(default)]
    pub database: RepoDatabaseConfig,
    #[serde(default)]
    pub paths: RepoPathsConfig,
    #[serde(default)]
    pub llm: RepoLlmConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoPathsConfig {
    #[serde(default)]
    pub collections_config_path: Option<String>,
    #[serde(default)]
    pub search_telemetry_path: Option<String>,
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

pub fn load_repo_qmd_config() -> RepoQmdConfig {
    load_repo_json_config(REPO_QMD_CONFIG_CANDIDATES)
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
