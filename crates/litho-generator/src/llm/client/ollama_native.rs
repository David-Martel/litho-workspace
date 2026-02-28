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
use std::sync::LazyLock;

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

        let ollama = Ollama::new(host, port);
        Ok(Self { inner: ollama })
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
        let mut messages = Vec::with_capacity(2);
        if !system_prompt.is_empty() {
            messages.push(ChatMessage::system(system_prompt.to_string()));
        }
        messages.push(ChatMessage::user(user_prompt.to_string()));

        let mut request = ChatMessageRequest::new(model.to_string(), messages);

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
        let max_retries = config.retry_attempts.max(1);
        let mut last_error: Option<String> = None;

        for attempt in 1..=max_retries {
            let enhanced = Self::build_extraction_prompt::<T>(user_prompt, last_error.as_deref());

            match self
                .try_extract::<T>(model, system_prompt, &enhanced, config, attempt as usize)
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

    // -- internals ----------------------------------------------------------

    fn build_options(&self, config: &LLMConfig) -> ollama_rs::models::ModelOptions {
        let mut opts = ollama_rs::models::ModelOptions::default();

        // Context window – the most critical parameter.
        // Configurable via `context_window` in litho.toml [llm] section.
        // Gemma3 12B-IT-QAT supports up to 131072; default is 32768.
        opts = opts.num_ctx(config.context_window as u64);

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
}

// ---------------------------------------------------------------------------
// Standalone JSON parsing helpers (shared with OllamaExtractorWrapper)
// ---------------------------------------------------------------------------

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
}
