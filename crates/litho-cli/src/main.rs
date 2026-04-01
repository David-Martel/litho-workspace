//! `litho` — command-line interface for the litho documentation toolkit.
//!
//! # Subcommands
//!
//! - `litho extract <path>` — Walk a project tree, extract AST metadata, and
//!   emit structured output.
//! - `litho generate <path>` — Extract a project and then invoke the Codex-CLI
//!   doc generator to produce Markdown documentation.
//! - `litho status <path>` — Show generation status from the documentation manifest.
//! - `litho serve <path>` — Launch litho-book to serve generated documentation.
//! - `litho validate <path>` — Check generated docs for broken references.
//! - `litho qmd ...` — Forward QMD retrieval/indexing commands to `litho-qmd-cli`.
//! - `litho search <query>` — Run QMD hybrid search without entering passthrough mode.
//! - `litho query <query>` — Run QMD query mode without entering passthrough mode.
//! - `litho vsearch <query>` — Run QMD vector search without entering passthrough mode.

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use litho_codex::exec::CodexExecGenerator;
use litho_core::config::{ExtractBackend, LithoConfig};
use litho_core::types::ExtractedCodebase;
use litho_qmd_core::{QmdService, SearchOptions};
use litho_qmd_llm::AdaptiveLlmEngine;
use litho_qmd_storage::{AutoQmdStore, QmdBackendKind};
use std::path::PathBuf;
use std::process::Stdio;

type AppQmdService = QmdService<AutoQmdStore, AdaptiveLlmEngine>;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// litho — local LLM-powered codebase documentation toolkit.
///
/// Run `litho <COMMAND> --help` for subcommand-specific flags.
#[derive(Debug, Parser)]
#[command(
    name = "litho",
    version = env!("LITHO_BUILD_VERSION"),
    about = "Extract and document code with litho",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Walk a project tree and extract AST metadata.
    ///
    /// By default the result is serialised as JSON to stdout.
    /// Use `--format summary` for a compact human-readable report.
    Extract(ExtractArgs),

    /// Extract a project and generate Markdown documentation via Codex-CLI.
    Generate(GenerateArgs),

    /// Show generation status from the documentation manifest.
    Status(StatusArgs),

    /// Launch litho-book to serve generated documentation.
    Serve(ServeArgs),

    /// Check generated docs for broken file path references.
    Validate(ValidateArgs),

    /// Forward all remaining args to litho-qmd-cli.
    Qmd(QmdArgs),

    /// Run a QMD hybrid search.
    Search(QmdSearchArgs),

    /// Run a QMD query.
    Query(QmdSearchArgs),

    /// Run a QMD vector search.
    Vsearch(QmdSearchArgs),
}

// ---------------------------------------------------------------------------
// `extract` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct ExtractArgs {
    /// Root directory of the project to extract.
    path: PathBuf,

    /// Output format.
    ///
    /// `json` (default) emits the full [`ExtractedCodebase`] as JSON to
    /// stdout.  `summary` prints a human-readable breakdown.
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,

    /// Optional path to a `litho.toml` configuration file.
    ///
    /// When omitted, built-in defaults are used (no external servers
    /// contacted).
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Extraction backend strategy (`auto`, `tree-sitter`, `ast-grep`).
    #[arg(long, value_enum)]
    extract_backend: Option<ExtractBackendArg>,

    /// Optional ast-grep binary path override.
    #[arg(long, value_name = "BIN")]
    ast_grep_bin: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Summary,
}

#[derive(Debug, Clone, ValueEnum)]
enum ExtractBackendArg {
    Auto,
    TreeSitter,
    AstGrep,
}

impl From<ExtractBackendArg> for ExtractBackend {
    fn from(value: ExtractBackendArg) -> Self {
        match value {
            ExtractBackendArg::Auto => ExtractBackend::Auto,
            ExtractBackendArg::TreeSitter => ExtractBackend::TreeSitter,
            ExtractBackendArg::AstGrep => ExtractBackend::AstGrep,
        }
    }
}

// ---------------------------------------------------------------------------
// `generate` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct GenerateArgs {
    /// Root directory of the project to generate docs for.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Documentation provider to use.
    #[arg(long, value_enum, default_value_t = Provider::CodexLib)]
    provider: Provider,

    /// Directory where generated documentation files are written.
    ///
    /// Created if it does not exist.  Defaults to `./litho-docs/`.
    #[arg(long, value_name = "DIR", default_value = "./litho-docs/")]
    output: PathBuf,

    /// Model identifier passed to the provider (e.g. `o3`, `o4-mini`).
    ///
    /// When omitted the provider uses its built-in default.
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,
}

#[derive(Debug, Clone, ValueEnum)]
enum Provider {
    /// Use OpenAI Codex-CLI via direct library call (recommended).
    CodexLib,
    /// Use OpenAI Codex-CLI via subprocess execution.
    CodexExec,
}

// ---------------------------------------------------------------------------
// `status` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct StatusArgs {
    /// Project path to check status for.
    #[arg(default_value = ".")]
    path: PathBuf,
}

// ---------------------------------------------------------------------------
// `serve` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct ServeArgs {
    /// Path to generated documentation directory.
    path: PathBuf,

    /// Port to serve on.
    #[arg(long, default_value = "3333")]
    port: u16,
}

// ---------------------------------------------------------------------------
// `validate` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct ValidateArgs {
    /// Path to generated documentation directory.
    path: PathBuf,
}

#[derive(Debug, Parser)]
struct QmdArgs {
    /// Raw args forwarded to litho-qmd-cli.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum QmdBackendArg {
    Auto,
    Sqlite,
    Postgres,
}

impl QmdBackendArg {
    fn as_kind(self) -> Option<QmdBackendKind> {
        match self {
            QmdBackendArg::Auto => None,
            QmdBackendArg::Sqlite => Some(QmdBackendKind::Sqlite),
            QmdBackendArg::Postgres => Some(QmdBackendKind::Postgres),
        }
    }
}

#[derive(Debug, Parser)]
struct QmdSearchArgs {
    /// Query string.
    query: String,
    /// Max hits to return.
    #[arg(long, default_value_t = 10)]
    limit: usize,
    /// Minimum score threshold.
    #[arg(long = "min-score", default_value_t = 0.0)]
    min_score: f32,
    /// Optional collection filter.
    #[arg(short = 'c', long)]
    collection: Option<String>,
    /// Emit JSON output from qmd.
    #[arg(long)]
    json: bool,
    /// Optional qmd index override.
    #[arg(long)]
    index: Option<String>,
    /// Optional backend override.
    #[arg(long, value_enum)]
    backend: Option<QmdBackendArg>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    litho_core::env::LithoEnv::load();
    if let Err(msg) = litho_core::build_info::assert_expected_token_sync() {
        anyhow::bail!("Build token sync check failed: {msg}");
    }
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract(args) => cmd_extract(args).await,
        Commands::Generate(args) => cmd_generate(args).await,
        Commands::Status(args) => cmd_status(args).await,
        Commands::Serve(args) => cmd_serve(args).await,
        Commands::Validate(args) => cmd_validate(args).await,
        Commands::Qmd(args) => cmd_qmd(args).await,
        Commands::Search(args) => cmd_qmd_search_like("search", args).await,
        Commands::Query(args) => cmd_qmd_search_like("query", args).await,
        Commands::Vsearch(args) => cmd_qmd_search_like("vsearch", args).await,
    }
}

// ---------------------------------------------------------------------------
// `extract` handler
// ---------------------------------------------------------------------------

async fn cmd_extract(args: ExtractArgs) -> anyhow::Result<()> {
    let project_path = args
        .path
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", args.path.display()))?;

    let mut cfg = match args.config {
        Some(config_path) => LithoConfig::from_file(&config_path)
            .with_context(|| format!("failed to load config from {}", config_path.display()))?,
        None => LithoConfig::default(),
    };

    if let Some(backend) = args.extract_backend {
        cfg.extract_backend = backend.into();
    }
    if let Some(bin) = args.ast_grep_bin {
        cfg.ast_grep_binary = Some(bin);
    }

    let extracted = litho_extract::extract_with_config(&project_path, &cfg)
        .with_context(|| format!("extraction failed for {}", project_path.display()))?;

    match args.format {
        OutputFormat::Json => print_json(&extracted)?,
        OutputFormat::Summary => print_summary(&extracted),
    }

    Ok(())
}

/// Serialise `extracted` as pretty-printed JSON to stdout.
fn print_json(extracted: &ExtractedCodebase) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(extracted)
        .context("failed to serialise extraction result as JSON")?;
    println!("{json}");
    Ok(())
}

/// Print a human-readable extraction summary to stdout.
fn print_summary(extracted: &ExtractedCodebase) {
    let stats = &extracted.statistics;

    println!("Project : {}", extracted.project_name);
    println!("Files   : {}", stats.total_files);
    println!("LOC     : {}", stats.total_loc);
    println!();

    // Language breakdown, sorted descending by file count.
    println!("Language breakdown:");
    let mut langs: Vec<(&String, &usize)> = stats.languages.iter().collect();
    langs.sort_by(|a, b| b.1.cmp(a.1));
    for (lang, count) in &langs {
        println!("  {lang:<20} {count} file(s)");
    }
    println!();

    // Top 10 most complex files.
    if !stats.top_complex_files.is_empty() {
        println!(
            "Top {} most complex file(s):",
            stats.top_complex_files.len()
        );
        for (i, path) in stats.top_complex_files.iter().enumerate() {
            println!("  {:>2}. {}", i + 1, path.display());
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// `generate` handler
// ---------------------------------------------------------------------------

async fn cmd_generate(args: GenerateArgs) -> anyhow::Result<()> {
    let project_path = args
        .path
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", args.path.display()))?;

    // Ensure output directory exists.
    std::fs::create_dir_all(&args.output).with_context(|| {
        format!(
            "failed to create output directory: {}",
            args.output.display()
        )
    })?;

    let output_path = args
        .output
        .canonicalize()
        .with_context(|| format!("output path is invalid: {}", args.output.display()))?;

    eprintln!("Extracting project: {}", project_path.display());
    let extracted = litho_extract::extract(&project_path)
        .with_context(|| format!("extraction failed for {}", project_path.display()))?;

    eprintln!(
        "Extracted {} file(s), {} LOC",
        extracted.statistics.total_files, extracted.statistics.total_loc
    );

    let generator: Box<dyn litho_codex::provider::DocGenerator> = match args.provider {
        Provider::CodexLib => Box::new(litho_codex::exec::CodexLibGenerator {
            model: args
                .model
                .unwrap_or_else(|| litho_core::env::LithoEnv::codex_model().unwrap_or_default()),
        }),
        Provider::CodexExec => Box::new(CodexExecGenerator {
            model: args
                .model
                .unwrap_or_else(|| litho_core::env::LithoEnv::codex_model().unwrap_or_default()),
            sandbox: "read-only".into(),
            ..Default::default()
        }),
    };

    // Fast-fail if the provider isn't ready.
    generator
        .validate_readiness()
        .await
        .context("documentation provider is not ready")?;

    eprintln!("Generating documentation...");
    let sections = generator
        .generate(&extracted, &project_path, &output_path)
        .await
        .context("doc generation failed")?;

    for section in &sections {
        eprintln!("  wrote: {} ({})", section.filename, section.title);
    }

    eprintln!(
        "Done. {} section(s) written to {}",
        sections.len(),
        output_path.display()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// `status` handler
// ---------------------------------------------------------------------------

/// Lightweight manifest reader (mirrors litho-generator's DocumentationManifest
/// but avoids pulling in that crate as a dependency).
#[derive(serde::Deserialize)]
struct ManifestInfo {
    version: u32,
    generated_at: chrono::DateTime<chrono::Utc>,
    git_commit: Option<String>,
    git_branch: Option<String>,
    file_hashes: std::collections::HashMap<String, String>,
    modules: std::collections::HashMap<String, serde_json::Value>,
    total_generation_time_secs: f64,
}

async fn cmd_status(args: StatusArgs) -> anyhow::Result<()> {
    let project_path = args
        .path
        .canonicalize()
        .with_context(|| format!("project path does not exist: {}", args.path.display()))?;

    let manifest_path = project_path.join(".litho").join("manifest.json");
    if !manifest_path.exists() {
        println!(
            "No documentation manifest found at {}",
            manifest_path.display()
        );
        println!("Run `litho-generator` to generate documentation first.");
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&manifest_path).await?;
    let info: ManifestInfo =
        serde_json::from_str(&content).context("failed to parse manifest.json")?;

    println!("Documentation Status");
    println!("====================");
    println!("Manifest version : {}", info.version);
    println!(
        "Generated at     : {}",
        info.generated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "Git commit       : {}",
        info.git_commit.as_deref().unwrap_or("(unknown)")
    );
    println!(
        "Git branch       : {}",
        info.git_branch.as_deref().unwrap_or("(unknown)")
    );
    println!("Files tracked    : {}", info.file_hashes.len());
    println!("Modules generated: {}", info.modules.len());
    println!("Generation time  : {:.1}s", info.total_generation_time_secs);

    // Compute staleness: how many commits since the manifest
    if let Some(ref commit) = info.git_commit {
        let output = tokio::process::Command::new("git")
            .args(["rev-list", "--count", &format!("{}..HEAD", commit)])
            .current_dir(&project_path)
            .output()
            .await;

        if let Ok(out) = output
            && out.status.success()
        {
            let count = String::from_utf8_lossy(&out.stdout).trim().to_string();
            println!("Staleness        : {} commit(s) behind HEAD", count);
        }
    }

    if !info.modules.is_empty() {
        println!("\nModules:");
        for key in info.modules.keys() {
            println!("  - {}", key);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// `serve` handler
// ---------------------------------------------------------------------------

async fn cmd_serve(args: ServeArgs) -> anyhow::Result<()> {
    let docs_path = args
        .path
        .canonicalize()
        .with_context(|| format!("docs path does not exist: {}", args.path.display()))?;

    // Try to find litho-book binary
    let litho_book = which_litho_book();

    match litho_book {
        Some(bin) => {
            println!("Serving docs at http://localhost:{}", args.port);
            let status = tokio::process::Command::new(bin)
                .args([
                    "--docs-dir",
                    &docs_path.to_string_lossy(),
                    "--port",
                    &args.port.to_string(),
                ])
                .status()
                .await?;

            if !status.success() {
                anyhow::bail!("litho-book exited with status: {}", status);
            }
        }
        None => {
            println!("litho-book not found in PATH.");
            println!("To serve documentation, install litho-book or use any static file server:");
            println!(
                "  python -m http.server {} --directory {}",
                args.port,
                docs_path.display()
            );
        }
    }

    Ok(())
}

/// Try to find the litho-book binary in common locations.
fn which_litho_book() -> Option<PathBuf> {
    // Check PATH using platform-appropriate command
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    if let Ok(output) = std::process::Command::new(cmd).arg("litho-book").output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    // Check common locations
    let candidates = [
        ".claude/tools/bin/litho-book.exe",
        ".claude/tools/bin/litho-book",
        "target/release/litho-book.exe",
        "target/release/litho-book",
    ];

    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// `validate` handler
// ---------------------------------------------------------------------------

async fn cmd_validate(args: ValidateArgs) -> anyhow::Result<()> {
    let docs_path = args
        .path
        .canonicalize()
        .with_context(|| format!("docs path does not exist: {}", args.path.display()))?;

    println!("Validating documentation at {}", docs_path.display());
    println!();

    let mut issues: Vec<String> = Vec::new();
    let mut files_checked = 0;
    let project_root = detect_project_root(&docs_path);

    // Walk all .md files in the docs directory
    for entry in walkdir(&docs_path)? {
        let path = entry;
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        files_checked += 1;
        let content = tokio::fs::read_to_string(&path).await?;
        let relative = path.strip_prefix(&docs_path).unwrap_or(&path);

        // Check for backtick-quoted file paths that might be broken references
        for (line_num, line) in content.lines().enumerate() {
            // Find backtick-quoted paths (e.g., `src/main.rs`)
            for cap in find_backtick_paths(line) {
                let ref_path = PathBuf::from(&cap);
                // Only check paths that look like file references (contain / or \, have extension)
                if (cap.contains('/') || cap.contains('\\')) && cap.contains('.') {
                    let resolved = project_root.join(&ref_path);
                    if !resolved.exists() {
                        issues.push(format!(
                            "{}:{}: broken reference `{}`",
                            relative.display(),
                            line_num + 1,
                            cap
                        ));
                    }
                }
            }
        }
    }

    if issues.is_empty() {
        println!("All {} file(s) validated. No issues found.", files_checked);
    } else {
        println!(
            "Found {} issue(s) across {} file(s):",
            issues.len(),
            files_checked
        );
        println!();
        for issue in &issues {
            println!("  {}", issue);
        }
    }

    Ok(())
}

async fn cmd_qmd(args: QmdArgs) -> anyhow::Result<()> {
    run_qmd_cli(args.args).await
}

async fn cmd_qmd_search_like(mode: &str, args: QmdSearchArgs) -> anyhow::Result<()> {
    let service = build_qmd_service(args.index.as_deref(), args.backend)
        .context("failed to initialize qmd service")?;
    let response = match mode {
        "search" => service
            .search(
                &args.query,
                SearchOptions {
                    limit: args.limit,
                    min_score: args.min_score,
                    collection: args.collection,
                },
            )
            .map_err(anyhow::Error::msg)?,
        "query" => service
            .query(
                &args.query,
                SearchOptions {
                    limit: args.limit,
                    min_score: args.min_score,
                    collection: args.collection,
                },
            )
            .map_err(anyhow::Error::msg)?,
        "vsearch" => service
            .vsearch(
                &args.query,
                SearchOptions {
                    limit: args.limit,
                    min_score: args.min_score,
                    collection: args.collection,
                },
            )
            .map_err(anyhow::Error::msg)?,
        _ => anyhow::bail!("unsupported qmd mode: {mode}"),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else if mode == "search" {
        println!(
            "Found {} result(s) for \"{}\":",
            response.results.len(),
            response.query
        );
        for hit in response.results {
            println!(
                "{} {:.2} {} - {}",
                hit.docid, hit.score, hit.file, hit.title
            );
        }
    } else {
        for hit in response.results {
            println!(
                "{} {:.2} {} - {}",
                hit.docid, hit.score, hit.file, hit.title
            );
        }
    }
    Ok(())
}

async fn run_qmd_cli(args: Vec<String>) -> anyhow::Result<()> {
    let Some(bin) = which_qmd_cli() else {
        anyhow::bail!(
            "litho-qmd-cli not found in PATH. Build or install `litho-qmd-cli` to use `litho qmd ...`"
        );
    };

    let status = tokio::process::Command::new(bin)
        .args(&args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .context("failed to spawn litho-qmd-cli")?;

    if !status.success() {
        anyhow::bail!("litho-qmd-cli exited with status: {}", status);
    }

    Ok(())
}

fn build_qmd_service(
    index: Option<&str>,
    backend: Option<QmdBackendArg>,
) -> anyhow::Result<AppQmdService> {
    let backend_kind = backend.and_then(QmdBackendArg::as_kind);
    let store =
        AutoQmdStore::open_with_backend(index, backend_kind).context("failed to open qmd store")?;
    Ok(QmdService::new(store, AdaptiveLlmEngine::from_env()))
}

/// Detect project root for doc reference resolution.
///
/// Prefer nearest ancestor containing `.git` or `.litho`; otherwise fall back
/// to the direct parent of the docs directory.
fn detect_project_root(docs_path: &std::path::Path) -> PathBuf {
    for ancestor in docs_path.ancestors() {
        if ancestor.join(".git").exists() || ancestor.join(".litho").exists() {
            return ancestor.to_path_buf();
        }
    }

    docs_path.parent().unwrap_or(docs_path).to_path_buf()
}

fn which_qmd_cli() -> Option<PathBuf> {
    let cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    if let Ok(output) = std::process::Command::new(cmd)
        .arg("litho-qmd-cli")
        .output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    let candidates = [
        ".claude/tools/bin/litho-qmd-cli.exe",
        ".claude/tools/bin/litho-qmd-cli",
        "target/release/litho-qmd-cli.exe",
        "target/release/litho-qmd-cli",
    ];

    for candidate in &candidates {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Walk a directory and collect all file paths.
fn walkdir(path: &std::path::Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_recursive(path, &mut files)?;
    Ok(files)
}

fn walk_recursive(path: &std::path::Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                walk_recursive(&entry_path, files)?;
            } else {
                files.push(entry_path);
            }
        }
    } else {
        files.push(path.to_path_buf());
    }
    Ok(())
}

/// Find backtick-quoted strings that look like file paths.
fn find_backtick_paths(line: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_backtick = false;
    let mut current = String::new();

    for ch in line.chars() {
        if ch == '`' {
            if in_backtick {
                // End of backtick — check if it looks like a path
                if !current.is_empty() {
                    paths.push(current.clone());
                }
                current.clear();
            }
            in_backtick = !in_backtick;
        } else if in_backtick {
            current.push(ch);
        }
    }

    paths
}

#[cfg(test)]
mod cli_parse_tests {
    use super::*;

    #[test]
    fn cli_parses_status_default_path() {
        let cli = Cli::try_parse_from(["litho", "status"]).expect("failed to parse status command");
        match cli.command {
            Commands::Status(args) => assert_eq!(args.path, PathBuf::from(".")),
            _ => panic!("expected status subcommand"),
        }
    }

    #[test]
    fn cli_parses_serve_with_port() {
        let cli = Cli::try_parse_from(["litho", "serve", "docs", "--port", "4444"])
            .expect("failed to parse serve command");
        match cli.command {
            Commands::Serve(args) => {
                assert_eq!(args.path, PathBuf::from("docs"));
                assert_eq!(args.port, 4444);
            }
            _ => panic!("expected serve subcommand"),
        }
    }

    #[test]
    fn cli_parses_validate_with_path() {
        let cli =
            Cli::try_parse_from(["litho", "validate", "docs"]).expect("failed to parse validate");
        match cli.command {
            Commands::Validate(args) => assert_eq!(args.path, PathBuf::from("docs")),
            _ => panic!("expected validate subcommand"),
        }
    }

    #[test]
    fn cli_parses_generate_provider_output_and_model() {
        let cli = Cli::try_parse_from([
            "litho",
            "generate",
            ".",
            "--provider",
            "codex-exec",
            "--output",
            "out",
            "--model",
            "o3",
        ])
        .expect("failed to parse generate command");
        match cli.command {
            Commands::Generate(args) => {
                assert!(matches!(args.provider, Provider::CodexExec));
                assert_eq!(args.output, PathBuf::from("out"));
                assert_eq!(args.model.as_deref(), Some("o3"));
            }
            _ => panic!("expected generate subcommand"),
        }
    }

    #[test]
    fn cli_parses_extract_backend_and_format() {
        let cli = Cli::try_parse_from([
            "litho",
            "extract",
            ".",
            "--format",
            "summary",
            "--extract-backend",
            "ast-grep",
        ])
        .expect("failed to parse extract command");
        match cli.command {
            Commands::Extract(args) => {
                assert!(matches!(args.format, OutputFormat::Summary));
                assert!(matches!(
                    args.extract_backend,
                    Some(ExtractBackendArg::AstGrep)
                ));
            }
            _ => panic!("expected extract subcommand"),
        }
    }

    #[test]
    fn cli_rejects_invalid_generate_provider() {
        let err = Cli::try_parse_from(["litho", "generate", ".", "--provider", "bad-provider"])
            .expect_err("expected invalid provider parse error");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("possible values"),
            "unexpected parse error: {err}"
        );
    }

    #[test]
    fn cli_rejects_invalid_extract_backend() {
        let err =
            Cli::try_parse_from(["litho", "extract", ".", "--extract-backend", "bad-backend"])
                .expect_err("expected invalid backend parse error");
        assert!(
            err.to_string()
                .to_ascii_lowercase()
                .contains("possible values"),
            "unexpected parse error: {err}"
        );
    }

    #[test]
    fn cli_parses_qmd_passthrough_args() {
        let cli =
            Cli::try_parse_from(["litho", "qmd", "search", "--query", "cache", "--limit", "3"])
                .expect("failed to parse qmd passthrough command");
        match cli.command {
            Commands::Qmd(args) => assert_eq!(
                args.args,
                vec![
                    "search".to_string(),
                    "--query".to_string(),
                    "cache".to_string(),
                    "--limit".to_string(),
                    "3".to_string()
                ]
            ),
            _ => panic!("expected qmd subcommand"),
        }
    }

    #[test]
    fn cli_parses_search_subcommand() {
        let cli = Cli::try_parse_from([
            "litho",
            "search",
            "cache manager",
            "--limit",
            "5",
            "--min-score",
            "0.4",
            "--json",
            "--backend",
            "sqlite",
            "--index",
            "index",
        ])
        .expect("failed to parse search command");
        match cli.command {
            Commands::Search(args) => {
                assert_eq!(args.query, "cache manager");
                assert_eq!(args.limit, 5);
                assert_eq!(args.min_score, 0.4);
                assert!(args.json);
                assert!(matches!(args.backend, Some(QmdBackendArg::Sqlite)));
                assert_eq!(args.index.as_deref(), Some("index"));
            }
            _ => panic!("expected search subcommand"),
        }
    }

    #[test]
    fn cli_parses_query_subcommand_collection() {
        let cli =
            Cli::try_parse_from(["litho", "query", "status endpoint", "--collection", "docs"])
                .expect("failed to parse query command");
        match cli.command {
            Commands::Query(args) => {
                assert_eq!(args.query, "status endpoint");
                assert_eq!(args.collection.as_deref(), Some("docs"));
            }
            _ => panic!("expected query subcommand"),
        }
    }

    #[test]
    fn detect_project_root_prefers_ancestor_with_git() {
        let temp = tempfile::TempDir::new().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("create .git");
        let docs = repo.join("docs").join("auto").join("litho_docs");
        std::fs::create_dir_all(&docs).expect("create docs tree");

        let root = detect_project_root(&docs);
        assert_eq!(root, repo);
    }
}

#[cfg(test)]
mod build_token_tests {
    #[test]
    fn token_matches_pipeline_when_expected_is_set() {
        if let Ok(expected) = std::env::var("LITHO_EXPECT_BUILD_TOKEN")
            && !expected.trim().is_empty()
        {
            let actual = option_env!("LITHO_BUILD_TOKEN").unwrap_or("unknown-token");
            assert_eq!(
                actual, expected,
                "compiled build token differs from LITHO_EXPECT_BUILD_TOKEN"
            );
        }
    }
}
