//! Ollama Structured Output Wrapper
//!
//! Ollama does not support native structured output (unlike OpenAI), so this module
//! provides a wrapper to parse JSON from Ollama's text responses and validate against schemas.
//!
//! ## LLM failure recovery (ported from deepwiki-rs Patches 3, 5, 8)
//!
//! Local 7B–8B models frequently produce malformed or wrapped output:
//!
//! - **Schema wrapper**: The model echoes the JSON Schema instead of data.
//!   Detected by `$schema`/`$defs`/`properties` keys → unwrapped via
//!   `unwrap_schema_wrapper`.
//!
//! - **Envelope wrappers**: The data object is nested under `result`, `data`,
//!   or `output` keys (sometimes double-encoded as a JSON string).
//!   Stripped by `unwrap_envelope` from [`crate::llm::serde_helpers`].
//!
//! - **Type coercion**: Field-level helpers in `serde_helpers` handle
//!   `String` where `Vec<String>` was expected, numbers where strings were
//!   expected, and `null` defaulting.

use anyhow::{Context, Result};
use regex::Regex;
use rig::{agent::Agent, completion::Prompt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use crate::llm::serde_helpers::unwrap_envelope;

/// JSON code block regex pattern
static JSON_CODE_BLOCK_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").unwrap());

/// Ollama structured output extractor
pub struct OllamaExtractorWrapper<T> {
    agent: Agent<rig::providers::ollama::CompletionModel<reqwest::Client>>,
    max_retries: u32,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> OllamaExtractorWrapper<T>
where
    T: JsonSchema + Serialize + for<'de> Deserialize<'de>,
{
    /// Create a new Ollama extractor
    pub fn new(
        agent: Agent<rig::providers::ollama::CompletionModel<reqwest::Client>>,
        max_retries: u32,
    ) -> Self {
        Self {
            agent,
            max_retries,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Execute structured extraction
    pub async fn extract(&self, prompt: &str) -> Result<T> {
        let mut last_error = None;

        for attempt in 1..=self.max_retries {
            let enhanced_prompt = self.build_prompt(prompt, last_error.as_deref());

            match self.try_extract(&enhanced_prompt, attempt as usize).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(format!("{:#}", e));
                    if attempt < self.max_retries {
                        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed after {} attempts. Last error: {}",
            self.max_retries,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        ))
    }

    /// Build enhanced prompt with schema and instructions
    fn build_prompt(&self, base_prompt: &str, previous_error: Option<&str>) -> String {
        let schema = schemars::schema_for!(T);
        let schema_json =
            serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string());

        let mut prompt = format!(
            "{}\n\n**CRITICAL: YOU MUST RETURN VALID JSON**\n\nYou MUST return the result as a valid JSON object that strictly follows this schema:\n\n```json\n{}\n```\n\n",
            base_prompt, schema_json
        );

        prompt.push_str("Requirements:\n");
        prompt.push_str(
            "1. Return a pure JSON object with the DATA, not the schema definition itself\n",
        );
        prompt.push_str("2. Do NOT include $schema, $defs, title, or type:object wrapper - return the data directly\n");
        prompt.push_str("3. All required fields must be present at the top level\n");
        prompt.push_str("4. Field types must match schema exactly\n");
        prompt.push_str("5. Arrays and nested objects must be correctly formatted\n\n");

        if let Some(error) = previous_error {
            prompt.push_str(&format!(
                "**Previous attempt failed with error: {}**\nPlease fix these issues and regenerate.\n\n",
                error
            ));
        }

        prompt
    }

    /// Try to execute extraction once.
    ///
    /// Applies a layered unwrapping pipeline before deserialization:
    /// 1. Parse raw text to `serde_json::Value` (4-strategy text parsing).
    /// 2. Strip JSON Schema wrapper (`$schema`/`properties`/`required`).
    /// 3. Strip response envelope wrappers (`result`/`data`/`output`).
    async fn try_extract(&self, prompt: &str, attempt: usize) -> Result<T> {
        let response = self
            .agent
            .prompt(prompt)
            .await
            .context("Failed to get response from Ollama")?;

        let mut parsed = self
            .parse_json_response(&response, attempt)
            .context("Failed to parse JSON from Ollama response")?;

        // Pass 1: Unwrap JSON Schema wrapper (Ollama 7B-8B models often wrap
        // their response in a JSON Schema structure with $schema/properties/required).
        parsed = Self::unwrap_schema_wrapper(parsed);

        // Pass 2: Unwrap common response envelope keys (result / data / output).
        // Some models nest the actual payload one level deeper.
        parsed = unwrap_envelope(parsed);

        self.validate_json(&parsed)?;

        let result: T = serde_json::from_value(parsed.clone()).with_context(|| {
            let json_str =
                serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| "invalid".to_string());
            format!(
                "Failed to deserialize JSON to target type on attempt {}. JSON structure: {}",
                attempt, json_str
            )
        })?;

        Ok(result)
    }

    /// Parse JSON response using multiple strategies.
    ///
    /// Strategies are tried in order of increasing cost:
    ///
    /// 1. **Direct**: parse the raw response as JSON.
    /// 2. **Code block**: extract from a ```` ```json ... ``` ```` fence.
    /// 3. **First object**: scan for the first balanced `{…}` in the text.
    /// 4. **Clean**: strip leading/trailing fence markers and retry.
    /// 5. **Double-encoded**: if the cleaned text is a JSON string whose
    ///    contents parse as an object, unwrap the extra layer of encoding.
    ///    Some 7B models emit `"{\"foo\": 1}"` instead of `{"foo": 1}`.
    fn parse_json_response(&self, response: &str, attempt: usize) -> Result<Value> {
        // Strategy 1: Try direct parsing
        if let Ok(json) = serde_json::from_str::<Value>(response) {
            return Ok(json);
        }

        // Strategy 2: Extract from markdown code blocks
        if let Some(json_str) = self.extract_from_code_block(response) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&json_str) {
                return Ok(parsed);
            }
        }

        // Strategy 3: Extract first JSON object
        if let Some(json_str) = self.extract_first_json_object(response) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&json_str) {
                return Ok(parsed);
            }
        }

        // Strategy 4: Clean fence markers and try parsing
        let cleaned = self.clean_response(response);
        if let Ok(parsed) = serde_json::from_str::<Value>(&cleaned) {
            return Ok(parsed);
        }

        // Strategy 5: Detect double-encoded JSON — the model returned a JSON
        // string whose content is itself a JSON object, e.g. `"{\"x\":1}"`.
        if let Ok(Value::String(inner)) = serde_json::from_str::<Value>(&cleaned) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&inner) {
                if parsed.is_object() || parsed.is_array() {
                    return Ok(parsed);
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed to parse JSON from Ollama response (attempt {}). Response preview: {}",
            attempt,
            response.chars().take(200).collect::<String>()
        ))
    }

    /// Extract JSON from markdown code blocks
    fn extract_from_code_block(&self, text: &str) -> Option<String> {
        JSON_CODE_BLOCK_REGEX
            .captures(text)
            .and_then(|cap| cap.get(1))
            .map(|m| m.as_str().to_string())
    }

    /// Extract first complete JSON object
    fn extract_first_json_object(&self, text: &str) -> Option<String> {
        let start = text.find('{')?;
        let mut depth = 0;
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

    /// Clean response text
    fn clean_response(&self, text: &str) -> String {
        text.trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string()
    }

    /// Validate basic JSON structure
    fn validate_json(&self, json: &Value) -> Result<()> {
        if !json.is_object() {
            anyhow::bail!("Expected JSON object, got: {}", json);
        }
        Ok(())
    }

    /// Unwrap JSON Schema wrapper if present.
    ///
    /// Ollama 7B-8B models sometimes return responses wrapped in a JSON Schema
    /// structure (with "$schema", "properties", "required", "type": "object")
    /// instead of a flat JSON object. This extracts the actual data from the
    /// "properties" field when the wrapper pattern is detected.
    fn unwrap_schema_wrapper(json: Value) -> Value {
        if let Some(obj) = json.as_object() {
            let has_schema = obj.contains_key("$schema") || obj.contains_key("$defs");
            let has_properties = obj.contains_key("properties");
            let has_required = obj.contains_key("required");
            let has_type_object = obj.get("type").and_then(|v| v.as_str()) == Some("object");

            if (has_schema || (has_required && has_type_object)) && has_properties {
                if let Some(properties) = obj.get("properties") {
                    if properties.is_object() {
                        return properties.clone();
                    }
                }
            }
        }
        json
    }
}
