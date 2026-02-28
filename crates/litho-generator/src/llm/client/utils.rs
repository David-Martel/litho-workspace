use crate::{
    config::LLMConfig, generator::step_forward_agent::ModelPreference,
    llm::client::types::TokenUsage, utils::token_estimator::TokenEstimator,
};

use std::sync::LazyLock;

static TOKEN_ESTIMATOR: LazyLock<TokenEstimator> = LazyLock::new(TokenEstimator::new);

pub fn evaluate_befitting_model(
    llm_config: &LLMConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> (String, Option<String>) {
    if system_prompt.len() + user_prompt.len() <= 32 * 1024 {
        return (
            llm_config.model_efficient.clone(),
            Some(llm_config.model_powerful.clone()),
        );
    }
    (llm_config.model_powerful.clone(), None)
}

/// Resolve the model to use based on agent preference and config.
///
/// Returns `(primary_model, fallback_model)` where the fallback is `None` when
/// both models are identical or when the powerful model has no separate fallback.
///
/// # Examples
///
/// ```
/// use litho_generator::config::LLMConfig;
/// use litho_generator::generator::step_forward_agent::ModelPreference;
/// use litho_generator::llm::client::utils::resolve_model_for_agent;
///
/// let config = LLMConfig {
///     model_efficient: "small:7b".to_string(),
///     model_powerful: "large:70b".to_string(),
///     ..Default::default()
/// };
/// let (primary, fallback) = resolve_model_for_agent(&config, ModelPreference::Efficient, "", "");
/// assert_eq!(primary, "small:7b");
/// assert_eq!(fallback, Some("large:70b".to_string()));
/// ```
pub fn resolve_model_for_agent(
    llm_config: &LLMConfig,
    preference: ModelPreference,
    system_prompt: &str,
    user_prompt: &str,
) -> (String, Option<String>) {
    match preference {
        ModelPreference::Efficient => {
            // Always use efficient model, fallback to powerful.
            let primary = if llm_config.model_efficient.is_empty() {
                llm_config.model_powerful.clone()
            } else {
                llm_config.model_efficient.clone()
            };
            let fallback =
                if primary != llm_config.model_powerful && !llm_config.model_powerful.is_empty() {
                    Some(llm_config.model_powerful.clone())
                } else {
                    None
                };
            (primary, fallback)
        }
        ModelPreference::Powerful => {
            // Always use powerful model, fallback to efficient.
            let primary = if llm_config.model_powerful.is_empty() {
                llm_config.model_efficient.clone()
            } else {
                llm_config.model_powerful.clone()
            };
            let fallback = if primary != llm_config.model_efficient
                && !llm_config.model_efficient.is_empty()
            {
                Some(llm_config.model_efficient.clone())
            } else {
                None
            };
            (primary, fallback)
        }
        ModelPreference::Auto => {
            // Existing size-based heuristic.
            evaluate_befitting_model(llm_config, system_prompt, user_prompt)
        }
    }
}

/// Estimate token usage (based on text length)
pub fn estimate_token_usage(input_text: &str, output_text: &str) -> TokenUsage {
    // Rough estimate: 1 token ≈ 4 characters (English) or ~1.5 characters (Chinese)
    let input_estimate = TOKEN_ESTIMATOR.estimate_tokens(input_text);
    let output_estimate = TOKEN_ESTIMATOR.estimate_tokens(output_text);
    TokenUsage::new(
        input_estimate.estimated_tokens,
        output_estimate.estimated_tokens,
    )
}
