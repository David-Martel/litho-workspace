//! CodexRs fallback provider
//!
//! Invokes the local `codex` binary via subprocess when all Ollama models fail.
//! This is an emergency fallback — not a primary inference path.
//!
//! # Design
//!
//! - Shells out to `codex.exe` (or `codex` on non-Windows) using
//!   [`tokio::process::Command`].
//! - Concatenates system + user prompt into a single `-q` argument.
//! - Parses stdout with the same 4-strategy JSON cascade used by
//!   [`OllamaExtractorWrapper`].
//! - Configurable timeout (default 120 s).

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use uuid::Uuid;

/// JSON code block regex — mirrors the pattern in `ollama_extractor.rs`.
static JSON_CODE_BLOCK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").unwrap());

/// Default subprocess timeout in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Client that delegates to a local `codex` binary.
///
/// # Examples
///
/// ```rust,no_run
/// # use litho_generator::llm::client::codex_provider::CodexRsClient;
/// // Provide an explicit path; existence is only checked on first call.
/// let client = CodexRsClient::new(
///     Some("/usr/local/bin/codex"),
///     Some(60),
///     Some("gpt-5".to_string()),
///     None,
/// ).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct CodexRsClient {
    /// Absolute path to the `codex` binary.
    pub binary_path: String,
    /// Maximum time (seconds) to wait for the subprocess to finish.
    pub timeout_secs: u64,
    /// Default model to use for codex calls.
    pub default_model: Option<String>,
    /// Optional working directory passed via `--cd`.
    pub working_dir: Option<PathBuf>,
}

impl CodexRsClient {
    /// Build a client, auto-detecting the binary when `binary_path` is `None`.
    ///
    /// Auto-detection order:
    /// 1. `CODEX_BINARY_PATH` environment variable.
    /// 2. Adjacent to the current executable (`<exe_dir>/codex[.exe]`).
    /// 3. `which codex` / `where codex` (PATH lookup).
    ///
    /// Returns an error only if the binary cannot be found at all; the client
    /// does **not** verify the binary is functional at construction time.
    pub fn new(
        binary_path: Option<&str>,
        timeout_secs: Option<u64>,
        default_model: Option<String>,
        working_dir: Option<PathBuf>,
    ) -> Result<Self> {
        let path = match binary_path {
            Some(p) => p.to_string(),
            None => Self::detect_binary()?,
        };
        Ok(Self {
            binary_path: path,
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
            default_model,
            working_dir,
        })
    }

    /// Detect the `codex` binary using a simple priority chain.
    fn detect_binary() -> Result<String> {
        // 1. Explicit environment override.
        if let Ok(env_path) = std::env::var("CODEX_BINARY_PATH")
            && !env_path.is_empty()
        {
            return Ok(env_path);
        }
        if let Ok(env_path) = std::env::var("CODEX_BIN")
            && !env_path.is_empty()
        {
            return Ok(env_path);
        }

        // 2. Sibling of the current executable.
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            for candidate in &["codex.exe", "codex"] {
                let p = dir.join(candidate);
                if p.exists() {
                    return Ok(p.to_string_lossy().into_owned());
                }
            }
        }

        // 3. PATH lookup via `where` (Windows) or `which` (Unix).
        #[cfg(target_os = "windows")]
        let (finder, arg) = ("where", "codex");
        #[cfg(not(target_os = "windows"))]
        let (finder, arg) = ("which", "codex");

        let out = std::process::Command::new(finder)
            .arg(arg)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok());

        if let Some(path_str) = out {
            let first_line = path_str.lines().next().unwrap_or("").trim().to_string();
            if !first_line.is_empty() {
                return Ok(first_line);
            }
        }

        bail!(
            "[CodexRs] Could not locate codex binary. \
             Set CODEX_BINARY_PATH or ensure codex is on PATH."
        )
    }

    /// Send a prompt to the codex binary and return the raw stdout response.
    ///
    /// The system and user prompts are combined as:
    /// `"<system>\n\n<user>"` and passed via the `-q` flag.
    pub async fn prompt(&self, system: &str, user: &str) -> Result<String> {
        self.prompt_with_model(system, user, None).await
    }

    pub async fn prompt_with_model(
        &self,
        system: &str,
        user: &str,
        model: Option<&str>,
    ) -> Result<String> {
        eprintln!("[CodexRs] Invoking codex-rs fallback...");

        let combined = format!("{}\n\n{}", system, user);
        let effective_model = model
            .map(ToOwned::to_owned)
            .or_else(|| self.default_model.clone());

        // Spawn the child process.
        let mut command = Command::new(&self.binary_path);
        if let Some(ref cwd) = self.working_dir {
            command.arg("--cd").arg(cwd);
        }
        command.arg("--skip-git-repo-check");
        if let Some(ref model_name) = effective_model
            && !model_name.trim().is_empty()
        {
            command.arg("--model").arg(model_name);
        }

        let child = command
            .arg("-q")
            .arg(&combined)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "[CodexRs] Failed to spawn codex binary: {}",
                    self.binary_path
                )
            })?;

        // Apply a timeout.  `wait_with_output` consumes `child`, so we record
        // the OS pid beforehand for the kill-on-timeout path.
        let timeout_secs = self.timeout_secs;

        let result = timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;

        match result {
            Err(_elapsed) => {
                bail!(
                    "[CodexRs] Subprocess timed out after {} seconds",
                    timeout_secs
                )
            }
            Ok(Err(e)) => {
                bail!("[CodexRs] Subprocess I/O error: {}", e)
            }
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "[CodexRs] codex exited with status {}: {}",
                        output.status,
                        stderr.trim()
                    )
                }
                let stdout = String::from_utf8(output.stdout)
                    .context("[CodexRs] codex output was not valid UTF-8")?;
                Ok(stdout)
            }
        }
    }

    /// Run the codex binary and attempt to deserialise the output as `T`.
    ///
    /// Uses the same 4-strategy JSON cascade as `OllamaExtractorWrapper`:
    /// 1. Direct `serde_json::from_str`.
    /// 2. Extract from a fenced code block.
    /// 3. Extract the first `{…}` object.
    /// 4. Strip markdown fences and retry.
    pub async fn extract<T>(&self, system: &str, user: &str) -> Result<T>
    where
        T: DeserializeOwned + schemars::JsonSchema,
    {
        self.extract_with_model(system, user, None).await
    }

    pub async fn extract_with_model<T>(
        &self,
        system: &str,
        user: &str,
        model: Option<&str>,
    ) -> Result<T>
    where
        T: DeserializeOwned + schemars::JsonSchema,
    {
        let raw = self
            .prompt_structured_with_schema::<T>(system, user, model)
            .await?;
        let value = parse_json_response(&raw)?;
        let result: T = serde_json::from_value(value)
            .context("[CodexRs] Failed to deserialise codex output to target type")?;
        Ok(result)
    }

    async fn prompt_structured_with_schema<T>(
        &self,
        system: &str,
        user: &str,
        model: Option<&str>,
    ) -> Result<String>
    where
        T: schemars::JsonSchema,
    {
        let combined = format!("{}\n\n{}", system, user);
        let effective_model = model
            .map(ToOwned::to_owned)
            .or_else(|| self.default_model.clone());

        let schema_path =
            std::env::temp_dir().join(format!("litho-codex-schema-{}.json", Uuid::new_v4()));
        let output_path =
            std::env::temp_dir().join(format!("litho-codex-output-{}.json", Uuid::new_v4()));

        let schema = schemars::schema_for!(T);
        let schema_json = serde_json::to_vec_pretty(&schema)
            .context("[CodexRs] Failed to serialize output schema")?;
        tokio::fs::write(&schema_path, schema_json)
            .await
            .context("[CodexRs] Failed to write schema temp file")?;

        let run_result = self
            .run_structured_prompt(
                &combined,
                effective_model.as_deref(),
                &schema_path,
                &output_path,
            )
            .await;

        let output = match run_result {
            Ok(out) => {
                let maybe_last = tokio::fs::read_to_string(&output_path).await.ok();
                maybe_last.filter(|s| !s.trim().is_empty()).unwrap_or(out)
            }
            Err(err) => {
                let _ = tokio::fs::remove_file(&schema_path).await;
                let _ = tokio::fs::remove_file(&output_path).await;
                return Err(err);
            }
        };

        let _ = tokio::fs::remove_file(&schema_path).await;
        let _ = tokio::fs::remove_file(&output_path).await;
        Ok(output)
    }

    async fn run_structured_prompt(
        &self,
        combined_prompt: &str,
        model: Option<&str>,
        schema_path: &Path,
        output_path: &Path,
    ) -> Result<String> {
        let mut command = Command::new(&self.binary_path);
        if let Some(ref cwd) = self.working_dir {
            command.arg("--cd").arg(cwd);
        }
        command
            .arg("--skip-git-repo-check")
            .arg("--output-schema")
            .arg(schema_path)
            .arg("--output-last-message")
            .arg(output_path);

        if let Some(model_name) = model
            && !model_name.trim().is_empty()
        {
            command.arg("--model").arg(model_name);
        }

        let child = command
            .arg("-q")
            .arg(combined_prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "[CodexRs] Failed to spawn codex binary: {}",
                    self.binary_path
                )
            })?;

        let timeout_secs = self.timeout_secs;
        let result = timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;
        match result {
            Err(_) => bail!(
                "[CodexRs] Subprocess timed out after {} seconds",
                timeout_secs
            ),
            Ok(Err(e)) => bail!("[CodexRs] Subprocess I/O error: {}", e),
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "[CodexRs] codex exited with status {}: {}",
                        output.status,
                        stderr.trim()
                    )
                }
                String::from_utf8(output.stdout)
                    .context("[CodexRs] codex output was not valid UTF-8")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// JSON parsing helpers (free functions so they can be unit-tested standalone)
// ---------------------------------------------------------------------------

/// Parse a JSON [`Value`] from a raw LLM text response using 4 strategies.
pub fn parse_json_response(response: &str) -> Result<Value> {
    // Strategy 1: direct parse.
    if let Ok(v) = serde_json::from_str::<Value>(response) {
        return Ok(v);
    }

    // Strategy 2: markdown code block.
    if let Some(extracted) = extract_from_code_block(response)
        && let Ok(v) = serde_json::from_str::<Value>(&extracted)
    {
        return Ok(v);
    }

    // Strategy 3: first `{…}` object.
    if let Some(extracted) = extract_first_json_object(response)
        && let Ok(v) = serde_json::from_str::<Value>(&extracted)
    {
        return Ok(v);
    }

    // Strategy 4: strip fences then parse.
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str::<Value>(cleaned).with_context(|| {
        let preview: String = response.chars().take(200).collect();
        format!(
            "[CodexRs] Failed to parse JSON from codex output. Preview: {}",
            preview
        )
    })
}

fn extract_from_code_block(text: &str) -> Option<String> {
    JSON_CODE_BLOCK_REGEX
        .captures(text)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_string())
}

fn extract_first_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut end = start;

    for (i, c) in text[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth == 0 && end > start {
        Some(text[start..end].to_string())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestOutput {
        answer: String,
        confidence: u8,
    }

    // --- parse_json_response strategy tests ---

    #[test]
    fn strategy1_direct_json() {
        let raw = r#"{"answer":"hello","confidence":9}"#;
        let v = parse_json_response(raw).unwrap();
        assert_eq!(v["answer"], "hello");
        assert_eq!(v["confidence"], 9);
    }

    #[test]
    fn strategy2_markdown_code_block() {
        let raw =
            "Some preamble\n```json\n{\"answer\":\"hi\",\"confidence\":8}\n```\nTrailing text";
        let v = parse_json_response(raw).unwrap();
        assert_eq!(v["answer"], "hi");
    }

    #[test]
    fn strategy3_first_json_object() {
        let raw = "Here is the data: {\"answer\":\"hey\",\"confidence\":7} and some trailing text.";
        let v = parse_json_response(raw).unwrap();
        assert_eq!(v["answer"], "hey");
    }

    #[test]
    fn strategy4_strip_fences() {
        let raw = "```\n{\"answer\":\"strip\",\"confidence\":6}\n```";
        let v = parse_json_response(raw).unwrap();
        assert_eq!(v["confidence"], 6);
    }

    #[test]
    fn parse_error_on_garbage() {
        let result = parse_json_response("this is definitely not json !!!");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("[CodexRs]"));
    }

    #[test]
    fn deserialise_into_struct() {
        let raw = r#"{"answer":"structured","confidence":5}"#;
        let v = parse_json_response(raw).unwrap();
        let out: TestOutput = serde_json::from_value(v).unwrap();
        assert_eq!(out.answer, "structured");
        assert_eq!(out.confidence, 5);
    }

    // --- extract_from_code_block ---

    #[test]
    fn code_block_without_language_tag() {
        let raw = "```\n{\"key\":\"value\"}\n```";
        assert!(extract_from_code_block(raw).is_some());
    }

    #[test]
    fn code_block_with_json_tag() {
        let raw = "```json\n{\"key\":\"value\"}\n```";
        assert!(extract_from_code_block(raw).is_some());
    }

    // --- detect_binary (compile-only, no real binary needed) ---

    #[test]
    fn client_new_with_explicit_path() {
        // Providing an explicit (non-existent) path should succeed at
        // construction time — existence is only checked on first use.
        let client = CodexRsClient::new(Some("/fake/path/codex"), Some(30), None, None);
        assert!(client.is_ok());
        let c = client.unwrap();
        assert_eq!(c.binary_path, "/fake/path/codex");
        assert_eq!(c.timeout_secs, 30);
    }

    #[test]
    fn default_timeout_applied() {
        let client = CodexRsClient::new(Some("/fake/codex"), None, None, None).unwrap();
        assert_eq!(client.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn client_new_preserves_default_model_and_working_dir() {
        let cwd = std::env::temp_dir();
        let client = CodexRsClient::new(
            Some("/fake/codex"),
            Some(45),
            Some("gpt-5".to_string()),
            Some(cwd.clone()),
        )
        .unwrap();
        assert_eq!(client.timeout_secs, 45);
        assert_eq!(client.default_model.as_deref(), Some("gpt-5"));
        assert_eq!(client.working_dir.as_ref(), Some(&cwd));
    }

    #[test]
    fn detect_binary_prefers_new_env_name_over_legacy() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CODEX_BINARY_PATH", "C:\\preferred\\codex.exe");
            std::env::set_var("CODEX_BIN", "C:\\legacy\\codex.exe");
        }
        let detected = CodexRsClient::detect_binary().unwrap();
        assert_eq!(detected, "C:\\preferred\\codex.exe");
        unsafe {
            std::env::remove_var("CODEX_BINARY_PATH");
            std::env::remove_var("CODEX_BIN");
        }
    }

    #[test]
    fn detect_binary_falls_back_to_legacy_env_name() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("CODEX_BINARY_PATH");
            std::env::set_var("CODEX_BIN", "C:\\legacy\\codex.exe");
        }
        let detected = CodexRsClient::detect_binary().unwrap();
        assert_eq!(detected, "C:\\legacy\\codex.exe");
        unsafe {
            std::env::remove_var("CODEX_BIN");
        }
    }
}
