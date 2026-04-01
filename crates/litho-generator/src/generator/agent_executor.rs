use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};
use std::time::{Instant as StdInstant, SystemTime};

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::config::LLMConfig;
use crate::generator::context::GeneratorContext;
use crate::generator::step_forward_agent::ModelPreference;
use crate::generator::workflow::{TimingKeys, TimingScope};
use crate::llm::client::utils::{estimate_token_usage, resolve_model_for_agent};

static IN_FLIGHT_PROMPT_LOCKS: LazyLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct AgentExecuteParams {
    pub prompt_sys: String,
    pub prompt_user: String,
    pub cache_scope: String,
    pub log_tag: String,
    /// Model routing preference — controls which model size is selected for
    /// this agent's LLM calls. Defaults to `ModelPreference::Auto`.
    pub model_preference: ModelPreference,
}

async fn record_first_llm_response_timing(
    context: &GeneratorContext,
    prompt_started: StdInstant,
) -> Result<()> {
    if context
        .has_memory_data(TimingScope::TIMING, TimingKeys::FIRST_LLM_RESPONSE)
        .await
    {
        return Ok(());
    }

    let elapsed_seconds = if let Some(start_unix) = context
        .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::PIPELINE_START_UNIX)
        .await
    {
        let now_unix = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        (now_unix - start_unix).max(0.0)
    } else {
        prompt_started.elapsed().as_secs_f64()
    };

    context
        .store_to_memory(
            TimingScope::TIMING,
            TimingKeys::FIRST_LLM_RESPONSE,
            elapsed_seconds,
        )
        .await?;
    Ok(())
}

fn llm_cache_signature(
    llm: &LLMConfig,
    primary_model: Option<&str>,
    fallback_model: Option<&str>,
) -> String {
    format!(
        concat!(
            "provider={:?};base={};effective={};fallback={};",
            "efficient={};powerful={};ctx={};max_tokens={};temp={:?};",
            "num_predict={:?};top_p={:?};top_k={:?};repeat_penalty={:?};",
            "repeat_last_n={:?};tfs_z={:?};seed={:?};"
        ),
        llm.provider,
        llm.api_base_url,
        primary_model.unwrap_or_default(),
        fallback_model.unwrap_or_default(),
        llm.model_efficient,
        llm.model_powerful,
        llm.context_window,
        llm.max_tokens,
        llm.temperature,
        llm.ollama_num_predict,
        llm.ollama_top_p,
        llm.ollama_top_k,
        llm.ollama_repeat_penalty,
        llm.ollama_repeat_last_n,
        llm.ollama_tfs_z,
        llm.ollama_seed
    )
}

fn coalesce_prompt_key(cache_scope: &str, prompt_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prompt_key.hash(&mut hasher);
    format!("{cache_scope}:{:016x}", hasher.finish())
}

async fn acquire_in_flight_prompt_lock(coalesce_key: &str) -> Arc<Mutex<()>> {
    let mut locks = IN_FLIGHT_PROMPT_LOCKS.lock().await;
    locks
        .entry(coalesce_key.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

// ---------------------------------------------------------------------------
// Preference-aware executor functions (primary entry points from the trait)
// ---------------------------------------------------------------------------

/// Prompt (no tools) using the model selected by the agent's `ModelPreference`.
pub async fn prompt_with_preference(
    context: &GeneratorContext,
    params: AgentExecuteParams,
) -> Result<String> {
    let prompt_sys = &params.prompt_sys;
    let prompt_user = &params.prompt_user;
    let cache_scope = &params.cache_scope;
    let log_tag = &params.log_tag;

    let (primary_model, _fallback) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );
    let signature = llm_cache_signature(&context.config.llm, Some(&primary_model), None);
    let prompt_key = format!("{}|{}|reply-prompt|{}", prompt_sys, prompt_user, signature);
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let lock_key = coalesce_prompt_key(cache_scope, &prompt_key);
    let lock = acquire_in_flight_prompt_lock(&lock_key).await;
    let _guard = lock.lock().await;
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let msg = context
        .config
        .target_language
        .msg_ai_analyzing()
        .replace("{}", log_tag);
    println!("{}", msg);

    let call_start = StdInstant::now();
    let reply = context
        .llm_client
        .prompt_without_react_with_model(&primary_model, prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;
    let _ = record_first_llm_response_timing(context, call_start).await;

    let input_text = format!("{} {}", prompt_sys, prompt_user);
    let token_usage = estimate_token_usage(&input_text, &reply);

    context
        .cache_manager
        .write()
        .await
        .set_with_tokens(cache_scope, &prompt_key, &reply, token_usage)
        .await?;

    Ok(reply)
}

/// Prompt with tools using the model selected by the agent's `ModelPreference`.
pub async fn prompt_with_tools_with_preference(
    context: &GeneratorContext,
    params: AgentExecuteParams,
) -> Result<String> {
    let prompt_sys = &params.prompt_sys;
    let prompt_user = &params.prompt_user;
    let cache_scope = &params.cache_scope;
    let log_tag = &params.log_tag;

    let (primary_model, _fallback) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );
    let signature = llm_cache_signature(&context.config.llm, Some(&primary_model), None);
    let prompt_key = format!(
        "{}|{}|reply-prompt+tool|{}",
        prompt_sys, prompt_user, signature
    );
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let lock_key = coalesce_prompt_key(cache_scope, &prompt_key);
    let lock = acquire_in_flight_prompt_lock(&lock_key).await;
    let _guard = lock.lock().await;
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let msg = context
        .config
        .target_language
        .msg_ai_analyzing()
        .replace("{}", log_tag);
    println!("{}", msg);

    let call_start = StdInstant::now();
    let reply = context
        .llm_client
        .prompt_with_model(&primary_model, prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;
    let _ = record_first_llm_response_timing(context, call_start).await;

    let input_text = format!("{} {}", prompt_sys, prompt_user);
    let output_text = serde_json::to_string(&reply).unwrap_or_default();
    let token_usage = estimate_token_usage(&input_text, &output_text);

    context
        .cache_manager
        .write()
        .await
        .set_with_tokens(cache_scope, &prompt_key, &reply, token_usage)
        .await?;

    Ok(reply)
}

/// Extract structured data using the model selected by the agent's `ModelPreference`.
pub async fn extract_with_preference<T>(
    context: &GeneratorContext,
    params: AgentExecuteParams,
) -> Result<T>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let prompt_sys = &params.prompt_sys;
    let prompt_user = &params.prompt_user;
    let cache_scope = &params.cache_scope;
    let log_tag = &params.log_tag;

    let (primary_model, fallback_model) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );
    let signature = llm_cache_signature(
        &context.config.llm,
        Some(&primary_model),
        fallback_model.as_deref(),
    );
    let prompt_key = format!("{}|{}|extract|{}", prompt_sys, prompt_user, signature);
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<T>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply);
    }

    let lock_key = coalesce_prompt_key(cache_scope, &prompt_key);
    let lock = acquire_in_flight_prompt_lock(&lock_key).await;
    let _guard = lock.lock().await;
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<T>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply);
    }

    let msg = context
        .config
        .target_language
        .msg_ai_analyzing()
        .replace("{}", log_tag);
    println!("{}", msg);

    let call_start = StdInstant::now();
    let reply = context
        .llm_client
        .extract_with_models::<T>(prompt_sys, prompt_user, primary_model, fallback_model)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;
    let _ = record_first_llm_response_timing(context, call_start).await;

    let input_text = format!("{} {}", prompt_sys, prompt_user);
    let output_text = serde_json::to_string(&reply).unwrap_or_default();
    let token_usage = estimate_token_usage(&input_text, &output_text);

    context
        .cache_manager
        .write()
        .await
        .set_with_tokens(cache_scope, &prompt_key, &reply, token_usage)
        .await?;

    Ok(reply)
}

// ---------------------------------------------------------------------------
// Legacy wrappers — kept for direct callers in preprocessing and utilities.
//
// These functions hard-code Auto routing and delegate to the original
// `LLMClient` methods so that existing call sites (preprocessing agents,
// prompt compressor) are not disturbed.
// ---------------------------------------------------------------------------

pub async fn prompt(context: &GeneratorContext, params: AgentExecuteParams) -> Result<String> {
    let prompt_sys = &params.prompt_sys;
    let prompt_user = &params.prompt_user;
    let cache_scope = &params.cache_scope;
    let log_tag = &params.log_tag;

    let signature = llm_cache_signature(&context.config.llm, None, None);
    let prompt_key = format!("{}|{}|reply-prompt|{}", prompt_sys, prompt_user, signature);
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let lock_key = coalesce_prompt_key(cache_scope, &prompt_key);
    let lock = acquire_in_flight_prompt_lock(&lock_key).await;
    let _guard = lock.lock().await;
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<serde_json::Value>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply.to_string());
    }

    let msg = context
        .config
        .target_language
        .msg_ai_analyzing()
        .replace("{}", log_tag);
    println!("{}", msg);

    let call_start = StdInstant::now();
    let reply = context
        .llm_client
        .prompt_without_react(prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;
    let _ = record_first_llm_response_timing(context, call_start).await;

    let input_text = format!("{} {}", prompt_sys, prompt_user);
    let token_usage = estimate_token_usage(&input_text, &reply);

    context
        .cache_manager
        .write()
        .await
        .set_with_tokens(cache_scope, &prompt_key, &reply, token_usage)
        .await?;

    Ok(reply)
}

pub async fn extract<T>(context: &GeneratorContext, params: AgentExecuteParams) -> Result<T>
where
    T: JsonSchema + for<'a> Deserialize<'a> + Serialize + Send + Sync + 'static,
{
    let prompt_sys = &params.prompt_sys;
    let prompt_user = &params.prompt_user;
    let cache_scope = &params.cache_scope;
    let log_tag = &params.log_tag;

    let signature = llm_cache_signature(&context.config.llm, None, None);
    let prompt_key = format!("{}|{}|extract|{}", prompt_sys, prompt_user, signature);
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<T>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply);
    }

    let lock_key = coalesce_prompt_key(cache_scope, &prompt_key);
    let lock = acquire_in_flight_prompt_lock(&lock_key).await;
    let _guard = lock.lock().await;
    if let Some(cached_reply) = context
        .cache_manager
        .read()
        .await
        .get::<T>(cache_scope, &prompt_key)
        .await?
    {
        let msg = context
            .config
            .target_language
            .msg_cache_hit()
            .replace("{}", log_tag);
        println!("{}", msg);
        return Ok(cached_reply);
    }

    let msg = context
        .config
        .target_language
        .msg_ai_analyzing()
        .replace("{}", log_tag);
    println!("{}", msg);

    let call_start = StdInstant::now();
    let reply = context
        .llm_client
        .extract::<T>(prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;
    let _ = record_first_llm_response_timing(context, call_start).await;

    let input_text = format!("{} {}", prompt_sys, prompt_user);
    let output_text = serde_json::to_string(&reply).unwrap_or_default();
    let token_usage = estimate_token_usage(&input_text, &output_text);

    context
        .cache_manager
        .write()
        .await
        .set_with_tokens(cache_scope, &prompt_key, &reply, token_usage)
        .await?;

    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::llm_cache_signature;
    use crate::config::LLMConfig;

    #[test]
    fn llm_cache_signature_changes_with_effective_model() {
        let cfg = LLMConfig::default();
        let a = llm_cache_signature(&cfg, Some("qwen2.5-coder:7b"), Some("gemma3:12b"));
        let b = llm_cache_signature(&cfg, Some("gemma3:12b"), Some("qwen2.5-coder:7b"));
        assert_ne!(a, b);
    }

    #[test]
    fn llm_cache_signature_changes_with_sampling_config() {
        let a_cfg = LLMConfig {
            ollama_top_p: Some(0.85),
            ..LLMConfig::default()
        };
        let b_cfg = LLMConfig {
            ollama_top_p: Some(0.95),
            ..a_cfg.clone()
        };
        let a = llm_cache_signature(&a_cfg, Some("qwen2.5-coder:7b"), Some("gemma3:12b"));
        let b = llm_cache_signature(&b_cfg, Some("qwen2.5-coder:7b"), Some("gemma3:12b"));
        assert_ne!(a, b);
    }
}
