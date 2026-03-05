//! Native Ollama provider using `ollama-rs`.
//!
//! Bypasses rig-core's OpenAI-compatibility layer to access Ollama's native API
//! directly.  This enables:
//!
//! * **Tool calling** for models like Gemma 3 that support it natively but not
//!   through the `/v1` shim.
//! * **`num_ctx` control** — set the context window explicitly instead of relying
//!   on Modelfile defaults (often 2 048).
//! * **Full Ollama-specific options** (`repeat_penalty`, `num_gpu`, etc.).

use anyhow::{Context, Result};
use ollama_rs::{
    Ollama,
    generation::chat::{ChatMessage, request::ChatMessageRequest},
};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::RwLock;

use crate::{config::LLMConfig, llm::serde_helpers::unwrap_envelope};

/// Regex shared with `ollama_extractor.rs`.
static JSON_CODE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").unwrap());

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Thin wrapper around `ollama_rs::Ollama` that knows about the litho
/// `LLMConfig` for parameter forwarding.
#[derive(Clone)]
pub struct OllamaNativeClient {
    inner: Ollama,
    base_url: String,
    http: reqwest::Client,
    local_models_cache: Arc<RwLock<Vec<String>>>,
}

impl OllamaNativeClient {
    /// Build from the LLM configuration.
    pub fn from_config(config: &LLMConfig) -> Result<Self> {
        let base = &config.api_base_url;

        // Parse host and port from the api_base_url.
        // ollama-rs wants  (host_without_port, port)  as separate values.
        let url = url::Url::parse(base)
            .unwrap_or_else(|_| url::Url::parse("http://localhost:11434").unwrap());
        let host = format!(
            "{}://{}",
            url.scheme(),
            url.host_str().unwrap_or("localhost")
        );
        let port = url.port().unwrap_or(11434);
        let base_url = format!("{}:{}", host, port);

        let ollama = Ollama::new(host, port);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds.clamp(5, 900)))
            .build()
            .context("Failed to build Ollama metadata HTTP client")?;

        Ok(Self {
            inner: ollama,
            base_url,
            http,
            local_models_cache: Arc::new(RwLock::new(Vec::new())),
        })
    }

    // -- simple chat (no tools) ---------------------------------------------

    /// Single-turn chat completion.  Returns raw text.
    pub async fn chat(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        config: &LLMConfig,
    ) -> Result<String> {
        let resolved_model = self.ensure_model_available(model, config).await?;

        let mut messages = Vec::with_capacity(2);
        if !system_prompt.is_empty() {
            messages.push(ChatMessage::system(system_prompt.to_string()));
        }
        messages.push(ChatMessage::user(user_prompt.to_string()));

        let mut request = ChatMessageRequest::new(resolved_model.clone(), messages);

        // Inject Ollama-native options via ModelOptions.
        let options = self.build_options(config);
        request = request.options(options);

        let response = self
            .inner
            .send_chat_messages(request)
            .await
            .context("ollama-rs: send_chat_messages failed")?;

        Ok(response.message.content)
    }

    // -- structured extraction ----------------------------------------------

    /// Chat + JSON parse pipeline.  Re-uses the same 5-strategy cascade from
    /// `OllamaExtractorWrapper` but driven by `ollama-rs` instead of rig-core.
    pub async fn extract<T>(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        config: &LLMConfig,
    ) -> Result<T>
    where
        T: JsonSchema + Serialize + for<'de> Deserialize<'de>,
    {
        let resolved_model = self.ensure_model_available(model, config).await?;
        let max_retries = config.retry_attempts.max(1);
        let mut last_error: Option<String> = None;

        for attempt in 1..=max_retries {
            let enhanced = Self::build_extraction_prompt::<T>(user_prompt, last_error.as_deref());

            match self
                .try_extract::<T>(
                    &resolved_model,
                    system_prompt,
                    &enhanced,
                    config,
                    attempt as usize,
                )
                .await
            {
                Ok(val) => return Ok(val),
                Err(e) => {
                    last_error = Some(format!("{e:#}"));
                    if attempt < max_retries {
                        tokio::time::sleep(std::time::Duration::from_millis(config.retry_delay_ms))
                            .await;
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "ollama-rs extract failed after {max_retries} attempts. Last: {}",
            last_error.unwrap_or_default()
        ))
    }

    /// Prepare runtime capabilities for Ollama before the main pipeline starts.
    ///
    /// - resolves configured models against local `/api/tags` inventory
    /// - optionally pulls missing models
    /// - optionally sends one-shot warmup calls
    pub async fn prepare_runtime(&self, config: &LLMConfig) -> Result<()> {
        let mut requested = vec![
            config.model_efficient.clone(),
            config.model_powerful.clone(),
        ];
        requested.extend(config.ollama_required_models.clone());
        requested.retain(|v| !v.trim().is_empty());
        requested = dedup_preserve_order(requested);

        if requested.is_empty() {
            let local = self.get_local_models().await.unwrap_or_default();
            if local.is_empty() {
                anyhow::bail!(
                    "No Ollama models configured and no local models available via /api/tags"
                );
            }
            return Ok(());
        }

        let mut resolved_models = Vec::new();
        for model in requested {
            let resolved = self.ensure_model_available(&model, config).await?;
            resolved_models.push(resolved);
        }
        resolved_models = dedup_preserve_order(resolved_models);

        if config.ollama_warm_models_on_start {
            for model in resolved_models {
                if let Err(err) = self.warm_model(&model, config).await {
                    eprintln!(
                        "⚠️  Ollama warmup failed for '{}': {} (continuing)",
                        model, err
                    );
                }
            }
        }

        Ok(())
    }

    // -- internals ----------------------------------------------------------

    fn build_options(&self, config: &LLMConfig) -> ollama_rs::models::ModelOptions {
        let mut opts = ollama_rs::models::ModelOptions::default();

        opts = opts.num_ctx(config.resolve_context_window() as u64);

        // Generation limit.
        opts = opts.num_predict(config.max_tokens as i32);

        if let Some(t) = config.temperature {
            opts = opts.temperature(t as f32);
        }

        opts
    }

    fn build_extraction_prompt<T: JsonSchema>(base: &str, previous_error: Option<&str>) -> String {
        let schema = schemars::schema_for!(T);
        let schema_json =
            serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string());

        let mut prompt = format!(
            "{base}\n\n\
             **CRITICAL: YOU MUST RETURN VALID JSON**\n\n\
             Return a valid JSON object strictly following this schema:\n\n\
             ```json\n{schema_json}\n```\n\n\
             Requirements:\n\
             1. Return the DATA, not the schema definition itself\n\
             2. Do NOT include $schema, $defs, title, or type:object wrapper\n\
             3. All required fields must be present\n\
             4. Field types must match schema exactly\n\
             5. Arrays and nested objects must be correctly formatted\n\n"
        );

        if let Some(err) = previous_error {
            prompt.push_str(&format!(
                "**Previous attempt failed: {err}**\nFix these issues and regenerate.\n\n"
            ));
        }

        prompt
    }

    async fn try_extract<T>(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        config: &LLMConfig,
        attempt: usize,
    ) -> Result<T>
    where
        T: JsonSchema + Serialize + for<'de> Deserialize<'de>,
    {
        let response = self.chat(model, system_prompt, user_prompt, config).await?;

        let mut parsed = parse_json_response(&response, attempt)
            .context("JSON parse from ollama-rs response failed")?;

        // Pass 1: strip Schema wrapper.
        parsed = unwrap_schema_wrapper(parsed);
        // Pass 2: strip response envelope.
        parsed = unwrap_envelope(parsed);

        if !parsed.is_object() {
            anyhow::bail!("Expected JSON object, got: {parsed}");
        }

        serde_json::from_value(parsed.clone()).with_context(|| {
            let s = serde_json::to_string_pretty(&parsed).unwrap_or_default();
            format!("Deserialization failed (attempt {attempt}): {s}")
        })
    }

    async fn ensure_model_available(&self, requested: &str, config: &LLMConfig) -> Result<String> {
        let requested = requested.trim();
        let local_models = self.get_local_models().await.unwrap_or_default();

        if requested.is_empty() {
            if let Some(best) = choose_best_local_model(&local_models, &[]) {
                return Ok(best);
            }
            anyhow::bail!("No Ollama model provided and no local models available");
        }

        if let Some(found) = find_matching_local_model(&local_models, requested) {
            return Ok(found.to_string());
        }

        if config.ollama_auto_pull_missing_models {
            eprintln!("📥 Pulling missing Ollama model '{}'", requested);
            self.pull_model(requested).await?;
            let refreshed = self.refresh_local_models().await.unwrap_or_default();
            if let Some(found) = find_matching_local_model(&refreshed, requested) {
                return Ok(found.to_string());
            }
        }

        if config.ollama_auto_detect_models {
            let mut preferred = vec![
                requested.to_string(),
                config.model_efficient.clone(),
                config.model_powerful.clone(),
            ];
            preferred.extend(config.ollama_required_models.clone());
            preferred.retain(|m| !m.trim().is_empty());

            if let Some(fallback) = choose_best_local_model(&local_models, &preferred) {
                eprintln!(
                    "ℹ️  Ollama model '{}' not found locally, using '{}' instead",
                    requested, fallback
                );
                return Ok(fallback);
            }
        }

        Ok(requested.to_string())
    }

    async fn warm_model(&self, model: &str, config: &LLMConfig) -> Result<()> {
        let mut request = ChatMessageRequest::new(
            model.to_string(),
            vec![ChatMessage::user("ping".to_string())],
        );
        let options = ollama_rs::models::ModelOptions::default()
            .num_ctx(2_048)
            .num_predict(1)
            .temperature(0.0);
        request = request.options(options);

        let timeout_secs = config.timeout_seconds.clamp(5, 120);
        tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.inner.send_chat_messages(request),
        )
        .await
        .context("warmup timed out")?
        .context("warmup request failed")?;
        Ok(())
    }

    async fn pull_model(&self, model: &str) -> Result<()> {
        let payload = serde_json::json!({
            "name": model,
            "stream": false
        });
        let endpoint = format!("{}/api/pull", self.base_url.trim_end_matches('/'));

        let resp = self
            .http
            .post(&endpoint)
            .json(&payload)
            .send()
            .await
            .context("Failed to call Ollama /api/pull")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama /api/pull failed ({}): {}", status, body);
        }

        Ok(())
    }

    async fn get_local_models(&self) -> Result<Vec<String>> {
        {
            let cache = self.local_models_cache.read().await;
            if !cache.is_empty() {
                return Ok(cache.clone());
            }
        }
        self.refresh_local_models().await
    }

    async fn refresh_local_models(&self) -> Result<Vec<String>> {
        let models = self.fetch_local_models().await?;
        let mut cache = self.local_models_cache.write().await;
        *cache = models.clone();
        Ok(models)
    }

    async fn fetch_local_models(&self) -> Result<Vec<String>> {
        let endpoint = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .get(&endpoint)
            .send()
            .await
            .context("Failed to call Ollama /api/tags")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama /api/tags failed ({}): {}", status, body);
        }

        let tags: OllamaTagsResponse = resp
            .json()
            .await
            .context("Failed to decode Ollama /api/tags response")?;

        let mut names = Vec::new();
        for model in tags.models {
            if let Some(name) = model.name
                && !name.trim().is_empty()
            {
                names.push(name);
            }
            if let Some(name) = model.model
                && !name.trim().is_empty()
            {
                names.push(name);
            }
        }
        Ok(dedup_preserve_order(names))
    }
}

// ---------------------------------------------------------------------------
// Standalone JSON parsing helpers (shared with OllamaExtractorWrapper)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// 5-strategy JSON parse cascade.
pub fn parse_json_response(response: &str, _attempt: usize) -> Result<Value> {
    // Strategy 1: direct
    if let Ok(v) = serde_json::from_str::<Value>(response) {
        return Ok(v);
    }

    // Strategy 2: code block
    if let Some(cap) = JSON_CODE_BLOCK_RE.captures(response)
        && let Some(m) = cap.get(1)
        && let Ok(v) = serde_json::from_str::<Value>(m.as_str())
    {
        return Ok(v);
    }

    // Strategy 3: first balanced `{…}`
    if let Some(obj) = extract_first_json_object(response)
        && let Ok(v) = serde_json::from_str::<Value>(&obj)
    {
        return Ok(v);
    }

    // Strategy 4: strip fences
    let cleaned = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        return Ok(v);
    }

    // Strategy 5: double-encoded
    if let Ok(Value::String(inner)) = serde_json::from_str::<Value>(cleaned)
        && let Ok(v) = serde_json::from_str::<Value>(&inner)
        && (v.is_object() || v.is_array())
    {
        return Ok(v);
    }

    Err(anyhow::anyhow!(
        "No JSON found in response (first 200 chars): {}",
        response.chars().take(200).collect::<String>()
    ))
}

fn extract_first_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0i32;
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
    (depth == 0 && end > start).then(|| text[start..end].to_string())
}

fn unwrap_schema_wrapper(json: Value) -> Value {
    if let Some(obj) = json.as_object() {
        let has_schema = obj.contains_key("$schema") || obj.contains_key("$defs");
        let has_properties = obj.contains_key("properties");
        let has_required = obj.contains_key("required");
        let has_type_object = obj.get("type").and_then(|v| v.as_str()) == Some("object");

        if (has_schema || (has_required && has_type_object))
            && has_properties
            && let Some(props) = obj.get("properties")
            && props.is_object()
        {
            return props.clone();
        }
    }
    json
}

fn dedup_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let key = item.trim().to_ascii_lowercase();
        if key.is_empty() {
            continue;
        }
        if seen.insert(key) {
            out.push(item.trim().to_string());
        }
    }
    out
}

fn choose_best_local_model(local_models: &[String], preferred: &[String]) -> Option<String> {
    for candidate in preferred {
        if let Some(found) = find_matching_local_model(local_models, candidate) {
            return Some(found.to_string());
        }
    }

    let ranked_keywords = [
        "qwen2.5",
        "qwen3",
        "gemma3",
        "gemma2",
        "deepseek",
        "llama3",
        "llama",
        "mistral",
        "phi",
        "codellama",
    ];
    for keyword in ranked_keywords {
        if let Some(found) = local_models
            .iter()
            .find(|m| m.to_ascii_lowercase().contains(keyword))
        {
            return Some(found.to_string());
        }
    }

    local_models.first().cloned()
}

fn find_matching_local_model<'a>(local_models: &'a [String], requested: &str) -> Option<&'a str> {
    let requested = requested.trim();
    if requested.is_empty() {
        return None;
    }

    if let Some(found) = local_models
        .iter()
        .find(|m| m.eq_ignore_ascii_case(requested))
    {
        return Some(found.as_str());
    }

    let requested_base = requested.split(':').next().unwrap_or(requested);
    if let Some(found) = local_models.iter().find(|m| {
        let base = m.split(':').next().unwrap_or(m.as_str());
        base.eq_ignore_ascii_case(requested_base)
    }) {
        return Some(found.as_str());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_direct_json() {
        let input = r#"{"name":"test","value":42}"#;
        let v = parse_json_response(input, 1).unwrap();
        assert_eq!(v["name"], "test");
    }

    #[test]
    fn parse_code_block() {
        let input = "Here is the result:\n```json\n{\"ok\":true}\n```\nDone.";
        let v = parse_json_response(input, 1).unwrap();
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn parse_first_object() {
        let input = "Thinking... The answer is {\"x\":1} and that is it.";
        let v = parse_json_response(input, 1).unwrap();
        assert_eq!(v["x"], 1);
    }

    #[test]
    fn unwrap_schema() {
        let input: Value = serde_json::json!({
            "$schema": "...",
            "type": "object",
            "properties": {"name": "David"},
            "required": ["name"]
        });
        let unwrapped = unwrap_schema_wrapper(input);
        assert_eq!(unwrapped["name"], "David");
    }

    #[test]
    fn match_local_model_exact_and_family() {
        let local = vec![
            "qwen2.5-coder:7b".to_string(),
            "gemma3:12b".to_string(),
            "llama3.1:8b".to_string(),
        ];
        assert_eq!(
            find_matching_local_model(&local, "gemma3:12b"),
            Some("gemma3:12b")
        );
        assert_eq!(
            find_matching_local_model(&local, "gemma3"),
            Some("gemma3:12b")
        );
        assert_eq!(
            find_matching_local_model(&local, "qwen2.5-coder:14b"),
            Some("qwen2.5-coder:7b")
        );
    }

    #[test]
    fn best_local_model_prefers_requested_then_ranked() {
        let local = vec![
            "mistral:7b".to_string(),
            "qwen2.5-coder:7b".to_string(),
            "llama3.1:8b".to_string(),
        ];
        let preferred = vec!["gemma3".to_string(), "qwen2.5-coder".to_string()];
        let selected = choose_best_local_model(&local, &preferred);
        assert_eq!(selected.as_deref(), Some("qwen2.5-coder:7b"));

        let selected2 = choose_best_local_model(&local, &["nonexistent".to_string()]);
        assert_eq!(selected2.as_deref(), Some("qwen2.5-coder:7b"));
    }

    #[test]
    fn dedup_preserves_order_case_insensitive() {
        let input = vec![
            "qwen2.5-coder:7b".to_string(),
            "QWEN2.5-CODER:7B".to_string(),
            "gemma3:12b".to_string(),
        ];
        let out = dedup_preserve_order(input);
        assert_eq!(out, vec!["qwen2.5-coder:7b", "gemma3:12b"]);
    }
}
