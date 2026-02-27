use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
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
        self.execute_agent(&BoundaryAnalyzer::default(), context)
            .await?;

        // Database overview analysis (only if database files exist)
        if self.has_database_files(context).await {
            self.execute_agent(&DatabaseOverviewAnalyzer::default(), context)
                .await?;
        }

        println!("✓ Litho Studies Research pipeline execution completed");

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
