use crate::generator::agent_executor::{AgentExecuteParams, extract};
use crate::{
    generator::{
        context::GeneratorContext,
        preprocess::extractors::language_processors::LanguageProcessorManager,
    },
    types::{
        code::{CodeDossier, CodeInsight},
        project_structure::ProjectStructure,
    },
    utils::{sources::read_dependency_code_source, threads::do_parallel_with_limit},
};
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

/// Files with source_summary shorter than this (bytes) are eligible for batching.
const BATCH_SOURCE_THRESHOLD: usize = 3000;

/// Maximum total source bytes in a single batch prompt.
/// Conservative target: ~50K chars ≈ 12.5K tokens, leaving room for prompt
/// overhead and LLM response within a 32K context window.
const MAX_BATCH_SOURCE_BYTES: usize = 50_000;

/// Minimum number of batchable files required to activate batching.
/// If fewer files qualify, they are processed individually (no batching overhead).
const MIN_BATCH_FILES: usize = 3;

/// Wrapper type for LLM batch extraction of multiple CodeInsights.
#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
struct BatchedCodeInsights {
    /// Array of code insight analyses, one per file in the batch.
    #[serde(default)]
    pub insights: Vec<CodeInsight>,
}

pub struct CodeAnalyze {
    language_processor: LanguageProcessorManager,
}

impl CodeAnalyze {
    pub fn new() -> Self {
        Self {
            language_processor: LanguageProcessorManager::new(),
        }
    }

    pub async fn execute(
        &self,
        context: &GeneratorContext,
        codes: &Vec<CodeDossier>,
        project_structure: &ProjectStructure,
    ) -> Result<Vec<CodeInsight>> {
        let max_parallels = context.config.llm.max_parallels;

        // Phase 1: Run static analysis for ALL files upfront (no LLM calls, fast)
        let mut static_analyses: Vec<(CodeDossier, CodeInsight)> = Vec::with_capacity(codes.len());
        for code in codes {
            let analysis = self.analyze_code_by_rules(code, project_structure).await?;
            static_analyses.push((code.clone(), analysis));
        }

        // Phase 2: Partition into batchable (small) and individual (large) files
        let mut batch_eligible: Vec<(CodeDossier, CodeInsight)> = Vec::new();
        let mut individual: Vec<(CodeDossier, CodeInsight)> = Vec::new();

        for (code, analysis) in static_analyses {
            if code.source_summary.len() < BATCH_SOURCE_THRESHOLD {
                batch_eligible.push((code, analysis));
            } else {
                individual.push((code, analysis));
            }
        }

        // Only batch if enough files qualify; otherwise process all individually
        if batch_eligible.len() < MIN_BATCH_FILES {
            individual.extend(batch_eligible);
            batch_eligible = Vec::new();
        }

        let batch_count = batch_eligible.len();
        let individual_count = individual.len();

        // Phase 3: Group small files into batches
        let batches = Self::group_into_batches(batch_eligible);
        let num_batches = batches.len();

        if batch_count > 0 {
            println!(
                "   \u{1f4e6} Smart batching: {} small files \u{2192} {} batch calls, {} large files \u{2192} {} individual calls",
                batch_count, num_batches, individual_count, individual_count
            );
        }

        // Phase 4: Create futures for both batch and individual analyses
        // Use trait objects since batch and individual closures have different concrete types
        type AnalysisFuture = Pin<Box<dyn Future<Output = Result<Vec<CodeInsight>>> + Send>>;
        let mut all_futures: Vec<AnalysisFuture> = Vec::new();

        // Batch futures — each batch produces multiple CodeInsights
        for batch in batches {
            let context_clone = context.clone();
            let project_structure_clone = project_structure.clone();
            let language_processor = self.language_processor.clone();

            all_futures.push(Box::pin(async move {
                let code_analyze = CodeAnalyze { language_processor };
                let original_codes: Vec<CodeDossier> =
                    batch.iter().map(|(c, _)| c.clone()).collect();
                let analyses: Vec<&CodeInsight> = batch.iter().map(|(_, a)| a).collect();

                let agent_params = code_analyze
                    .prepare_batch_code_agent_params(&project_structure_clone, &analyses)?;

                let batch_result =
                    extract::<BatchedCodeInsights>(&context_clone, agent_params).await?;

                // Match results back to original codes and restore source_summary
                let mut insights = Vec::new();
                for (i, mut insight) in batch_result.insights.into_iter().enumerate() {
                    if let Some(original) = original_codes.get(i) {
                        insight.code_dossier.source_summary = original.source_summary.clone();
                        // Preserve file_path from original (LLM may mangle it)
                        insight.code_dossier.file_path = original.file_path.clone();
                    }
                    insights.push(insight);
                }

                // If LLM returned fewer results than expected, fill with static analyses
                if insights.len() < original_codes.len() {
                    for (code, analysis) in batch.into_iter().skip(insights.len()) {
                        let mut fallback = analysis;
                        fallback.code_dossier.source_summary = code.source_summary.clone();
                        insights.push(fallback);
                    }
                }

                Result::<Vec<CodeInsight>>::Ok(insights)
            }));
        }

        // Individual futures — full single-file analysis with dependency code
        for (code, analysis) in individual {
            let context_clone = context.clone();
            let project_structure_clone = project_structure.clone();
            let language_processor = self.language_processor.clone();

            all_futures.push(Box::pin(async move {
                let code_analyze = CodeAnalyze { language_processor };
                let agent_params = code_analyze
                    .prepare_single_code_agent_params_from_analysis(
                        &project_structure_clone,
                        &analysis,
                    );
                let mut code_insight =
                    extract::<CodeInsight>(&context_clone, agent_params).await?;

                // LLM will rewrite source_summary, so exclude it and override here
                code_insight.code_dossier.source_summary = code.source_summary.to_owned();

                Result::<Vec<CodeInsight>>::Ok(vec![code_insight])
            }));
        }

        // Phase 5: Execute all with concurrency control
        let results = do_parallel_with_limit(all_futures, max_parallels).await;

        // Phase 6: Flatten results
        let mut code_insights = Vec::new();
        for result in results {
            match result {
                Ok(insights) => code_insights.extend(insights),
                Err(e) => {
                    eprintln!("\u{274c} Code analysis failed: {}", e);
                    return Err(e);
                }
            }
        }

        println!(
            "\u{2713} Concurrent code analysis completed, successfully analyzed {} files",
            code_insights.len()
        );
        Ok(code_insights)
    }

    /// Group batch-eligible files into sized batches based on source byte budget.
    fn group_into_batches(
        files: Vec<(CodeDossier, CodeInsight)>,
    ) -> Vec<Vec<(CodeDossier, CodeInsight)>> {
        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        let mut current_bytes = 0usize;

        for item in files {
            let source_bytes = item.0.source_summary.len();

            // If adding this file would exceed budget, start a new batch
            if !current_batch.is_empty() && current_bytes + source_bytes > MAX_BATCH_SOURCE_BYTES {
                batches.push(std::mem::take(&mut current_batch));
                current_bytes = 0;
            }

            current_bytes += source_bytes;
            current_batch.push(item);
        }

        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }
}

impl CodeAnalyze {
    /// Build agent params for a single file from pre-computed static analysis.
    /// Includes dependency code snippets for full context.
    fn prepare_single_code_agent_params_from_analysis(
        &self,
        project_structure: &ProjectStructure,
        analysis: &CodeInsight,
    ) -> AgentExecuteParams {
        let prompt_user = self.build_code_analysis_prompt(project_structure, analysis);
        let prompt_sys = include_str!("prompts/code_analyze_sys.tpl").to_string();

        AgentExecuteParams {
            prompt_sys,
            prompt_user,
            cache_scope: "ai_code_insight".to_string(),
            log_tag: analysis.code_dossier.name.to_string(),
        }
    }

    /// Build agent params for a batch of small files.
    /// Omits dependency code snippets to keep prompt within budget.
    fn prepare_batch_code_agent_params(
        &self,
        _project_structure: &ProjectStructure,
        analyses: &[&CodeInsight],
    ) -> Result<AgentExecuteParams> {
        let prompt_sys = include_str!("prompts/code_analyze_sys.tpl").to_string();

        let mut files_section = String::new();
        for (i, analysis) in analyses.iter().enumerate() {
            use std::fmt::Write;
            write!(
                files_section,
                "\n### File {} of {}\n\
                 - Component Name: {}\n\
                 - File Path: {}\n\
                 - Component Type: {}\n\
                 - Importance Score: {:.2}\n\
                 - Responsibilities: {}\n\
                 - Number of Interfaces: {}\n\
                 - Number of Dependencies: {}\n\
                 - Lines of Code: {}\n\
                 - Cyclomatic Complexity: {:.1}\n\
                 \n```\n{}\n```\n",
                i + 1,
                analyses.len(),
                analysis.code_dossier.name,
                analysis.code_dossier.file_path.display(),
                analysis.code_dossier.code_purpose.display_name(),
                analysis.code_dossier.importance_score,
                if analysis.responsibilities.is_empty() {
                    "(none detected)".to_string()
                } else {
                    analysis.responsibilities.join(", ")
                },
                analysis.interfaces.len(),
                analysis.dependencies.len(),
                analysis.complexity_metrics.lines_of_code,
                analysis.complexity_metrics.cyclomatic_complexity,
                analysis.code_dossier.source_summary,
            )?;
        }

        let prompt_user = format!(
            "Please conduct an in-depth analysis of the following {} code files. \
             Return your analysis as a JSON object with an \"insights\" array containing \
             one CodeInsight object per file, in the same order as presented below.\n\
             \n## Files to Analyze\n{}\n\
             \nFor each file, provide:\n\
             1. Detailed functional description and business logic\n\
             2. Core responsibilities (3-5 items)\n\
             3. Role in the system architecture\n\
             4. Interface and dependency analysis\n\
             \nReturn exactly {} insight objects in the \"insights\" array, \
             one for each file above, maintaining the same order.\n\
             \nThe analysis should be based on actual code content and provide specific \
             and actionable insights.",
            analyses.len(),
            files_section,
            analyses.len(),
        );

        // Build a truncated log tag showing batch contents
        let file_names: Vec<&str> = analyses
            .iter()
            .map(|a| a.code_dossier.name.as_str())
            .collect();
        let log_tag = {
            let full = format!("batch[{}files:{}]", analyses.len(), file_names.join(","));
            if full.len() > 80 {
                format!("{}...", &full[..77])
            } else {
                full
            }
        };

        Ok(AgentExecuteParams {
            prompt_sys,
            prompt_user,
            cache_scope: "ai_code_insight_batch".to_string(),
            log_tag,
        })
    }
}

impl CodeAnalyze {
    fn build_code_analysis_prompt(
        &self,
        project_structure: &ProjectStructure,
        analysis: &CodeInsight,
    ) -> String {
        let project_path = &project_structure.root_path;

        // Read source code snippets of dependency components
        let dependency_code =
            read_dependency_code_source(&self.language_processor, analysis, project_path);

        format!(
            include_str!("prompts/code_analyze_user.tpl"),
            analysis.code_dossier.name,
            analysis.code_dossier.file_path.display(),
            analysis.code_dossier.code_purpose.display_name(),
            analysis.code_dossier.importance_score,
            analysis.responsibilities.join(", "),
            analysis.interfaces.len(),
            analysis.dependencies.len(),
            analysis.complexity_metrics.lines_of_code,
            analysis.complexity_metrics.cyclomatic_complexity,
            analysis.code_dossier.source_summary,
            dependency_code
        )
    }

    async fn analyze_code_by_rules(
        &self,
        code: &CodeDossier,
        project_structure: &ProjectStructure,
    ) -> Result<CodeInsight> {
        let full_path = project_structure.root_path.join(&code.file_path);

        // Read file content
        let content = if full_path.exists() {
            tokio::fs::read_to_string(&full_path).await?
        } else {
            String::new()
        };

        // Analyze interfaces
        let interfaces = self
            .language_processor
            .extract_interfaces(&code.file_path, &content);

        // Analyze dependencies
        let dependencies = self
            .language_processor
            .extract_dependencies(&code.file_path, &content);

        // Calculate complexity metrics
        let complexity_metrics = self
            .language_processor
            .calculate_complexity_metrics(&content);

        Ok(CodeInsight {
            code_dossier: code.clone(),
            detailed_description: format!("Detailed analysis of {}", code.name),
            interfaces,
            dependencies,
            complexity_metrics,
            responsibilities: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::code::{CodeComplexity, CodePurpose};
    use std::path::PathBuf;

    fn make_dossier(name: &str, source_len: usize) -> CodeDossier {
        CodeDossier {
            name: name.to_string(),
            file_path: PathBuf::from(format!("src/{}.rs", name)),
            source_summary: "x".repeat(source_len),
            code_purpose: CodePurpose::Other,
            importance_score: 0.5,
            description: None,
            functions: vec![],
            interfaces: vec![],
        }
    }

    fn make_pair(name: &str, source_len: usize) -> (CodeDossier, CodeInsight) {
        let dossier = make_dossier(name, source_len);
        let insight = CodeInsight {
            code_dossier: dossier.clone(),
            detailed_description: String::new(),
            responsibilities: vec![],
            interfaces: vec![],
            dependencies: vec![],
            complexity_metrics: CodeComplexity::default(),
        };
        (dossier, insight)
    }

    #[test]
    fn test_group_into_batches_single_batch() {
        let files = vec![
            make_pair("a", 1000),
            make_pair("b", 1000),
            make_pair("c", 1000),
        ];
        let batches = CodeAnalyze::group_into_batches(files);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[test]
    fn test_group_into_batches_multiple_batches() {
        // Each file is 30K, MAX_BATCH_SOURCE_BYTES is 50K, so max 1 per batch after first
        let files = vec![
            make_pair("a", 30_000),
            make_pair("b", 30_000),
            make_pair("c", 30_000),
        ];
        let batches = CodeAnalyze::group_into_batches(files);
        // First batch: a (30K), can't add b (60K > 50K)
        // Second batch: b (30K), can't add c (60K > 50K)
        // Third batch: c (30K)
        assert_eq!(batches.len(), 3);
    }

    #[test]
    fn test_group_into_batches_empty() {
        let files: Vec<(CodeDossier, CodeInsight)> = vec![];
        let batches = CodeAnalyze::group_into_batches(files);
        assert!(batches.is_empty());
    }

    #[test]
    fn test_group_into_batches_fits_exactly() {
        // Two files that together exactly reach the limit
        let files = vec![
            make_pair("a", 25_000),
            make_pair("b", 25_000),
        ];
        let batches = CodeAnalyze::group_into_batches(files);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 2);
    }

    #[test]
    fn test_batch_threshold_partitioning() {
        // Files below threshold should be batch-eligible
        let small = make_dossier("small", BATCH_SOURCE_THRESHOLD - 1);
        let large = make_dossier("large", BATCH_SOURCE_THRESHOLD);

        assert!(small.source_summary.len() < BATCH_SOURCE_THRESHOLD);
        assert!(large.source_summary.len() >= BATCH_SOURCE_THRESHOLD);
    }
}
