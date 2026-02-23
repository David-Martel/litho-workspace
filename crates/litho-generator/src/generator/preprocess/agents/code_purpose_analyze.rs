use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::{
    types::code::{CodePurpose, CodePurposeMapper},
};
use crate::generator::agent_executor::{AgentExecuteParams, extract};
use crate::generator::context::GeneratorContext;

/// AI component type analysis result
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct AICodePurposeAnalysis {
    // Inferred code functionality classification
    pub code_purpose: CodePurpose,
    // Confidence of the inference result (min 0.0, max 1.0), confidence is high when > 0.7.
    pub confidence: f64,
    pub reasoning: String,
}

/// Component type enhancer, combining rules and AI analysis
pub struct CodePurposeEnhancer;

impl CodePurposeEnhancer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn execute(
        &self,
        context: &GeneratorContext,
        file_path: &Path,
        file_name: &str,
        file_content: &str) -> Result<CodePurpose>
    {
        // First use rule mapping
        let rule_based_type =
            CodePurposeMapper::map_by_path_and_name(&file_path.to_string_lossy(), file_name);

        // If rule mapping gets explicit type with high confidence, return directly
        if rule_based_type != CodePurpose::Other {
            return Ok(rule_based_type);
        }

        // If there's AI analyzer and file content, use AI enhanced analysis
        let prompt_sys = "You are a professional code architecture analyst specializing in analyzing component types of code files.".to_string();
        let prompt_user = self.build_code_purpose_analysis_prompt(file_path, file_content, file_name);

        let analyze_result = extract::<AICodePurposeAnalysis>(context, AgentExecuteParams {
            prompt_sys,
            prompt_user,
            cache_scope: "ai_code_purpose".to_string(),
            log_tag: file_name.to_string(),
        }).await;

        return match analyze_result {
            Ok(ai_analysis) => {
                // If AI analysis confidence is high, use AI result
                if ai_analysis.confidence > 0.7 {
                    return Ok(ai_analysis.code_purpose);
                }
                // Otherwise combine rule and AI results
                if rule_based_type != CodePurpose::Other {
                    Ok(rule_based_type)
                } else {
                    Ok(ai_analysis.code_purpose)
                }
            }
            Err(_) => {
                // AI analysis failed, use rule result
                Ok(rule_based_type)
            }
        }
    }

    /// Build component type analysis prompt
    fn build_code_purpose_analysis_prompt(
        &self,
        file_path: &Path,
        file_content: &str,
        file_name: &str,
    ) -> String {
        // Safely truncate first 1000 characters of file content for analysis
        let content_preview = if file_content.chars().count() > 1000 {
            let truncated: String = file_content.chars().take(1000).collect();
            format!("{}...", truncated)
        } else {
            file_content.to_string()
        };

        format!(
            include_str!("prompts/code_purpose_analyze_user.tpl"),
            file_path.display(),
            file_name,
            content_preview
        )
    }
}
