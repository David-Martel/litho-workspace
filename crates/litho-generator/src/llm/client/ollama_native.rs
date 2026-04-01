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
    generation::{
        chat::{ChatMessage, ChatMessageResponse, request::ChatMessageRequest},
        parameters::{FormatType, JsonStructure, KeepAlive, TimeUnit},
    },
};
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, Semaphore};

use crate::{
    config::{LLMConfig, model_default_context_window},
    llm::serde_helpers::unwrap_envelope,
    utils::token_estimator::TokenEstimator,
};

/// Regex shared with `ollama_extractor.rs`.
static JSON_CODE_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"```(?:json)?\s*(\{[\s\S]*?\})\s*```").unwrap());
static TOKEN_ESTIMATOR: LazyLock<TokenEstimator> = LazyLock::new(TokenEstimator::new);

const DEFAULT_CONTEXT_WINDOW: u32 = 32_768;

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
    local_models_cache: Arc<RwLock<LocalModelsCache>>,
    request_limiter: Arc<Semaphore>,
}

#[derive(Debug, Clone, Default)]
struct LocalModelsCache {
    models: Vec<String>,
    refreshed_at: Option<Instant>,
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
            local_models_cache: Arc::new(RwLock::new(LocalModelsCache::default())),
            request_limiter: Arc::new(Semaphore::new(
                config
                    .ollama_max_in_flight
                    .unwrap_or_else(|| config.max_parallels.clamp(1, 3))
                    .clamp(1, 128),
            )),
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
        let max_retries = config.retry_attempts.max(1);
        let mut last_error: Option<String> = None;

        for attempt in 1..=max_retries {
            let mut messages = Vec::with_capacity(2);
            if !system_prompt.is_empty() {
                messages.push(ChatMessage::system(system_prompt.to_string()));
            }
            messages.push(ChatMessage::user(user_prompt.to_string()));

            let mut request = ChatMessageRequest::new(resolved_model.clone(), messages);
            let options = self.build_options(&resolved_model, system_prompt, user_prompt, config);
            request =
                apply_common_request_options(request, options, config.ollama_keep_alive_seconds);

            match self.send_with_timeout(request, config, "chat").await {
                Ok(response) => {
                    maybe_log_perf_metrics(&resolved_model, "chat", &response, config);
                    let content = response.message.content.trim().to_string();
                    if !content.is_empty() {
                        return Ok(content);
                    }
                    last_error = Some("empty Ollama response content".to_string());
                }
                Err(err) => {
                    last_error = Some(err.to_string());
                }
            }

            if attempt < max_retries {
                let delay = retry_delay_with_backoff_and_jitter(config.retry_delay_ms, attempt);
                tokio::time::sleep(delay).await;
            }
        }

        Err(anyhow::anyhow!(
            "ollama-rs chat failed after {max_retries} attempts: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        ))
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
                        let delay =
                            retry_delay_with_backoff_and_jitter(config.retry_delay_ms, attempt);
                        tokio::time::sleep(delay).await;
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
            let local = self.get_local_models(config).await.unwrap_or_default();
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

    fn build_options(
        &self,
        model: &str,
        system_prompt: &str,
        user_prompt: &str,
        config: &LLMConfig,
    ) -> ollama_rs::models::ModelOptions {
        let mut opts = ollama_rs::models::ModelOptions::default();

        let num_ctx = resolve_num_ctx_for_request(model, system_prompt, user_prompt, config);
        opts = opts.num_ctx(num_ctx as u64);

        // Generation limit.
        let default_predict = (config.max_tokens.min(i32::MAX as u32)) as i32;
        let num_predict = config
            .ollama_num_predict
            .filter(|v| *v != 0)
            .unwrap_or(default_predict);
        opts = opts.num_predict(num_predict);

        if let Some(t) = config.temperature {
            opts = opts.temperature(t as f32);
        }

        if let Some(num_gpu) = config.ollama_num_gpu {
            opts = opts.num_gpu(num_gpu);
        }

        if let Some(num_thread) = config.ollama_num_thread
            && num_thread > 0
        {
            opts = opts.num_thread(num_thread);
        }

        if let Some(top_k) = config.ollama_top_k
            && top_k > 0
        {
            opts = opts.top_k(top_k);
        }

        if let Some(top_p) = config.ollama_top_p
            && top_p.is_finite()
            && (0.0..=1.0).contains(&top_p)
        {
            opts = opts.top_p(top_p as f32);
        }

        if let Some(repeat_last_n) = config.ollama_repeat_last_n {
            opts = opts.repeat_last_n(repeat_last_n);
        }

        if let Some(repeat_penalty) = config.ollama_repeat_penalty
            && repeat_penalty.is_finite()
            && repeat_penalty > 0.0
        {
            opts = opts.repeat_penalty(repeat_penalty as f32);
        }

        if let Some(tfs_z) = config.ollama_tfs_z
            && tfs_z.is_finite()
            && tfs_z > 0.0
        {
            opts = opts.tfs_z(tfs_z as f32);
        }

        if let Some(seed) = config.ollama_seed {
            opts = opts.seed(seed);
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
        let mut messages = Vec::with_capacity(2);
        if !system_prompt.is_empty() {
            messages.push(ChatMessage::system(system_prompt.to_string()));
        }
        messages.push(ChatMessage::user(user_prompt.to_string()));

        let mut request = ChatMessageRequest::new(model.to_string(), messages).format(
            FormatType::StructuredJson(Box::new(JsonStructure::new::<T>())),
        );
        let options = self.build_options(model, system_prompt, user_prompt, config);
        request = apply_common_request_options(request, options, config.ollama_keep_alive_seconds);

        let response = self.send_with_timeout(request, config, "extract").await?;
        maybe_log_perf_metrics(model, "extract", &response, config);
        let response = response.message.content;

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

    async fn send_with_timeout(
        &self,
        request: ChatMessageRequest,
        config: &LLMConfig,
        phase: &str,
    ) -> Result<ollama_rs::generation::chat::ChatMessageResponse> {
        let queue_started = Instant::now();
        let _permit = self
            .request_limiter
            .clone()
            .acquire_owned()
            .await
            .context("ollama request limiter closed")?;
        if config.ollama_log_perf_metrics {
            let queue_wait_ms = queue_started.elapsed().as_secs_f64() * 1000.0;
            if queue_wait_ms >= 1.0 {
                eprintln!("📊 Ollama[{phase}] queue_wait_ms={queue_wait_ms:.1}");
            }
        }
        let timeout_secs = config.timeout_seconds.clamp(5, 900);
        tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.inner.send_chat_messages(request),
        )
        .await
        .with_context(|| format!("ollama-rs {phase} timed out after {timeout_secs}s"))?
        .with_context(|| format!("ollama-rs {phase} request failed"))
    }

    async fn ensure_model_available(&self, requested: &str, config: &LLMConfig) -> Result<String> {
        let requested = requested.trim();
        let mut local_models = self.get_local_models(config).await.unwrap_or_default();

        if requested.is_empty() {
            if config.ollama_strict_model_selection {
                anyhow::bail!("No Ollama model configured and strict model selection is enabled");
            }
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
            local_models = refreshed;
        }

        if config.ollama_strict_model_selection {
            anyhow::bail!(
                "Ollama model '{}' is not available locally (strict model selection enabled)",
                requested
            );
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

        anyhow::bail!(
            "Ollama model '{}' is not available locally and auto-detection is disabled",
            requested
        )
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
        request = apply_common_request_options(request, options, config.ollama_keep_alive_seconds);

        self.send_with_timeout(request, config, "warmup").await?;
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

    async fn get_local_models(&self, config: &LLMConfig) -> Result<Vec<String>> {
        {
            let cache = self.local_models_cache.read().await;
            let ttl = Duration::from_secs(config.ollama_local_models_cache_ttl_seconds.max(1));
            if !cache.models.is_empty()
                && cache
                    .refreshed_at
                    .map(|t| t.elapsed() <= ttl)
                    .unwrap_or(false)
            {
                return Ok(cache.models.clone());
            }
        }
        self.refresh_local_models().await
    }

    async fn refresh_local_models(&self) -> Result<Vec<String>> {
        let models = self.fetch_local_models().await?;
        let mut cache = self.local_models_cache.write().await;
        cache.models = models.clone();
        cache.refreshed_at = Some(Instant::now());
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
    let mut in_string = false;
    let mut escaped = false;

    for (i, ch) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i + 1;
                    return Some(text[start..end].to_string());
                }
            }
            _ => {}
        }
    }

    None
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

fn resolve_num_ctx_for_request(
    model: &str,
    system_prompt: &str,
    user_prompt: &str,
    config: &LLMConfig,
) -> u32 {
    let explicit_context = config.context_window != DEFAULT_CONTEXT_WINDOW;
    let default_model_ctx = model_default_context_window(&model.to_ascii_lowercase());
    let hard_cap = if explicit_context {
        config.context_window
    } else {
        cmp::min(default_model_ctx, config.ollama_adaptive_context_max)
    };
    if !config.ollama_adaptive_context {
        return cmp::max(hard_cap, 1024);
    }

    let request_text = format!(
        "system:\n{}\n\nuser:\n{}",
        system_prompt.trim(),
        user_prompt.trim()
    );
    let estimated_tokens = TOKEN_ESTIMATOR
        .estimate_tokens(&request_text)
        .estimated_tokens as f64;
    let chars_per_token = config.ollama_chars_per_token.max(1) as f64;
    let scale = 4.0 / chars_per_token;
    let estimated_tokens = (estimated_tokens * scale).ceil().max(1.0) as u32;
    let output_budget = config
        .ollama_num_predict
        .filter(|v| *v > 0)
        .map(|v| v as u32)
        .unwrap_or_else(|| config.max_tokens.max(1));
    let requested = estimated_tokens
        .saturating_add(output_budget)
        .saturating_add(config.ollama_adaptive_headroom_tokens);
    let requested = round_up_to_step_u32(requested, config.ollama_adaptive_step_tokens.max(1));
    let min_ctx = cmp::min(config.ollama_adaptive_context_min, hard_cap.max(1));
    requested.clamp(min_ctx, hard_cap.max(min_ctx))
}

fn round_up_to_step_u32(value: u32, step: u32) -> u32 {
    if step <= 1 {
        return value;
    }
    let rem = value % step;
    if rem == 0 {
        value
    } else {
        value.saturating_add(step - rem)
    }
}

fn retry_delay_with_backoff_and_jitter(base_delay_ms: u64, attempt: u32) -> Duration {
    let base = base_delay_ms.max(1);
    let exp = attempt.saturating_sub(1).min(6);
    let scaled = base.saturating_mul(1u64 << exp);
    let capped = scaled.min(15_000);

    // Add bounded jitter to avoid retry stampedes when many requests fail together.
    let jitter_window = (capped / 5).max(1);
    let jitter_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let jitter = jitter_seed % jitter_window;

    Duration::from_millis(capped.saturating_add(jitter))
}

fn apply_common_request_options(
    mut request: ChatMessageRequest,
    options: ollama_rs::models::ModelOptions,
    keep_alive_seconds: i64,
) -> ChatMessageRequest {
    request = request.options(options);
    apply_keep_alive(request, keep_alive_seconds)
}

fn apply_keep_alive(request: ChatMessageRequest, keep_alive_seconds: i64) -> ChatMessageRequest {
    match keep_alive_seconds {
        i64::MIN..=-1 => request.keep_alive(KeepAlive::Indefinitely),
        0 => request.keep_alive(KeepAlive::UnloadOnCompletion),
        secs => request.keep_alive(KeepAlive::Until {
            time: secs as u64,
            unit: TimeUnit::Seconds,
        }),
    }
}

fn maybe_log_perf_metrics(
    model: &str,
    phase: &str,
    response: &ChatMessageResponse,
    config: &LLMConfig,
) {
    if !config.ollama_log_perf_metrics {
        return;
    }

    let Some(final_data) = response.final_data.as_ref() else {
        return;
    };

    let total_ms = final_data.total_duration as f64 / 1_000_000.0;
    let eval_ms = final_data.eval_duration as f64 / 1_000_000.0;
    let toks_per_sec = if final_data.eval_duration > 0 {
        (final_data.eval_count as f64) / (final_data.eval_duration as f64 / 1_000_000_000.0)
    } else {
        0.0
    };

    eprintln!(
        "📊 Ollama[{phase}] model='{model}' total_ms={total_ms:.1} eval_ms={eval_ms:.1} eval_tokens={} tok_s={toks_per_sec:.1} prompt_tokens={}",
        final_data.eval_count, final_data.prompt_eval_count
    );
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
    fn parse_first_object_with_braces_in_string() {
        let input = "trace {\"msg\":\"{brace}\",\"ok\":true} done";
        let v = parse_json_response(input, 1).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["msg"], "{brace}");
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

    #[test]
    fn adaptive_ctx_clamps_small_and_large_requests() {
        let cfg = LLMConfig {
            context_window: DEFAULT_CONTEXT_WINDOW,
            max_tokens: 1,
            ollama_adaptive_context: true,
            ollama_adaptive_context_min: 4096,
            ollama_adaptive_context_max: 32768,
            ollama_chars_per_token: 4,
            ollama_adaptive_headroom_tokens: 1024,
            ollama_adaptive_step_tokens: 1024,
            ..LLMConfig::default()
        };

        let tiny = resolve_num_ctx_for_request("mistral:7b", "", "tiny", &cfg);
        assert_eq!(tiny, 4096);

        let huge_prompt = "x".repeat(500_000);
        let huge = resolve_num_ctx_for_request("mistral:7b", "", &huge_prompt, &cfg);
        assert_eq!(huge, 32768);
    }

    #[test]
    fn adaptive_ctx_respects_explicit_context_cap() {
        let cfg = LLMConfig {
            context_window: 8192,
            ollama_adaptive_context: true,
            ollama_adaptive_context_min: 4096,
            ollama_adaptive_context_max: 131072,
            ollama_chars_per_token: 4,
            ollama_adaptive_headroom_tokens: 4096,
            ollama_adaptive_step_tokens: 1024,
            ..LLMConfig::default()
        };

        let huge_prompt = "x".repeat(300_000);
        let ctx = resolve_num_ctx_for_request("gemma3:12b", "", &huge_prompt, &cfg);
        assert_eq!(ctx, 8192);
    }

    #[test]
    fn adaptive_ctx_rounds_up_to_step() {
        let cfg = LLMConfig {
            context_window: 32768,
            max_tokens: 1024,
            ollama_adaptive_context: true,
            ollama_adaptive_context_min: 1024,
            ollama_adaptive_context_max: 32768,
            ollama_chars_per_token: 4,
            ollama_adaptive_headroom_tokens: 512,
            ollama_adaptive_step_tokens: 2048,
            ..LLMConfig::default()
        };

        let prompt = "x".repeat(5_000);
        let ctx = resolve_num_ctx_for_request("mistral:7b", "", &prompt, &cfg);
        assert_eq!(ctx, 4096);
    }

    #[test]
    fn adaptive_ctx_uses_ollama_num_predict_override() {
        let cfg = LLMConfig {
            context_window: 32768,
            max_tokens: 512,
            ollama_num_predict: Some(4096),
            ollama_adaptive_context: true,
            ollama_adaptive_context_min: 1024,
            ollama_adaptive_context_max: 32768,
            ollama_chars_per_token: 4,
            ollama_adaptive_headroom_tokens: 0,
            ollama_adaptive_step_tokens: 1,
            ..LLMConfig::default()
        };

        let prompt = "x".repeat(400);
        let ctx = resolve_num_ctx_for_request("mistral:7b", "", &prompt, &cfg);
        // token_estimator + serialized request labels add base overhead.
        assert_eq!(ctx, 4250);
    }

    #[test]
    fn retry_delay_backoff_scales_with_attempt() {
        let d1 = retry_delay_with_backoff_and_jitter(100, 1).as_millis() as u64;
        let d2 = retry_delay_with_backoff_and_jitter(100, 2).as_millis() as u64;
        let d3 = retry_delay_with_backoff_and_jitter(100, 3).as_millis() as u64;
        assert!(d2 > d1);
        assert!(d3 > d2);
    }

    #[test]
    fn retry_delay_backoff_caps_growth() {
        let d = retry_delay_with_backoff_and_jitter(2_000, 10).as_millis() as u64;
        // 15s cap + <=20% jitter.
        assert!(d >= 15_000);
        assert!(d <= 18_000);
    }

    #[tokio::test]
    async fn request_limiter_defaults_to_safe_cap() {
        let cfg = LLMConfig {
            max_parallels: 16,
            ollama_max_in_flight: None,
            ..LLMConfig::default()
        };
        let client = OllamaNativeClient::from_config(&cfg).expect("client should initialize");
        assert_eq!(client.request_limiter.available_permits(), 3);
    }

    #[tokio::test]
    async fn request_limiter_respects_explicit_override() {
        let cfg = LLMConfig {
            max_parallels: 16,
            ollama_max_in_flight: Some(5),
            ..LLMConfig::default()
        };
        let client = OllamaNativeClient::from_config(&cfg).expect("client should initialize");
        assert_eq!(client.request_limiter.available_permits(), 5);
    }
}
