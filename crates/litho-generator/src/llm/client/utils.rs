use crate::{
    config::LLMConfig, llm::client::types::TokenUsage, utils::token_estimator::TokenEstimator,
};

use std::sync::LazyLock;

static TOKEN_ESTIMATOR: LazyLock<TokenEstimator> = LazyLock::new(|| TokenEstimator::new());

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
    return (llm_config.model_powerful.clone(), None);
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
