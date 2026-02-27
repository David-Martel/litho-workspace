use std::env;
use std::path::PathBuf;
use anyhow::{Context, Result};

pub struct LithoEnv;

impl LithoEnv {
    /// Load environment variables from .env file.
    /// This should be called once at startup.
    pub fn load() {
        if let Err(e) = dotenvy::dotenv() {
            tracing::debug!("No .env file found or error loading it: {}", e);
        }
    }

    /// Get a required environment variable or return an error.
    fn get_required(key: &str) -> Result<String> {
        env::var(key).with_context(|| format!("Environment variable {} is missing", key))
    }

    /// Get an optional environment variable.
    fn get_optional(key: &str) -> Option<String> {
        env::var(key).ok()
    }

    /// Get the path to the codex binary.
    pub fn codex_bin() -> Option<PathBuf> {
        Self::get_optional("CODEX_BIN").map(PathBuf::from)
    }

    /// Get the database URL for PostgreSQL.
    pub fn database_url() -> Result<String> {
        Self::get_required("DATABASE_URL")
    }

    /// Get the Ollama API URL.
    pub fn ollama_url() -> String {
        Self::get_optional("OLLAMA_URL").unwrap_or_else(|| "http://localhost:11434".to_string())
    }

    /// Get the desired model for documentation generation.
    pub fn codex_model() -> Option<String> {
        Self::get_optional("CODEX_MODEL")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optional_var() {
        let key = "NON_EXISTENT_VAR_XYZ";
        assert!(LithoEnv::get_optional(key).is_none());
    }

    #[test]
    fn test_required_var_missing() {
        let key = "MISSING_REQUIRED_VAR";
        let result = LithoEnv::get_required(key);
        assert!(result.is_err());
    }

    #[test]
    fn test_ollama_default() {
        // Ensure it returns default if not set
        unsafe { env::remove_var("OLLAMA_URL"); }
        assert_eq!(LithoEnv::ollama_url(), "http://localhost:11434");
    }

    #[test]
    fn test_type_safe_accessors() {
        unsafe {
            env::set_var("DATABASE_URL", "postgres://test");
            env::set_var("CODEX_BIN", "C:\\bin\\codex.exe");
            env::set_var("CODEX_MODEL", "gpt-4");
        }

        assert_eq!(LithoEnv::database_url().unwrap(), "postgres://test");
        assert_eq!(LithoEnv::codex_bin().unwrap(), PathBuf::from("C:\\bin\\codex.exe"));
        assert_eq!(LithoEnv::codex_model().unwrap(), "gpt-4");

        unsafe {
            env::remove_var("DATABASE_URL");
            env::remove_var("CODEX_BIN");
            env::remove_var("CODEX_MODEL");
        }
    }
}
