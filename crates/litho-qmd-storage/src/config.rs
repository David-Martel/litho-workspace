use crate::{QmdError, QmdResult, load_repo_qmd_config as repo_qmd_config};
#[cfg(feature = "postgres")]
use crate::{get_dotenv_or_env, load_repo_dotenv_values as repo_dotenv_values};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[cfg(feature = "postgres")]
#[derive(Debug, Clone)]
pub(crate) struct RuntimeDbSettings {
    pub(crate) database_url: String,
    pub(crate) admin_db: String,
    pub(crate) bootstrap: bool,
    pub(crate) pool_max: u32,
    pub(crate) pool_min_idle: u32,
    pub(crate) pool_connect_timeout_ms: u64,
    pub(crate) pool_idle_timeout_ms: u64,
    pub(crate) pool_max_lifetime_ms: u64,
    pub(crate) pool_test_on_check_out: bool,
    pub(crate) db_connect_timeout_ms: u64,
    pub(crate) allow_tls: bool,
    pub(crate) allow_insecure_tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ConfigFile {
    #[serde(default)]
    pub(crate) collections: BTreeMap<String, CollectionConfig>,
    #[serde(default)]
    pub(crate) global_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CollectionConfig {
    pub(crate) path: String,
    #[serde(default = "default_pattern")]
    pub(crate) pattern: String,
    #[serde(default)]
    pub(crate) context: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) update: Option<String>,
}

impl Default for CollectionConfig {
    fn default() -> Self {
        Self {
            path: String::new(),
            pattern: default_pattern(),
            context: BTreeMap::new(),
            update: None,
        }
    }
}

pub(crate) fn resolve_collection_config_path(index_name: &str) -> QmdResult<PathBuf> {
    if let Some(path) = repo_qmd_config().paths.collections_config_path
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    default_config_path(index_name)
}

#[cfg(feature = "postgres")]
pub(crate) fn resolve_runtime_db_settings(index_name: &str) -> RuntimeDbSettings {
    let fallback_url = build_default_database_url(index_name);
    default_runtime_db_settings(fallback_url)
}

pub(crate) fn sanitize_index_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn default_config_path(index_name: &str) -> QmdResult<PathBuf> {
    if let Ok(dir) = std::env::var("QMD_CONFIG_DIR") {
        let mut path = PathBuf::from(dir);
        path.push(format!("{index_name}.yml"));
        return Ok(path);
    }

    let mut base = dirs::config_dir().ok_or_else(|| {
        QmdError::Internal("unable to determine user config directory".to_string())
    })?;
    base.push("qmd");
    base.push(format!("{index_name}.yml"));
    Ok(base)
}

#[cfg(feature = "postgres")]
pub(crate) fn default_runtime_db_settings(fallback_url: String) -> RuntimeDbSettings {
    let cfg = repo_qmd_config();
    let dot = repo_dotenv_values();
    let db = cfg.database;
    let sslmode = db
        .sslmode
        .clone()
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_SSLMODE"))
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_SSLMODE"))
        .unwrap_or_else(|| "disable".to_string());

    let database_url = db
        .url
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DATABASE_URL"))
        .or_else(|| get_dotenv_or_env(&dot, "DATABASE_URL"))
        .unwrap_or_else(|| {
            let host = db
                .host
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_HOST"))
                .or_else(|| get_dotenv_or_env(&dot, "PGHOST"))
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = db
                .port
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PORT").and_then(|v| v.parse().ok()))
                .or_else(|| get_dotenv_or_env(&dot, "PGPORT").and_then(|v| v.parse().ok()))
                .unwrap_or(5432);
            let user = db
                .user
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_USER"))
                .or_else(|| get_dotenv_or_env(&dot, "PGUSER"))
                .unwrap_or_else(|| "postgres".to_string());
            let pass = db
                .password
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PASSWORD"))
                .or_else(|| get_dotenv_or_env(&dot, "PGPASSWORD"))
                .unwrap_or_else(|| "password".to_string());
            let name = db
                .name
                .clone()
                .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_NAME"))
                .or_else(|| get_dotenv_or_env(&dot, "PGDATABASE"))
                .unwrap_or_else(|| "qmd_index".to_string());
            format!("postgresql://{user}:{pass}@{host}:{port}/{name}?sslmode={sslmode}")
        });

    let pool_max = db
        .pool_max
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_POOL_MAX").and_then(|v| v.parse().ok()))
        .unwrap_or(24)
        .max(4);
    let pool_min_idle = db
        .pool_min_idle
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_POOL_MIN_IDLE").and_then(|v| v.parse().ok()))
        .unwrap_or(4)
        .min(pool_max);
    let pool_connect_timeout_ms = db
        .pool_connect_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_CONNECT_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(8_000);
    let pool_idle_timeout_ms = db
        .pool_idle_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_IDLE_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(600_000);
    let pool_max_lifetime_ms = db
        .pool_max_lifetime_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_POOL_MAX_LIFETIME_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(3_600_000);
    let pool_test_on_check_out = db
        .pool_test_on_check_out
        .or_else(|| {
            parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_POOL_TEST_ON_CHECKOUT").as_deref())
        })
        .unwrap_or(true);
    let db_connect_timeout_ms = db
        .db_connect_timeout_ms
        .or_else(|| {
            get_dotenv_or_env(&dot, "QMD_PG_DB_CONNECT_TIMEOUT_MS").and_then(|v| v.parse().ok())
        })
        .unwrap_or(5_000);

    let allow_tls = db
        .allow_tls
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_ALLOW_TLS").as_deref()))
        .unwrap_or(true);
    let allow_insecure_tls = db
        .allow_insecure_tls
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_TLS_INSECURE").as_deref()))
        .unwrap_or(false);
    let bootstrap = db
        .bootstrap
        .or_else(|| parse_bool_opt(get_dotenv_or_env(&dot, "QMD_PG_BOOTSTRAP").as_deref()))
        .unwrap_or(true);
    let admin_db = db
        .admin_db
        .or_else(|| get_dotenv_or_env(&dot, "QMD_PG_ADMIN_DB"))
        .unwrap_or_else(|| "postgres".to_string());

    RuntimeDbSettings {
        database_url: if database_url.is_empty() {
            fallback_url
        } else {
            database_url
        },
        admin_db,
        bootstrap,
        pool_max,
        pool_min_idle,
        pool_connect_timeout_ms,
        pool_idle_timeout_ms,
        pool_max_lifetime_ms,
        pool_test_on_check_out,
        db_connect_timeout_ms,
        allow_tls,
        allow_insecure_tls,
    }
}

#[cfg(feature = "postgres")]
fn build_default_database_url(index_name: &str) -> String {
    let host = "127.0.0.1";
    let port = 5432;
    let user = "postgres";
    let pass = "password";
    let db = format!("qmd_{}", sanitize_index_name(index_name));
    format!("postgresql://{user}:{pass}@{host}:{port}/{db}")
}

#[cfg(feature = "postgres")]
pub(crate) fn runtime_password() -> Option<String> {
    let dot = repo_dotenv_values();
    let db = repo_qmd_config().database;
    db.password
        .or_else(|| get_dotenv_or_env(&dot, "QMD_DB_PASSWORD"))
        .or_else(|| get_dotenv_or_env(&dot, "PGPASSWORD"))
}

fn default_pattern() -> String {
    "**/*.md".to_string()
}

#[cfg(feature = "postgres")]
fn parse_bool_opt(input: Option<&str>) -> Option<bool> {
    match input?.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
