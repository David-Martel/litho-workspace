use anyhow::Result;
use chrono;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

use crate::generator::compose::memory::MemoryScope as ComposeMemoryScope;
use crate::generator::context::GeneratorContext;
use crate::generator::preprocess::memory::{MemoryScope as PreprocessMemoryScope, ScopedKeys};
use crate::generator::research::memory::MemoryScope as ResearchMemoryScope;
use crate::generator::research::types::AgentType as ResearchAgentType;
use crate::generator::workflow::{TimingKeys, TimingScope};

/// Summary data collector - responsible for extracting four types of research materials from context
pub struct SummaryDataCollector;

/// Summary content generator - responsible for formatting and organizing content
pub struct SummaryContentGenerator;

/// Summary generation mode
#[derive(Debug, Clone)]
pub enum SummaryMode {
    /// Full mode - includes all detailed data
    Full,
    /// Brief mode - includes only basic information and core metrics
    Brief,
}

/// Summary data structure
#[derive(Debug)]
pub struct SummaryData {
    /// System context research report
    pub system_context: Option<Value>,
    /// Domain modules research report
    pub domain_modules: Option<Value>,
    /// Workflow research report
    pub workflow: Option<Value>,
    /// Code insights data
    pub code_insights: Option<Value>,
    /// Memory storage statistics
    pub memory_stats: HashMap<String, usize>,
    /// Cache performance statistics
    pub cache_stats: CacheStatsData,
    /// Generated documents list
    pub generated_docs: Vec<String>,
    /// Timing statistics
    pub timing_stats: TimingStats,
}

/// Cache statistics data
#[derive(Debug)]
pub struct CacheStatsData {
    pub hit_rate: f64,
    pub total_operations: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub cache_writes: usize,
    pub cache_errors: usize,
    pub inference_time_saved: f64,
    pub cost_saved: f64,
    pub performance_improvement: f64,
    pub input_tokens_saved: usize,
    pub output_tokens_saved: usize,
}

/// Timing statistics data
#[derive(Debug)]
pub struct TimingStats {
    /// Total execution time (seconds)
    pub total_execution_time: f64,
    /// Preprocessing phase time (seconds)
    pub preprocess_time: f64,
    /// Research phase time (seconds)
    pub research_time: f64,
    /// Document generation phase time (seconds)
    pub compose_time: f64,
    /// Output phase time (seconds)
    pub output_time: f64,
    /// Document generation time
    pub document_generation_time: f64,
    /// Summary generation time
    pub summary_generation_time: f64,
}

impl SummaryDataCollector {
    /// Collect all required data from GeneratorContext
    pub async fn collect_data(context: &GeneratorContext) -> Result<SummaryData> {
        let start_time = Instant::now();

        // Collect four types of research materials
        let system_context = context
            .get_from_memory::<Value>(
                ResearchMemoryScope::STUDIES_RESEARCH,
                &ResearchAgentType::SystemContextResearcher.to_string(),
            )
            .await;

        let domain_modules = context
            .get_from_memory::<Value>(
                ResearchMemoryScope::STUDIES_RESEARCH,
                &ResearchAgentType::DomainModulesDetector.to_string(),
            )
            .await;

        let workflow = context
            .get_from_memory::<Value>(
                ResearchMemoryScope::STUDIES_RESEARCH,
                &ResearchAgentType::WorkflowResearcher.to_string(),
            )
            .await;

        let code_insights = context
            .get_from_memory::<Value>(PreprocessMemoryScope::PREPROCESS, ScopedKeys::CODE_INSIGHTS)
            .await;

        // Collect Memory statistics
        let memory_stats = context.get_memory_stats().await;

        // Collect cache statistics
        let cache_report = context
            .cache_manager
            .read()
            .await
            .generate_performance_report();
        let cache_stats = CacheStatsData {
            hit_rate: cache_report.hit_rate,
            total_operations: cache_report.total_operations,
            cache_hits: cache_report.cache_hits,
            cache_misses: cache_report.cache_misses,
            cache_writes: cache_report.cache_writes,
            cache_errors: cache_report.cache_errors,
            inference_time_saved: cache_report.inference_time_saved,
            cost_saved: cache_report.cost_saved,
            performance_improvement: cache_report.performance_improvement,
            input_tokens_saved: cache_report.input_tokens_saved,
            output_tokens_saved: cache_report.output_tokens_saved,
        };

        // Collect generated documents list
        let generated_docs = context
            .list_memory_keys(ComposeMemoryScope::DOCUMENTATION)
            .await;

        // Collect timing statistics (from various stages in memory, if available)
        let timing_stats = Self::collect_timing_stats(context).await;

        let summary_generation_time = start_time.elapsed().as_secs_f64();
        let mut timing_stats = timing_stats;
        timing_stats.summary_generation_time = summary_generation_time;

        Ok(SummaryData {
            system_context,
            domain_modules,
            workflow,
            code_insights,
            memory_stats,
            cache_stats,
            generated_docs,
            timing_stats,
        })
    }

    /// Collect timing statistics information
    async fn collect_timing_stats(context: &GeneratorContext) -> TimingStats {
        // Try to get timing information for each phase from memory
        let preprocess_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::PREPROCESS)
            .await
            .unwrap_or(0.0);

        let research_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::RESEARCH)
            .await
            .unwrap_or(0.0);

        let compose_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::COMPOSE)
            .await
            .unwrap_or(0.0);

        let output_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::OUTPUT)
            .await
            .unwrap_or(0.0);

        let document_generation_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::DOCUMENT_GENERATION)
            .await
            .unwrap_or(0.0);

        let total_execution_time = context
            .get_from_memory::<f64>(TimingScope::TIMING, TimingKeys::TOTAL_EXECUTION)
            .await
            .unwrap_or(preprocess_time + research_time + compose_time + output_time);

        TimingStats {
            total_execution_time,
            preprocess_time,
            research_time,
            compose_time,
            output_time,
            document_generation_time,
            summary_generation_time: 0.0, // Will be set at call site
        }
    }
}

impl SummaryContentGenerator {
    /// Generate Markdown-formatted summary content based on collected data
    pub fn generate_content(data: &SummaryData, mode: SummaryMode) -> String {
        match mode {
            SummaryMode::Full => Self::generate_full_content(data),
            SummaryMode::Brief => Self::generate_brief_content(data),
        }
    }

    /// Generate full version of summary content
    fn generate_full_content(data: &SummaryData) -> String {
        let mut content = String::new();

        // 1. Basic information
        content.push_str("# Project Analysis Summary Report (Full Version)\n\n");
        content.push_str(&format!(
            "Generation Time: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        // 2. Execution timing statistics
        content.push_str("## Execution Timing Statistics\n\n");
        let timing = &data.timing_stats;
        content.push_str(&format!(
            "- **Total Execution Time**: {:.2} seconds\n",
            timing.total_execution_time
        ));
        content.push_str(&format!(
            "- **Preprocessing Phase**: {:.2} seconds ({:.1}%)\n",
            timing.preprocess_time,
            if timing.total_execution_time > 0.0 {
                (timing.preprocess_time / timing.total_execution_time) * 100.0
            } else {
                0.0
            }
        ));
        content.push_str(&format!(
            "- **Research Phase**: {:.2} seconds ({:.1}%)\n",
            timing.research_time,
            if timing.total_execution_time > 0.0 {
                (timing.research_time / timing.total_execution_time) * 100.0
            } else {
                0.0
            }
        ));
        content.push_str(&format!(
            "- **Document Generation Phase**: {:.2} seconds ({:.1}%)\n",
            timing.compose_time,
            if timing.total_execution_time > 0.0 {
                (timing.compose_time / timing.total_execution_time) * 100.0
            } else {
                0.0
            }
        ));
        content.push_str(&format!(
            "- **Output Phase**: {:.2} seconds ({:.1}%)\n",
            timing.output_time,
            if timing.total_execution_time > 0.0 {
                (timing.output_time / timing.total_execution_time) * 100.0
            } else {
                0.0
            }
        ));
        if timing.document_generation_time > 0.0 {
            content.push_str(&format!(
                "- **Document Generation Time**: {:.2} seconds\n",
                timing.document_generation_time
            ));
        }
        content.push_str(&format!(
            "- **Summary Generation Time**: {:.3} seconds\n\n",
            timing.summary_generation_time
        ));

        // 3. Cache performance statistics and savings
        content.push_str("## Cache Performance Statistics and Savings\n\n");
        let stats = &data.cache_stats;

        content.push_str("### Performance Metrics\n");
        content.push_str(&format!(
            "- **Cache Hit Rate**: {:.1}%\n",
            stats.hit_rate * 100.0
        ));
        content.push_str(&format!("- **Total Operations**: {}\n", stats.total_operations));
        content.push_str(&format!("- **Cache Hits**: {} times\n", stats.cache_hits));
        content.push_str(&format!("- **Cache Misses**: {} times\n", stats.cache_misses));
        content.push_str(&format!("- **Cache Writes**: {} times\n", stats.cache_writes));
        if stats.cache_errors > 0 {
            content.push_str(&format!("- **Cache Errors**: {} times\n", stats.cache_errors));
        }

        content.push_str("\n### Savings\n");
        content.push_str(&format!(
            "- **Inference Time Saved**: {:.1} seconds\n",
            stats.inference_time_saved
        ));
        content.push_str(&format!(
            "- **Tokens Saved**: {} input + {} output = {} total\n",
            stats.input_tokens_saved,
            stats.output_tokens_saved,
            stats.input_tokens_saved + stats.output_tokens_saved
        ));
        content.push_str(&format!("- **Estimated Cost Savings**: ${:.4}\n", stats.cost_saved));
        if stats.performance_improvement > 0.0 {
            content.push_str(&format!(
                "- **Performance Improvement**: {:.1}%\n",
                stats.performance_improvement
            ));
        }

        // Calculate efficiency ratio
        if timing.total_execution_time > 0.0 && stats.inference_time_saved > 0.0 {
            let efficiency_ratio = stats.inference_time_saved / timing.total_execution_time;
            content.push_str(&format!(
                "- **Efficiency Improvement Ratio**: {:.1}x (saved time / actual execution time)\n",
                efficiency_ratio
            ));
        }
        content.push_str("\n");

        // 4. Core research data summary
        content.push_str("## Core Research Data Summary\n\n");
        content.push_str("Complete content of four types of research materials according to Prompt template data integration rules:\n\n");

        // System context research report
        if let Some(ref system_context) = data.system_context {
            content.push_str("### System Context Research Report\n");
            content.push_str("Provides core objectives, user roles, and system boundary information for the project.\n\n");
            content.push_str(&format!(
                "```json\n{}\n```\n\n",
                serde_json::to_string_pretty(system_context).unwrap_or_default()
            ));
        }

        // Domain modules research report
        if let Some(ref domain_modules) = data.domain_modules {
            content.push_str("### Domain Modules Research Report\n");
            content.push_str("Provides high-level domain division, module relationships, and core business process information.\n\n");
            content.push_str(&format!(
                "```json\n{}\n```\n\n",
                serde_json::to_string_pretty(domain_modules).unwrap_or_default()
            ));
        }

        // Workflow research report
        if let Some(ref workflow) = data.workflow {
            content.push_str("### Workflow Research Report\n");
            content.push_str("Contains static analysis results of the codebase and business process analysis.\n\n");
            content.push_str(&format!(
                "```json\n{}\n```\n\n",
                serde_json::to_string_pretty(workflow).unwrap_or_default()
            ));
        }

        // Code insights data
        if let Some(ref code_insights) = data.code_insights {
            content.push_str("### Code Insights Data\n");
            content.push_str("Code analysis results from preprocessing phase, including definitions of functions, classes, and modules.\n\n");
            content.push_str(&format!(
                "```json\n{}\n```\n\n",
                serde_json::to_string_pretty(code_insights).unwrap_or_default()
            ));
        }

        // 5. Memory storage statistics
        content.push_str("## Memory Storage Statistics\n\n");
        if data.memory_stats.is_empty() {
            content.push_str("No Memory storage data available.\n\n");
        } else {
            let total_size: usize = data.memory_stats.values().sum();
            content.push_str(&format!("**Total Storage Size**: {} bytes\n\n", total_size));
            for (scope, size) in &data.memory_stats {
                let percentage = (*size as f64 / total_size as f64) * 100.0;
                content.push_str(&format!(
                    "- **{}**: {} bytes ({:.1}%)\n",
                    scope, size, percentage
                ));
            }
            content.push_str("\n");
        }

        // 6. Generated documents statistics
        content.push_str("## Generated Documents Statistics\n\n");
        content.push_str(&format!(
            "Number of Generated Documents: {}\n\n",
            data.generated_docs.len()
        ));
        for doc in &data.generated_docs {
            content.push_str(&format!("- {}\n", doc));
        }

        content
    }

    /// Generate brief version of summary content
    fn generate_brief_content(data: &SummaryData) -> String {
        let mut content = String::new();

        // 1. Basic information
        content.push_str("# Project Analysis Brief Report\n\n");
        content.push_str(&format!(
            "Generation Time: {}\n\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));

        // 2. Execution overview
        content.push_str("## Execution Overview\n\n");
        let timing = &data.timing_stats;
        content.push_str(&format!(
            "**Total Execution Time**: {:.2} seconds\n",
            timing.total_execution_time
        ));

        // Display most time-consuming phases
        let mut stages = vec![
            ("Preprocessing", timing.preprocess_time),
            ("Research", timing.research_time),
            ("Documentation", timing.compose_time),
            ("Output", timing.output_time),
        ];
        stages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        content.push_str("**Phase Timing**:\n");
        for (stage, time) in stages {
            let percentage = if timing.total_execution_time > 0.0 {
                (time / timing.total_execution_time) * 100.0
            } else {
                0.0
            };
            content.push_str(&format!("- {}: {:.2}s ({:.1}%)\n", stage, time, percentage));
        }
        content.push_str("\n");

        // 3. Cache effectiveness overview
        content.push_str("## Cache Effectiveness Overview\n\n");
        let stats = &data.cache_stats;

        // Core metrics
        content.push_str(&format!("**Cache Hit Rate**: {:.1}% ", stats.hit_rate * 100.0));
        if stats.hit_rate >= 0.8 {
            content.push_str("🟢 Excellent\n");
        } else if stats.hit_rate >= 0.5 {
            content.push_str("🟡 Good\n");
        } else {
            content.push_str("🔴 Needs Optimization\n");
        }

        content.push_str(&format!(
            "**Time Saved**: {:.1} seconds\n",
            stats.inference_time_saved
        ));
        content.push_str(&format!(
            "**Tokens Saved**: {} input + {} output = {} total\n",
            stats.input_tokens_saved,
            stats.output_tokens_saved,
            stats.input_tokens_saved + stats.output_tokens_saved
        ));
        content.push_str(&format!("**Cost Savings**: ${:.4}\n", stats.cost_saved));

        // Efficiency assessment
        if timing.total_execution_time > 0.0 && stats.inference_time_saved > 0.0 {
            let efficiency_ratio = stats.inference_time_saved / timing.total_execution_time;
            content.push_str(&format!("**Efficiency Improvement**: {:.1}x\n", efficiency_ratio));
        }

        // Cost-benefit analysis
        if stats.cost_saved > 0.0 {
            let cost_per_second = stats.cost_saved / timing.total_execution_time;
            content.push_str(&format!("**Cost-Benefit**: ${:.6}/second\n", cost_per_second));
        }
        content.push_str("\n");

        // 4. Research data overview
        content.push_str("## Research Data Overview\n\n");
        content.push_str("Successfully collected four types of research materials according to Prompt template data integration rules:\n\n");

        let mut collected_count = 0;

        // Check if each type of research material exists
        if data.system_context.is_some() {
            content.push_str("✅ **System Context Research Report**: Generated\n");
            collected_count += 1;
        } else {
            content.push_str("❌ **System Context Research Report**: Not generated\n");
        }

        if data.domain_modules.is_some() {
            content.push_str("✅ **Domain Modules Research Report**: Generated\n");
            collected_count += 1;
        } else {
            content.push_str("❌ **Domain Modules Research Report**: Not generated\n");
        }

        if data.workflow.is_some() {
            content.push_str("✅ **Workflow Research Report**: Generated\n");
            collected_count += 1;
        } else {
            content.push_str("❌ **Workflow Research Report**: Not generated\n");
        }

        if data.code_insights.is_some() {
            content.push_str("✅ **Code Insights Data**: Generated\n");
            collected_count += 1;
        } else {
            content.push_str("❌ **Code Insights Data**: Not generated\n");
        }

        content.push_str(&format!(
            "\n**Research Completion Rate**: {}/4 ({:.1}%)\n\n",
            collected_count,
            (collected_count as f64 / 4.0) * 100.0
        ));

        // 5. Memory storage overview
        content.push_str("## Memory Storage Overview\n\n");
        if data.memory_stats.is_empty() {
            content.push_str("No Memory storage data available.\n\n");
        } else {
            let total_size: usize = data.memory_stats.values().sum();
            content.push_str(&format!("**Total Storage Size**: {} bytes\n", total_size));
            content.push_str(&format!(
                "**Number of Storage Scopes**: {}\n\n",
                data.memory_stats.len()
            ));

            // Display only the top 3 largest scopes
            let mut sorted_stats: Vec<_> = data.memory_stats.iter().collect();
            sorted_stats.sort_by(|a, b| b.1.cmp(a.1));

            content.push_str("### Main Storage Distribution (Top 3)\n");
            for (scope, size) in sorted_stats.iter().take(3) {
                let percentage = (**size as f64 / total_size as f64) * 100.0;
                content.push_str(&format!(
                    "- **{}**: {} bytes ({:.1}%)\n",
                    scope, size, percentage
                ));
            }
            content.push_str("\n");
        }

        // 6. Document generation overview
        content.push_str("## Document Generation Overview\n\n");
        content.push_str(&format!(
            "**Number of Generated Documents**: {}\n",
            data.generated_docs.len()
        ));

        if !data.generated_docs.is_empty() {
            content.push_str("**Document Types**: \n - ");
            content.push_str(&data.generated_docs.join("\n - "));
            content.push_str("\n");
        }
        content.push_str("\n");

        // 7. Overall assessment
        content.push_str("## Overall Assessment\n\n");

        // Data completeness assessment
        let data_completeness = (collected_count as f64 / 4.0) * 100.0;
        content.push_str(&format!("**Data Completeness**: {:.1}% ", data_completeness));
        if data_completeness == 100.0 {
            content.push_str("🟢 Complete\n");
        } else if data_completeness >= 75.0 {
            content.push_str("🟡 Mostly Complete\n");
        } else {
            content.push_str("🔴 Incomplete\n");
        }

        // Cache efficiency assessment
        content.push_str(&format!("**Cache Efficiency**: {:.1}% ", stats.hit_rate * 100.0));
        if stats.hit_rate >= 0.8 {
            content.push_str("🟢 Efficient\n");
        } else if stats.hit_rate >= 0.5 {
            content.push_str("🟡 Moderate\n");
        } else {
            content.push_str("🔴 Inefficient\n");
        }

        // Execution efficiency assessment
        content.push_str(&format!(
            "**Execution Efficiency**: {:.2}s ",
            timing.total_execution_time
        ));
        if timing.total_execution_time <= 60.0 {
            content.push_str("🟢 Fast\n");
        } else if timing.total_execution_time <= 300.0 {
            content.push_str("🟡 Normal\n");
        } else {
            content.push_str("🔴 Slow\n");
        }

        // Document generation completion
        let docs_generated = !data.generated_docs.is_empty();
        content.push_str(&format!(
            "**Document Generation**: {} ",
            if docs_generated {
                "Completed"
            } else {
                "Not Completed"
            }
        ));
        if docs_generated {
            content.push_str("🟢 Success\n");
        } else {
            content.push_str("🔴 Failed\n");
        }

        content
    }
}
