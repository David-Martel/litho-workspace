use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use std::time::SystemTime;

use crate::generator::compose::DocumentationComposer;
use crate::generator::compose::memory::MemoryScope as ComposeMemoryScope;
use crate::generator::outlet::{DocTree, Outlet, OutletKind, SummaryOutlet};
use crate::generator::preprocess::memory::{MemoryScope as PreprocessMemoryScope, ScopedKeys};
use crate::generator::quality_gate;
use crate::generator::validator::ContentValidator;
use crate::integrations::change_detector;
use crate::integrations::manifest::{self, DocumentationManifest};
use crate::{
    cache::{CacheManager, repo_index},
    config::Config,
    generator::{
        context::GeneratorContext, preprocess::PreProcessAgent,
        research::orchestrator::ResearchOrchestrator, types::Generator,
    },
    llm::client::LLMClient,
    memory::Memory,
    types::{
        code::{CodeInsight, CodePurpose},
        project_structure::ProjectStructure,
    },
};
use anyhow::{Context, Result};
use tokio::sync::RwLock;
use walkdir::WalkDir;

/// Memory scope and key definitions for workflow timing statistics
pub struct TimingScope;

impl TimingScope {
    /// Memory scope for timing statistics
    pub const TIMING: &'static str = "timing";
}

/// Memory key definitions for each workflow stage
pub struct TimingKeys;

impl TimingKeys {
    /// Unix timestamp (seconds) captured at workflow start.
    pub const PIPELINE_START_UNIX: &'static str = "pipeline_start_unix";
    /// Time from workflow start until first successful LLM response.
    pub const FIRST_LLM_RESPONSE: &'static str = "first_llm_response";
    /// Preprocessing stage duration
    pub const PREPROCESS: &'static str = "preprocess";
    /// Original document extraction duration.
    pub const PREPROCESS_ORIGINAL_DOC: &'static str = "preprocess_original_doc";
    /// Project structure extraction duration.
    pub const PREPROCESS_STRUCTURE: &'static str = "preprocess_structure";
    /// Core-file identification duration.
    pub const PREPROCESS_IDENTIFY_CORE: &'static str = "preprocess_identify_core";
    /// AI code-analysis duration.
    pub const PREPROCESS_CODE_ANALYZE: &'static str = "preprocess_code_analyze";
    /// Relationship analysis duration.
    pub const PREPROCESS_RELATIONSHIPS: &'static str = "preprocess_relationships";
    /// Ingestion DAG/RAG construction duration.
    pub const PREPROCESS_INGESTION: &'static str = "preprocess_ingestion";
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

fn unix_timestamp_seconds() -> f64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

async fn prepare_provider_runtime(context: &GeneratorContext) -> Result<()> {
    if let Err(err) = context.llm_client.prepare_runtime().await {
        if context.config.llm.ollama_prepare_runtime_strict {
            return Err(err)
                .context("provider runtime preparation failed and strict mode is enabled");
        }
        eprintln!(
            "⚠️  Warning: provider runtime preparation failed: {} (continuing)",
            err
        );
    }
    Ok(())
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
    context
        .store_to_memory(
            TimingScope::TIMING,
            TimingKeys::PIPELINE_START_UNIX,
            unix_timestamp_seconds(),
        )
        .await?;

    prepare_provider_runtime(&context).await?;
    refresh_repo_index(&context).await?;

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

    // Store preprocessing digest for downstream stages
    let preprocess_digest = context.memory_digest("preprocess").await;
    context
        .store_to_memory("digests", "preprocess", preprocess_digest)
        .await?;

    // Execute multi-agent research stage
    let research_start = Instant::now();
    let research_orchestrator = ResearchOrchestrator;
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

    // Store research digest for compose stage
    let research_digest = context.memory_digest("research").await;
    context
        .store_to_memory("digests", "research", research_digest)
        .await?;

    // Execute document generation process
    let compose_start = Instant::now();
    let mut doc_tree = DocTree::new(&context.config.target_language);
    let documentation_orchestrator = DocumentationComposer;
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

    // Validate generated content against source truth
    let validation_report = ContentValidator::validate(&context).await?;
    println!(
        "\n=== Content Validation: score={:.0}% ({} errors, {} warnings) ===",
        validation_report.quality_score * 100.0,
        validation_report.errors(),
        validation_report.warnings(),
    );
    if validation_report.errors() > 0 {
        for finding in &validation_report.findings {
            if finding.severity == crate::generator::validator::Severity::Error {
                eprintln!("  \u{274c} [{}] {}", finding.category, finding.message);
            }
        }
    }

    // Persist report, check regression, enforce quality gate
    quality_gate::process_validation_report(&context, &validation_report).await?;

    // Execute document storage (format-aware outlet selection)
    let output_start = Instant::now();
    let outlet = OutletKind::for_format(&context.config.output_format, doc_tree.clone());
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
    let manifest = build_manifest_from_context(&context, &doc_tree, total_time).await;
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
/// - The manifest exists but is unreadable/corrupt
/// - More than 30% of tracked files changed
/// - The manifest's recorded commit no longer exists in the git history
pub async fn launch_incremental(c: &Config) -> Result<()> {
    let overall_start = Instant::now();

    // Load previous manifest
    let prev_manifest = DocumentationManifest::load(&c.internal_path).await;
    let manifest = match prev_manifest {
        Ok(Some(m)) => m,
        Ok(None) => {
            println!("📋 No previous manifest found, running full generation...");
            return launch(c).await;
        }
        Err(err) => {
            eprintln!(
                "⚠️  Warning: Failed to load manifest at {}: {}. Running full generation...",
                c.internal_path.join("manifest.json").display(),
                err
            );
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
    context
        .store_to_memory(
            TimingScope::TIMING,
            TimingKeys::PIPELINE_START_UNIX,
            unix_timestamp_seconds(),
        )
        .await?;

    prepare_provider_runtime(&context).await?;
    refresh_repo_index(&context).await?;

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

    // Store preprocessing digest for downstream stages
    let preprocess_digest = context.memory_digest("preprocess").await;
    context
        .store_to_memory("digests", "preprocess", preprocess_digest)
        .await?;

    // Selective research stage — only re-run research agents in the affected set.
    let research_start = Instant::now();
    let research_orchestrator = ResearchOrchestrator;
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

    // Store research digest for compose stage
    let research_digest = context.memory_digest("research").await;
    context
        .store_to_memory("digests", "research", research_digest)
        .await?;

    // Selective compose stage — only re-run documentation agents in the affected set.
    let compose_start = Instant::now();
    let mut doc_tree = DocTree::new(&context.config.target_language);
    let documentation_orchestrator = DocumentationComposer;
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

    // Validate generated content against source truth
    let validation_report = ContentValidator::validate(&context).await?;
    println!(
        "\n=== Content Validation: score={:.0}% ({} errors, {} warnings) ===",
        validation_report.quality_score * 100.0,
        validation_report.errors(),
        validation_report.warnings(),
    );

    // Persist report, check regression, enforce quality gate
    quality_gate::process_validation_report(&context, &validation_report).await?;

    // Output stage always runs — it writes whatever the agents produced (or re-produced).
    let output_start = Instant::now();
    let outlet = OutletKind::for_format(&context.config.output_format, doc_tree.clone());
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
    let updated_manifest = build_manifest_from_context(&context, &doc_tree, total_time).await;
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

/// Build/update the repo index only, without running LLM-backed generation.
pub async fn launch_repo_index_only(c: &Config) -> Result<()> {
    if !c.cache.repo_index_enabled {
        println!(
            "ℹ️ Repo index is disabled in configuration (`cache.repo_index_enabled = false`)."
        );
        return Ok(());
    }

    let cfg = c.clone();
    let db_path = cfg
        .cache
        .repo_index_path
        .clone()
        .unwrap_or_else(|| cfg.internal_path.join("repo-index.sqlite3"));
    let plan = tokio::task::spawn_blocking(move || compute_repo_diff_plan(&cfg))
        .await
        .context("repo index task join failed")??;

    println!("📚 Repo index refreshed at {}", db_path.display());
    println!(
        "   → {} new, {} changed, {} removed ({} unchanged)",
        plan.new_paths.len(),
        plan.changed_paths.len(),
        plan.removed_paths.len(),
        plan.unchanged
    );
    if !plan.git_changed_paths.is_empty() || !plan.git_removed_paths.is_empty() {
        println!(
            "   → git diff hints: {} changed, {} removed",
            plan.git_changed_paths.len(),
            plan.git_removed_paths.len()
        );
    }
    Ok(())
}

async fn refresh_repo_index(context: &GeneratorContext) -> Result<()> {
    if !context.config.cache.repo_index_enabled {
        return Ok(());
    }

    let cfg = context.config.clone();
    let plan = tokio::task::spawn_blocking(move || compute_repo_diff_plan(&cfg))
        .await
        .context("repo index task join failed")??;

    println!(
        "📚 Repo index: {} new, {} changed, {} removed ({} unchanged)",
        plan.new_paths.len(),
        plan.changed_paths.len(),
        plan.removed_paths.len(),
        plan.unchanged
    );
    if !plan.git_changed_paths.is_empty() || !plan.git_removed_paths.is_empty() {
        println!(
            "   ↳ git diff hints: {} changed, {} removed",
            plan.git_changed_paths.len(),
            plan.git_removed_paths.len()
        );
    }

    context
        .store_to_memory("repo_index", "diff_plan", &plan)
        .await?;
    Ok(())
}

fn compute_repo_diff_plan(config: &Config) -> Result<repo_index::RepoDiffPlan> {
    let db_path = config
        .cache
        .repo_index_path
        .clone()
        .unwrap_or_else(|| config.internal_path.join("repo-index.sqlite3"));
    let store = repo_index::RepoIndexStore::open(db_path)?;

    let snapshots = collect_repo_snapshots(config)?;
    let mut plan = store.diff_with_snapshots(&snapshots)?;
    let previous_commit = store.last_commit()?;
    let git_plan = repo_index::detect_git_diff(&config.project_path, previous_commit.as_deref());
    plan.previous_commit = git_plan.previous_commit.clone();
    plan.current_commit = git_plan.current_commit.clone();
    plan.git_changed_paths = git_plan.git_changed_paths;
    plan.git_removed_paths = git_plan.git_removed_paths;

    store.apply_snapshots(&snapshots, git_plan.current_commit.as_deref())?;
    Ok(plan)
}

fn collect_repo_snapshots(config: &Config) -> Result<Vec<repo_index::RepoFileSnapshot>> {
    let root = config
        .project_path
        .canonicalize()
        .unwrap_or_else(|_| config.project_path.clone());
    let excluded_dirs: HashSet<&str> = config.excluded_dirs.iter().map(|s| s.as_str()).collect();
    let excluded_files: HashSet<&str> = config.excluded_files.iter().map(|s| s.as_str()).collect();
    let excluded_exts: HashSet<String> = config
        .excluded_extensions
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let included_exts: HashSet<String> = config
        .included_extensions
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let mut out = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if should_skip_path(
            path,
            &root,
            config.include_hidden,
            config.include_tests,
            &excluded_dirs,
            &excluded_files,
            &excluded_exts,
            &included_exts,
        ) {
            continue;
        }

        let Ok(meta) = fs::metadata(path) else {
            continue;
        };
        if meta.len() > config.max_file_size {
            continue;
        }
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let modified_unix = meta
            .modified()
            .ok()
            .and_then(|v| v.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|v| v.as_secs() as i64)
            .unwrap_or_default();
        out.push(repo_index::RepoFileSnapshot {
            path: relative,
            content_hash: blake3::hash(&bytes).to_hex().to_string(),
            file_size: meta.len(),
            modified_unix,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn should_skip_path(
    path: &Path,
    root: &Path,
    include_hidden: bool,
    include_tests: bool,
    excluded_dirs: &HashSet<&str>,
    excluded_files: &HashSet<&str>,
    excluded_exts: &HashSet<String>,
    included_exts: &HashSet<String>,
) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if excluded_files.contains(file_name) {
        return true;
    }
    if !include_hidden
        && rel
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return true;
    }
    if rel
        .components()
        .any(|c| excluded_dirs.contains(c.as_os_str().to_string_lossy().as_ref()))
    {
        return true;
    }
    if !include_tests {
        let lowered = rel.to_string_lossy().to_ascii_lowercase();
        if lowered.contains("/test/")
            || lowered.contains("/tests/")
            || lowered.ends_with("_test.rs")
            || lowered.ends_with(".spec.ts")
            || lowered.ends_with(".test.ts")
        {
            return true;
        }
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !included_exts.is_empty() && !included_exts.contains(&ext) {
        return true;
    }
    if excluded_exts.contains(&ext) {
        return true;
    }
    false
}

async fn build_manifest_from_context(
    context: &GeneratorContext,
    doc_tree: &DocTree,
    total_time: f64,
) -> DocumentationManifest {
    let mut manifest = DocumentationManifest::new(context.config.project_path.clone());
    manifest.git_commit = change_detector::get_git_commit(&context.config.project_path).await;
    manifest.git_branch = change_detector::get_git_branch(&context.config.project_path).await;
    manifest.total_generation_time_secs = total_time;

    collect_manifest_file_hashes(context, &mut manifest).await;
    collect_manifest_modules(context, doc_tree, &mut manifest).await;

    manifest
}

async fn collect_manifest_file_hashes(
    context: &GeneratorContext,
    manifest: &mut DocumentationManifest,
) {
    let project_structure = context
        .get_from_memory::<ProjectStructure>(
            PreprocessMemoryScope::PREPROCESS,
            ScopedKeys::PROJECT_STRUCTURE,
        )
        .await;

    let Some(project_structure) = project_structure else {
        return;
    };

    for file in &project_structure.files {
        let absolute_path = project_structure.root_path.join(&file.path);
        if let Ok(hash) = manifest::_hash_file(&absolute_path).await {
            manifest._record_file_hash(file.path.clone(), hash);
        }
    }
}

async fn collect_manifest_modules(
    context: &GeneratorContext,
    doc_tree: &DocTree,
    manifest: &mut DocumentationManifest,
) {
    let code_insights = context
        .get_from_memory::<Vec<CodeInsight>>(
            PreprocessMemoryScope::PREPROCESS,
            ScopedKeys::CODE_INSIGHTS,
        )
        .await
        .unwrap_or_default();

    for (memory_key, output_file) in doc_tree.entries() {
        let Some(content) = context
            .get_from_memory::<String>(ComposeMemoryScope::DOCUMENTATION, &memory_key)
            .await
        else {
            continue;
        };

        let agent_type = normalize_compose_agent_type(&memory_key);
        let input_files = resolve_input_files_for_module(&agent_type, &code_insights);

        manifest._record_module(memory_key, agent_type, output_file, input_files, &content);
    }
}

fn normalize_compose_agent_type(memory_key: &str) -> String {
    if memory_key.starts_with("KeyModulesInsight_") {
        return "KeyModulesInsight".to_string();
    }

    match memory_key {
        "Project Overview" => "Overview".to_string(),
        "Architecture Description" => "Architecture".to_string(),
        "Core Workflows" => "Workflow".to_string(),
        "Boundary Interfaces" => "Boundary".to_string(),
        "Database Overview" => "Database".to_string(),
        other => other.to_string(),
    }
}

fn resolve_input_files_for_module(agent_type: &str, code_insights: &[CodeInsight]) -> Vec<PathBuf> {
    let mut set = HashSet::new();
    for insight in code_insights {
        let include = match agent_type {
            "Database" => {
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
            }
            _ => true,
        };
        if include {
            set.insert(insight.code_dossier.file_path.clone());
        }
    }

    let mut input_files: Vec<PathBuf> = set.into_iter().collect();
    input_files.sort();
    input_files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::code::{CodeDossier, CodePurpose};
    use std::collections::HashSet;
    use tempfile::tempdir;

    #[test]
    fn normalize_compose_agent_type_maps_expected_labels() {
        assert_eq!(
            normalize_compose_agent_type("KeyModulesInsight_auth"),
            "KeyModulesInsight"
        );
        assert_eq!(normalize_compose_agent_type("Project Overview"), "Overview");
        assert_eq!(
            normalize_compose_agent_type("Architecture Description"),
            "Architecture"
        );
        assert_eq!(
            normalize_compose_agent_type("Boundary Interfaces"),
            "Boundary"
        );
        assert_eq!(
            normalize_compose_agent_type("Database Overview"),
            "Database"
        );
    }

    #[test]
    fn resolve_input_files_for_database_filters_non_database_files() {
        let make_insight = |path: &str, purpose: CodePurpose| CodeInsight {
            code_dossier: CodeDossier {
                file_path: PathBuf::from(path),
                code_purpose: purpose,
                ..Default::default()
            },
            ..Default::default()
        };

        let insights = vec![
            make_insight("src/db/repository.rs", CodePurpose::Database),
            make_insight("schema/init.sql", CodePurpose::SpecificFeature),
            make_insight("db/schema.sqlproj", CodePurpose::SpecificFeature),
            make_insight("src/http/handler.rs", CodePurpose::Api),
        ];

        let files = resolve_input_files_for_module("Database", &insights);
        assert_eq!(
            files,
            vec![
                PathBuf::from("db/schema.sqlproj"),
                PathBuf::from("schema/init.sql"),
                PathBuf::from("src/db/repository.rs"),
            ]
        );
    }

    #[test]
    fn resolve_input_files_for_non_database_includes_all_deduped() {
        let insights = vec![
            CodeInsight {
                code_dossier: CodeDossier {
                    file_path: PathBuf::from("src/a.rs"),
                    ..Default::default()
                },
                ..Default::default()
            },
            CodeInsight {
                code_dossier: CodeDossier {
                    file_path: PathBuf::from("src/a.rs"),
                    ..Default::default()
                },
                ..Default::default()
            },
            CodeInsight {
                code_dossier: CodeDossier {
                    file_path: PathBuf::from("src/b.rs"),
                    ..Default::default()
                },
                ..Default::default()
            },
        ];

        let files = resolve_input_files_for_module("Overview", &insights);
        assert_eq!(
            files,
            vec![PathBuf::from("src/a.rs"), PathBuf::from("src/b.rs")]
        );
    }

    #[test]
    fn should_skip_path_respects_hidden_tests_and_extension_rules() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path();
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        let tests_dir = root.join("tests");
        std::fs::create_dir_all(&tests_dir).expect("create tests");
        let hidden_dir = root.join(".hidden");
        std::fs::create_dir_all(&hidden_dir).expect("create hidden");

        let src_file = src.join("main.rs");
        let test_file = tests_dir.join("main_test.rs");
        let hidden_file = hidden_dir.join("config.rs");
        std::fs::write(&src_file, "fn main(){}").expect("write src");
        std::fs::write(&test_file, "fn test(){}").expect("write test");
        std::fs::write(&hidden_file, "secret").expect("write hidden");

        let excluded_dirs: HashSet<&str> = ["target"].into_iter().collect();
        let excluded_files: HashSet<&str> = HashSet::new();
        let excluded_exts: HashSet<String> = ["log".to_string()].into_iter().collect();
        let included_exts: HashSet<String> = ["rs".to_string()].into_iter().collect();

        assert!(!should_skip_path(
            &src_file,
            root,
            false,
            false,
            &excluded_dirs,
            &excluded_files,
            &excluded_exts,
            &included_exts,
        ));
        assert!(should_skip_path(
            &test_file,
            root,
            false,
            false,
            &excluded_dirs,
            &excluded_files,
            &excluded_exts,
            &included_exts,
        ));
        assert!(should_skip_path(
            &hidden_file,
            root,
            false,
            true,
            &excluded_dirs,
            &excluded_files,
            &excluded_exts,
            &included_exts,
        ));
        assert!(!should_skip_path(
            &hidden_file,
            root,
            true,
            true,
            &excluded_dirs,
            &excluded_files,
            &excluded_exts,
            &included_exts,
        ));
    }
}
