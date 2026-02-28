use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use tokio::process::Command;

use crate::generator::context::GeneratorContext;
use crate::generator::preprocess::memory::{MemoryScope as PreprocessMemoryScope, ScopedKeys};
use crate::generator::research::agents::architecture_researcher::ArchitectureResearcher;
use crate::generator::research::agents::boundary_analyzer::BoundaryAnalyzer;
use crate::generator::research::agents::database_overview_analyzer::DatabaseOverviewAnalyzer;
use crate::generator::research::agents::domain_modules_detector::DomainModulesDetector;
use crate::generator::research::agents::key_modules_insight::KeyModulesInsight;
use crate::generator::research::agents::system_context_researcher::SystemContextResearcher;
use crate::generator::research::agents::workflow_researcher::WorkflowResearcher;
use crate::generator::research::memory::MemoryScope as ResearchMemoryScope;
use crate::generator::step_forward_agent::StepForwardAgent;
use crate::types::code::{CodeInsight, CodePurpose};

/// Multi-agent research orchestrator
#[derive(Default)]
pub struct ResearchOrchestrator;

#[derive(Debug, Deserialize)]
struct QmdSearchResponse {
    #[serde(default)]
    results: Vec<QmdSearchHit>,
}

#[derive(Debug, Deserialize)]
struct QmdSearchHit {
    file: String,
    title: String,
    snippet: String,
    score: f32,
}

impl ResearchOrchestrator {
    /// Execute all agent analysis pipelines
    pub async fn execute_research_pipeline(&self, context: &GeneratorContext) -> Result<()> {
        println!("🚀 Starting Litho Studies Research investigation pipeline...");
        self.seed_qmd_research_context(context).await?;

        // First layer: Macro analysis (C1)
        self.execute_agent(&SystemContextResearcher, context)
            .await?;

        // Second layer: Meso analysis (C2)
        // These agents consume preprocess artifacts and can run concurrently.
        tokio::try_join!(
            self.execute_agent(&DomainModulesDetector, context),
            self.execute_agent(&ArchitectureResearcher, context),
            self.execute_agent(&WorkflowResearcher, context)
        )?;

        // Third layer: Micro analysis (C3-C4)
        self.execute_agent(&KeyModulesInsight, context).await?;

        // Boundary interface analysis
        self.execute_agent(&BoundaryAnalyzer, context).await?;

        // Database overview analysis (only if database files exist)
        if self.has_database_files(context).await {
            self.execute_agent(&DatabaseOverviewAnalyzer, context)
                .await?;
        }

        println!("✓ Litho Studies Research pipeline execution completed");

        Ok(())
    }

    /// Execute only the research agents whose short-form names appear in `affected_agents`.
    ///
    /// The short-form names used by the change detector are the enum variant name strings:
    /// `"SystemContextResearcher"`, `"DomainModulesDetector"`, `"ArchitectureResearcher"`,
    /// `"WorkflowResearcher"`, `"KeyModulesInsight"`, `"BoundaryAnalyzer"`,
    /// `"DatabaseOverviewAnalyzer"`.
    ///
    /// If `affected_agents` is empty, all agents are executed (safety fallback).
    pub async fn execute_research_pipeline_selective(
        &self,
        context: &GeneratorContext,
        affected_agents: &HashSet<String>,
    ) -> Result<()> {
        // Empty set means something went wrong upstream — run everything.
        if affected_agents.is_empty() {
            return self.execute_research_pipeline(context).await;
        }

        println!(
            "🚀 Starting selective research pipeline ({} agents affected)...",
            affected_agents.len()
        );
        self.seed_qmd_research_context(context).await?;

        let is_affected = |name: &str| affected_agents.contains(name);

        // Layer 1: SystemContextResearcher — others may depend on its memory output.
        if is_affected("SystemContextResearcher") {
            self.execute_agent(&SystemContextResearcher, context)
                .await?;
        } else {
            println!("   Skipping SystemContextResearcher (not affected)");
        }

        // Layer 2: parallel meso agents — only run the affected subset.
        // tokio::try_join! requires a fixed arity, so we enumerate all eight
        // possible (run/skip) combinations for the three agents.
        let run_domain = is_affected("DomainModulesDetector");
        let run_arch = is_affected("ArchitectureResearcher");
        let run_workflow = is_affected("WorkflowResearcher");

        if !run_domain {
            println!("   Skipping DomainModulesDetector (not affected)");
        }
        if !run_arch {
            println!("   Skipping ArchitectureResearcher (not affected)");
        }
        if !run_workflow {
            println!("   Skipping WorkflowResearcher (not affected)");
        }

        match (run_domain, run_arch, run_workflow) {
            (true, true, true) => {
                tokio::try_join!(
                    self.execute_agent(&DomainModulesDetector, context),
                    self.execute_agent(&ArchitectureResearcher, context),
                    self.execute_agent(&WorkflowResearcher, context)
                )?;
            }
            (true, true, false) => {
                tokio::try_join!(
                    self.execute_agent(&DomainModulesDetector, context),
                    self.execute_agent(&ArchitectureResearcher, context)
                )?;
            }
            (true, false, true) => {
                tokio::try_join!(
                    self.execute_agent(&DomainModulesDetector, context),
                    self.execute_agent(&WorkflowResearcher, context)
                )?;
            }
            (false, true, true) => {
                tokio::try_join!(
                    self.execute_agent(&ArchitectureResearcher, context),
                    self.execute_agent(&WorkflowResearcher, context)
                )?;
            }
            (true, false, false) => {
                self.execute_agent(&DomainModulesDetector, context).await?;
            }
            (false, true, false) => {
                self.execute_agent(&ArchitectureResearcher, context).await?;
            }
            (false, false, true) => {
                self.execute_agent(&WorkflowResearcher, context).await?;
            }
            (false, false, false) => {
                // None of the layer-2 agents are affected.
            }
        }

        // Layer 3: micro analysis
        if is_affected("KeyModulesInsight") {
            self.execute_agent(&KeyModulesInsight, context).await?;
        } else {
            println!("   Skipping KeyModulesInsight (not affected)");
        }

        // Boundary interface analysis
        if is_affected("BoundaryAnalyzer") {
            self.execute_agent(&BoundaryAnalyzer, context).await?;
        } else {
            println!("   Skipping BoundaryAnalyzer (not affected)");
        }

        // Database overview analysis: only when affected AND database files exist
        if is_affected("DatabaseOverviewAnalyzer") {
            if self.has_database_files(context).await {
                self.execute_agent(&DatabaseOverviewAnalyzer, context)
                    .await?;
            } else {
                println!("   Skipping DatabaseOverviewAnalyzer (no database files found)");
            }
        } else {
            println!("   Skipping DatabaseOverviewAnalyzer (not affected)");
        }

        println!("✓ Selective research pipeline execution completed");

        Ok(())
    }

    /// Check if the project has database-related files
    async fn has_database_files(&self, context: &GeneratorContext) -> bool {
        if let Some(insights) = context
            .get_from_memory::<Vec<CodeInsight>>(
                PreprocessMemoryScope::PREPROCESS,
                ScopedKeys::CODE_INSIGHTS,
            )
            .await
        {
            insights.iter().any(|insight| {
                matches!(insight.code_dossier.code_purpose, CodePurpose::Database)
                    || insight
                        .code_dossier
                        .file_path
                        .to_string_lossy()
                        .ends_with(".sql")
                    || insight
                        .code_dossier
                        .file_path
                        .to_string_lossy()
                        .ends_with(".sqlproj")
            })
        } else {
            false
        }
    }

    /// Execute a single agent
    async fn execute_agent<T>(&self, agent: &T, context: &GeneratorContext) -> Result<()>
    where
        T: StepForwardAgent + Send + Sync,
    {
        // Use localized agent name if available
        let agent_name = if let Some(agent_enum) = agent.agent_type_enum() {
            agent_enum.display_name(&context.config.target_language)
        } else {
            agent.agent_type()
        };

        println!("🤖 Executing {} agent analysis...", agent_name);

        agent.execute(context).await?;
        println!("✓ {} analysis completed", agent_name);
        Ok(())
    }

    async fn seed_qmd_research_context(&self, context: &GeneratorContext) -> Result<()> {
        let qmd = &context.config.qmd;
        if !qmd.enabled {
            return Ok(());
        }

        let queries = if qmd.queries.is_empty() {
            vec![
                "project architecture boundaries components interfaces".to_string(),
                "main workflows orchestration request flow".to_string(),
                "data model persistence storage database".to_string(),
            ]
        } else {
            qmd.queries.clone()
        };

        let mut batches = Vec::new();
        for query in queries {
            let hits = match self.run_qmd_query(context, &query).await {
                Ok(h) => h,
                Err(err) => {
                    eprintln!("⚠️  QMD retrieval query failed for '{}': {}", query, err);
                    continue;
                }
            };
            if hits.is_empty() {
                continue;
            }
            batches.push(json!({
                "query": query,
                "results": hits,
            }));
        }

        if batches.is_empty() {
            return Ok(());
        }

        let payload = json!({
            "source": "qmd",
            "batches": batches,
        });
        context
            .store_to_memory(
                ResearchMemoryScope::STUDIES_RESEARCH,
                &qmd.store_key,
                payload,
            )
            .await?;

        println!(
            "📚 Seeded studies_research from QMD into key '{}'",
            qmd.store_key
        );
        Ok(())
    }

    async fn run_qmd_query(&self, context: &GeneratorContext, query: &str) -> Result<Vec<Value>> {
        let qmd = &context.config.qmd;
        let mut command = Command::new(&qmd.bin);
        command
            .arg(&qmd.mode)
            .arg(query)
            .arg("--json")
            .arg("--limit")
            .arg(qmd.limit.to_string())
            .current_dir(&context.config.project_path);

        if let Some(index) = &qmd.index {
            command.arg("--index").arg(index);
        }

        let output = command.output().await?;
        if !output.status.success() {
            anyhow::bail!(
                "qmd exited with status {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let parsed: QmdSearchResponse = serde_json::from_slice(&output.stdout)?;
        let max_snippets = qmd.max_snippets_per_query.max(1);
        Ok(parsed
            .results
            .into_iter()
            .take(max_snippets)
            .map(|hit| {
                json!({
                    "file": hit.file,
                    "title": hit.title,
                    "snippet": hit.snippet,
                    "score": hit.score,
                })
            })
            .collect())
    }
}
