use std::collections::HashSet;

use crate::generator::compose::agents::architecture_editor::ArchitectureEditor;
use crate::generator::compose::agents::boundary_editor::BoundaryEditor;
use crate::generator::compose::agents::database_editor::DatabaseEditor;
use crate::generator::compose::agents::key_modules_insight_editor::KeyModulesInsightEditor;
use crate::generator::compose::agents::overview_editor::OverviewEditor;
use crate::generator::compose::agents::review_agent::{ReviewAgent, SectionReview};
use crate::generator::compose::agents::workflow_editor::WorkflowEditor;
use crate::generator::context::GeneratorContext;
use crate::generator::outlet::DocTree;
use crate::generator::preprocess::memory::{MemoryScope, ScopedKeys};
use crate::generator::step_forward_agent::StepForwardAgent;
use crate::types::code::{CodeInsight, CodePurpose};
use anyhow::Result;

mod agents;
pub mod memory;
pub mod types;

/// Documentation composer
#[derive(Default)]
pub struct DocumentationComposer;

impl DocumentationComposer {
    pub async fn execute(&self, context: &GeneratorContext, doc_tree: &mut DocTree) -> Result<()> {
        println!("\n🤖 Executing documentation generation process...");
        println!(
            "📝 Target language: {}",
            context.config.target_language.display_name()
        );

        let overview_editor = OverviewEditor;
        overview_editor.execute(context).await?;

        let architecture_editor = ArchitectureEditor;
        architecture_editor.execute(context).await?;

        let workflow_editor = WorkflowEditor;
        workflow_editor.execute(context).await?;

        let key_modules_insight_editor = KeyModulesInsightEditor::default();
        key_modules_insight_editor
            .execute(context, doc_tree)
            .await?;

        let boundary_editor = BoundaryEditor;
        boundary_editor.execute(context).await?;

        // Database documentation (only if database files exist)
        if self.has_database_files(context).await {
            let database_editor = DatabaseEditor;
            database_editor.execute(context).await?;
        }

        // Secondary review pass with retry loop (when enabled in config)
        if context.config.review.enabled {
            run_review_loop(context).await?;
        }

        Ok(())
    }

    /// Execute only the compose agents whose short-form names appear in `affected_agents`.
    ///
    /// The short-form names used by the change detector are:
    /// `"Overview"`, `"Architecture"`, `"Workflow"`, `"KeyModulesInsight"`,
    /// `"Boundary"`, `"Database"`.
    ///
    /// If `affected_agents` is empty, all agents are executed (safety fallback).
    pub async fn execute_selective(
        &self,
        context: &GeneratorContext,
        doc_tree: &mut DocTree,
        affected_agents: &HashSet<String>,
    ) -> Result<()> {
        // Empty set means something went wrong upstream — run everything.
        if affected_agents.is_empty() {
            return self.execute(context, doc_tree).await;
        }

        println!("\n🤖 Executing selective documentation generation...");
        println!(
            "📝 Target language: {}",
            context.config.target_language.display_name()
        );

        let is_affected = |name: &str| affected_agents.contains(name);

        if is_affected("Overview") {
            let overview_editor = OverviewEditor;
            overview_editor.execute(context).await?;
        } else {
            println!("   Skipping Overview editor (not affected)");
        }

        if is_affected("Architecture") {
            let architecture_editor = ArchitectureEditor;
            architecture_editor.execute(context).await?;
        } else {
            println!("   Skipping Architecture editor (not affected)");
        }

        if is_affected("Workflow") {
            let workflow_editor = WorkflowEditor;
            workflow_editor.execute(context).await?;
        } else {
            println!("   Skipping Workflow editor (not affected)");
        }

        if is_affected("KeyModulesInsight") {
            let key_modules_insight_editor = KeyModulesInsightEditor::default();
            key_modules_insight_editor
                .execute(context, doc_tree)
                .await?;
        } else {
            println!("   Skipping KeyModulesInsight editor (not affected)");
        }

        if is_affected("Boundary") {
            let boundary_editor = BoundaryEditor;
            boundary_editor.execute(context).await?;
        } else {
            println!("   Skipping Boundary editor (not affected)");
        }

        // Database editor: only when affected AND database files exist
        if is_affected("Database") {
            if self.has_database_files(context).await {
                let database_editor = DatabaseEditor;
                database_editor.execute(context).await?;
            } else {
                println!("   Skipping Database editor (no database files found)");
            }
        } else {
            println!("   Skipping Database editor (not affected)");
        }

        // Secondary review pass with retry loop (when enabled in config)
        if context.config.review.enabled {
            run_review_loop(context).await?;
        }

        Ok(())
    }

    /// Check if the project has database-related files
    async fn has_database_files(&self, context: &GeneratorContext) -> bool {
        if let Some(insights) = context
            .get_from_memory::<Vec<CodeInsight>>(MemoryScope::PREPROCESS, ScopedKeys::CODE_INSIGHTS)
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
}

/// Run the secondary review loop with configurable retries.
///
/// On each iteration the review agent scores all documentation sections.
/// Sections below the threshold are re-generated with the reviewer's feedback
/// injected into the prompt. The loop terminates when all sections pass or
/// `max_retries` attempts have been exhausted.
async fn run_review_loop(context: &GeneratorContext) -> Result<()> {
    let max_retries = context.config.review.max_retries;

    for attempt in 0..=max_retries {
        println!(
            "\n Running secondary review pass (attempt {}/{})...",
            attempt + 1,
            max_retries + 1
        );

        let reviews = ReviewAgent::review_sections(context).await?;
        let failed: Vec<&SectionReview> = reviews.iter().filter(|r| !r.passed).collect();

        if failed.is_empty() || attempt == max_retries {
            if !failed.is_empty() {
                println!(
                    "   {} section(s) still below threshold after {} retries: {}",
                    failed.len(),
                    max_retries,
                    failed
                        .iter()
                        .map(|r| r.section.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            } else {
                println!("   All sections passed review.");
            }
            context
                .store_to_memory("review", "section_reviews", reviews)
                .await?;
            break;
        }

        println!(
            "   {} section(s) below threshold ({:.0}%): {}",
            failed.len(),
            context.config.review.min_review_score * 100.0,
            failed
                .iter()
                .map(|r| r.section.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Re-generate each failing section with reviewer feedback
        for review in &failed {
            regenerate_with_feedback(context, &review.section, &review.feedback).await?;
        }

        context
            .store_to_memory("review", "section_reviews", reviews)
            .await?;
    }

    Ok(())
}

/// Re-generate a single documentation section using reviewer feedback.
///
/// Loads the original content from the documentation memory scope, builds a
/// refinement prompt with the feedback, calls the LLM, and overwrites the
/// memory key with the improved content.
async fn regenerate_with_feedback(
    context: &GeneratorContext,
    section: &str,
    feedback: &str,
) -> Result<()> {
    let original = context
        .get_from_memory::<String>(memory::MemoryScope::DOCUMENTATION, section)
        .await
        .unwrap_or_default();

    if original.is_empty() {
        println!("   Skipping regen for '{}' (no original content)", section);
        return Ok(());
    }

    let system_prompt = "You are a technical documentation editor. Improve the provided \
        documentation section based on the reviewer feedback. Preserve the existing structure \
        and formatting. Return ONLY the improved documentation, no explanations.";

    let user_prompt = format!(
        "## Section: {section}\n\n\
         ## Reviewer Feedback\n{feedback}\n\n\
         ## Original Content\n{original}"
    );

    // Use the powerful model for refinement
    let model = &context.config.llm.model_powerful;
    let improved = context
        .llm_client
        .prompt_with_model(model, system_prompt, &user_prompt)
        .await?;

    if !improved.is_empty() {
        println!(
            "   Regenerated section '{}' ({} chars)",
            section,
            improved.len()
        );
        context
            .store_to_memory(memory::MemoryScope::DOCUMENTATION, section, improved)
            .await?;
    }

    Ok(())
}
