use anyhow::Result;

use crate::generator::agent_executor::{AgentExecuteParams, extract};
use crate::types::code::CodeInsight;
use crate::{
    generator::context::GeneratorContext,
    types::{code_releationship::RelationshipAnalysis, project_structure::ProjectStructure},
    utils::prompt_compressor::{CompressionConfig, PromptCompressor},
};

pub struct RelationshipsAnalyze {
    prompt_compressor: PromptCompressor,
}

impl RelationshipsAnalyze {
    pub fn new() -> Self {
        Self {
            prompt_compressor: PromptCompressor::new(CompressionConfig::default()),
        }
    }

    pub async fn execute(
        &self,
        context: &GeneratorContext,
        code_insights: &Vec<CodeInsight>,
        _project_structure: &ProjectStructure,
    ) -> Result<RelationshipAnalysis> {
        let agent_params = self
            .build_optimized_analysis_params(context, code_insights)
            .await?;
        extract::<RelationshipAnalysis>(context, agent_params).await
    }

    /// Build optimized analysis parameters, supports intelligent compression
    async fn build_optimized_analysis_params(
        &self,
        context: &GeneratorContext,
        code_insights: &[CodeInsight],
    ) -> Result<AgentExecuteParams> {
        let prompt_sys = "You are a professional software architecture analyst specializing in analyzing project-level code dependency relationship graphs. Based on the provided code insights and dependencies, generate an overall architectural relationship analysis for the project.".to_string();

        // Sort by importance and intelligently select
        let mut sorted_insights: Vec<_> = code_insights.iter().collect();
        sorted_insights.sort_by(|a, b| {
            b.code_dossier
                .importance_score
                .partial_cmp(&a.code_dossier.importance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Build code insights content
        let insights_content = self.build_insights_content(&sorted_insights);

        let compression_result = self
            .prompt_compressor
            .compress_if_needed(context, &insights_content, "Code Insights")
            .await?;

        if compression_result.was_compressed {
            println!(
                "   ✅ Compression complete: {} -> {} tokens",
                compression_result.original_tokens, compression_result.compressed_tokens
            );
        }
        let compressed_insights = compression_result.compressed_content;

        let prompt_user = format!(
            "Please analyze the overall architectural relationship graph of the project based on the following code insights and dependencies:

## Core Code Insights
{}

## Analysis Requirements:
Generate a project-level dependency relationship graph, focusing on:
1. Dependencies between core modules
2. Key data flows
3. Architectural hierarchy
4. Potential circular dependencies",
            compressed_insights
        );

        Ok(AgentExecuteParams {
            prompt_sys,
            prompt_user,
            cache_scope: "ai_relationships_insights".to_string(),
            log_tag: "Dependency Relationship Analysis".to_string(),
        })
    }

    /// Build code insights content
    fn build_insights_content(&self, sorted_insights: &[&CodeInsight]) -> String {
        sorted_insights
            .iter()
            .filter(|insight| insight.code_dossier.importance_score >= 0.6)
            .take(150) // Increase quantity limit
            .map(|insight| {
                let dependencies_introduce = insight
                    .dependencies
                    .iter()
                    .take(20) // Limit number of dependencies per file
                    .map(|r| format!("{}({})", r.name, r.dependency_type))
                    .collect::<Vec<_>>()
                    .join(", ");

                format!(
                    "- {}: {} (path: `{}`, importance: {:.2}, complexity: {:.1}, dependencies: [{}])",
                    insight.code_dossier.name,
                    insight.code_dossier.code_purpose.display_name(),
                    insight.code_dossier.file_path.to_string_lossy(),
                    insight.code_dossier.importance_score,
                    insight.complexity_metrics.cyclomatic_complexity,
                    dependencies_introduce
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
