use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::generator::context::GeneratorContext;
use crate::generator::step_forward_agent::ModelPreference;
use crate::llm::client::utils::{estimate_token_usage, resolve_model_for_agent};

pub struct AgentExecuteParams {
    pub prompt_sys: String,
    pub prompt_user: String,
    pub cache_scope: String,
    pub log_tag: String,
    /// Model routing preference — controls which model size is selected for
    /// this agent's LLM calls. Defaults to `ModelPreference::Auto`.
    pub model_preference: ModelPreference,
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

    let prompt_key = format!("{}|{}|reply-prompt", prompt_sys, prompt_user);
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

    let (primary_model, _fallback) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );

    let reply = context
        .llm_client
        .prompt_without_react_with_model(&primary_model, prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;

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

    let prompt_key = format!("{}|{}|reply-prompt+tool", prompt_sys, prompt_user);
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

    // prompt (with tools) always uses the powerful model for compose agents;
    // for Auto the preference resolver already chose the right model based on
    // prompt size.  We ignore the fallback here — the LLMClient's internal
    // fallback chain handles that.
    let (primary_model, _fallback) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );

    let reply = context
        .llm_client
        .prompt_with_model(&primary_model, prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;

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

    let prompt_key = format!("{}|{}", prompt_sys, prompt_user);
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

    let (primary_model, fallback_model) = resolve_model_for_agent(
        &context.config.llm,
        params.model_preference,
        prompt_sys,
        prompt_user,
    );

    let reply = context
        .llm_client
        .extract_with_models::<T>(prompt_sys, prompt_user, primary_model, fallback_model)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;

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

    let prompt_key = format!("{}|{}|reply-prompt", prompt_sys, prompt_user);
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

    let reply = context
        .llm_client
        .prompt_without_react(prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;

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

    let prompt_key = format!("{}|{}", prompt_sys, prompt_user);
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

    let reply = context
        .llm_client
        .extract::<T>(prompt_sys, prompt_user)
        .await
        .map_err(|e| anyhow::anyhow!("AI analysis failed: {}", e))?;

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
