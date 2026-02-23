use serde::{Deserialize, Serialize};

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Number of input tokens
    pub input_tokens: usize,
    /// Number of output tokens
    pub output_tokens: usize,
    /// Total number of tokens
    pub total_tokens: usize,
}

impl TokenUsage {
    pub fn new(input_tokens: usize, output_tokens: usize) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
        }
    }

    /// Estimate cost (based on different model pricing)
    pub fn estimate_cost(&self, _model_name: &str) -> f64 {
        let (input_cost_per_1k, output_cost_per_1k) = (0.00025, 0.002);

        (self.input_tokens as f64 / 1000.0) * input_cost_per_1k
            + (self.output_tokens as f64 / 1000.0) * output_cost_per_1k
    }
}
