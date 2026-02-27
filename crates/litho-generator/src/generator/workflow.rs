use std::sync::Arc;
use std::time::Instant;

use crate::generator::compose::DocumentationComposer;
use crate::generator::outlet::{DocTree, Outlet, OutletKind, SummaryOutlet};
use crate::integrations::change_detector;
use crate::integrations::manifest::DocumentationManifest;
use crate::{
    cache::CacheManager,
    config::Config,
    generator::{
        context::GeneratorContext, preprocess::PreProcessAgent,
        research::orchestrator::ResearchOrchestrator, types::Generator,
    },
    llm::client::LLMClient,
    memory::Memory,
};
use anyhow::Result;
use tokio::sync::RwLock;

/// Memory scope and key definitions for workflow timing statistics
pub struct TimingScope;

impl TimingScope {
    /// Memory scope for timing statistics
    pub const TIMING: &'static str = "timing";
}

/// Memory key definitions for each workflow stage
pub struct TimingKeys;

impl TimingKeys {
    /// Preprocessing stage duration
    pub const PREPROCESS: &'static str = "preprocess";
    /// Research stage duration
    pub const RESEARCH: &'static str = "research";
    /// Document generation stage duration
    pub const COMPOSE: &'static str = "compose";
    /// Output stage duration
    pub const OUTPUT: &'static str = "output";
    /// Document generation time
    pub const DOCUMENT_GENERATION: &'static str = "document_generation";
    /// Total execution time
    pub const TOTAL_EXECUTION: &'static str = "total_execution";
}

pub async fn launch(c: &Config) -> Result<()> {
    let overall_start = Instant::now();

    let config = c.clone();
    let llm_client = LLMClient::new(config.clone())?;
    let cache_manager = Arc::new(RwLock::new(CacheManager::new(
        config.cache.clone(),
        config.target_language.clone(),
    )));
    let memory = Arc::new(RwLock::new(Memory::new()));

    let context = GeneratorContext {
        llm_client,
        config,
        cache_manager,
        memory,
    };

    // Sync external knowledge if configured
    if let Ok(syncer) = crate::integrations::KnowledgeSyncer::new(context.config.clone()) {
        if syncer.should_sync().unwrap_or(false) {
            println!("\n=== Syncing external knowledge sources ===");
            if let Err(e) = syncer.sync_all().await {
                eprintln!("⚠️  Warning: Failed to sync external knowledge: {}", e);
            }
        } else {
            let lang = context.config.target_language.display_name();
            println!("ℹ️  External knowledge cache ({}) is up to date", lang);
        }
    }

    // Preprocessing stage
    let preprocess_start = Instant::now();
    let preprocess_agent = PreProcessAgent::new();
    preprocess_agent.execute(context.clone()).await?;
    let preprocess_time = preprocess_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::PREPROCESS, preprocess_time)
        .await?;
    println!(
        "=== Preprocessing completed, results stored to Memory (Duration: {:.2}s) ===",
        preprocess_time
    );

    // Execute multi-agent research stage
    let research_start = Instant::now();
    let research_orchestrator = ResearchOrchestrator::default();
    research_orchestrator
        .execute_research_pipeline(&context)
        .await?;
    let research_time = research_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::RESEARCH, research_time)
        .await?;
    println!(
        "\n=== Project in-depth research completed (Duration: {:.2}s) ===",
        research_time
    );

    // Execute document generation process
    let compose_start = Instant::now();
    let mut doc_tree = DocTree::new(&context.config.target_language);
    let documentation_orchestrator = DocumentationComposer::default();
    documentation_orchestrator
        .execute(&context, &mut doc_tree)
        .await?;
    let compose_time = compose_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::COMPOSE, compose_time)
        .await?;
    println!(
        "\n=== Document generation completed (Duration: {:.2}s) ===",
        compose_time
    );

    // Execute document storage (format-aware outlet selection)
    let output_start = Instant::now();
    let outlet = OutletKind::for_format(&context.config.output_format, doc_tree);
    outlet.save(&context).await?;

    // Generate and save summary report
    let summary_outlet = SummaryOutlet::new();
    summary_outlet.save(&context).await?;

    let output_time = output_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::OUTPUT, output_time)
        .await?;
    println!(
        "\n=== Document storage completed (Duration: {:.2}s) ===",
        output_time
    );

    // Record total execution time
    let total_time = overall_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::TOTAL_EXECUTION, total_time)
        .await?;

    // Save documentation manifest for incremental builds
    let mut manifest = DocumentationManifest::new(context.config.project_path.clone());
    manifest.git_commit =
        change_detector::get_git_commit(&context.config.project_path).await;
    manifest.git_branch =
        change_detector::get_git_branch(&context.config.project_path).await;
    manifest.total_generation_time_secs = total_time;
    if let Err(e) = manifest.save(&context.config.internal_path).await {
        eprintln!("⚠️  Warning: Failed to save manifest: {}", e);
    }

    println!(
        "\n🎉 All processes execution completed! Total duration: {:.2}s",
        total_time
    );

    Ok(())
}

/// Launch in incremental mode — only re-generate documentation for changed files.
///
/// Loads the previous manifest, detects changes via git, and selectively re-runs
/// only the affected pipeline stages. Falls back to a full rebuild when:
/// - No manifest exists (first run)
/// - More than 30% of tracked files changed
/// - The manifest's recorded commit no longer exists in the git history
pub async fn launch_incremental(c: &Config) -> Result<()> {
    let overall_start = Instant::now();

    // Load previous manifest
    let prev_manifest = DocumentationManifest::load(&c.internal_path).await;
    let manifest = match prev_manifest {
        Some(m) => m,
        None => {
            println!("📋 No previous manifest found, running full generation...");
            return launch(c).await;
        }
    };

    // Detect changes
    let changeset = change_detector::detect_changes(&c.project_path, &manifest).await?;

    if changeset.is_empty() {
        println!("✅ No changes detected since last generation. Documentation is up to date.");
        return Ok(());
    }

    if changeset.full_rebuild_needed {
        println!(
            "📋 {} files changed (>30% threshold), running full generation...",
            changeset.total_changes()
        );
        return launch(c).await;
    }

    println!(
        "📋 Incremental mode: {} files changed, {} agents affected",
        changeset.total_changes(),
        changeset.affected_agents.len()
    );
    for agent in &changeset.affected_agents {
        println!("   -> {}", agent);
    }

    // Build the full pipeline context (identical to launch())
    let config = c.clone();
    let llm_client = LLMClient::new(config.clone())?;
    let cache_manager = Arc::new(RwLock::new(CacheManager::new(
        config.cache.clone(),
        config.target_language.clone(),
    )));
    let memory = Arc::new(RwLock::new(Memory::new()));

    let context = GeneratorContext {
        llm_client,
        config,
        cache_manager,
        memory,
    };

    // Preprocessing always runs — it populates memory with fresh AST data that
    // every subsequent agent reads from.
    let preprocess_start = Instant::now();
    let preprocess_agent = PreProcessAgent::new();
    preprocess_agent.execute(context.clone()).await?;
    let preprocess_time = preprocess_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::PREPROCESS, preprocess_time)
        .await?;
    println!(
        "=== Preprocessing completed (Duration: {:.2}s) ===",
        preprocess_time
    );

    // Selective research stage — only re-run research agents in the affected set.
    let research_start = Instant::now();
    let research_orchestrator = ResearchOrchestrator::default();
    research_orchestrator
        .execute_research_pipeline_selective(&context, &changeset.affected_agents)
        .await?;
    let research_time = research_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::RESEARCH, research_time)
        .await?;
    println!(
        "\n=== Selective research completed (Duration: {:.2}s) ===",
        research_time
    );

    // Selective compose stage — only re-run documentation agents in the affected set.
    let compose_start = Instant::now();
    let mut doc_tree = DocTree::new(&context.config.target_language);
    let documentation_orchestrator = DocumentationComposer::default();
    documentation_orchestrator
        .execute_selective(&context, &mut doc_tree, &changeset.affected_agents)
        .await?;
    let compose_time = compose_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::COMPOSE, compose_time)
        .await?;
    println!(
        "\n=== Selective document generation completed (Duration: {:.2}s) ===",
        compose_time
    );

    // Output stage always runs — it writes whatever the agents produced (or re-produced).
    let output_start = Instant::now();
    let outlet = OutletKind::for_format(&context.config.output_format, doc_tree);
    outlet.save(&context).await?;

    let summary_outlet = SummaryOutlet::new();
    summary_outlet.save(&context).await?;

    let output_time = output_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::OUTPUT, output_time)
        .await?;
    println!(
        "\n=== Document storage completed (Duration: {:.2}s) ===",
        output_time
    );

    let total_time = overall_start.elapsed().as_secs_f64();
    context
        .store_to_memory(TimingScope::TIMING, TimingKeys::TOTAL_EXECUTION, total_time)
        .await?;

    // Save an updated manifest so the next incremental run has a fresh baseline.
    let mut updated_manifest = DocumentationManifest::new(context.config.project_path.clone());
    updated_manifest.git_commit =
        change_detector::get_git_commit(&context.config.project_path).await;
    updated_manifest.git_branch =
        change_detector::get_git_branch(&context.config.project_path).await;
    updated_manifest.total_generation_time_secs = total_time;
    if let Err(e) = updated_manifest.save(&context.config.internal_path).await {
        eprintln!(
            "Warning: Failed to save manifest after incremental run: {}",
            e
        );
    }

    println!(
        "\nIncremental generation completed! Total duration: {:.2}s",
        total_time
    );

    Ok(())
}
