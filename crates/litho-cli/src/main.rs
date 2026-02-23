//! `litho` — command-line interface for the litho documentation toolkit.
//!
//! # Subcommands
//!
//! - `litho extract <path>` — Walk a project tree, extract AST metadata, and
//!   emit structured output.
//! - `litho generate <path>` — Extract a project and then invoke the Codex-CLI
//!   doc generator to produce Markdown documentation.

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use litho_codex::exec::CodexExecGenerator;
use litho_codex::provider::DocGenerator as _;
use litho_core::config::LithoConfig;
use litho_core::types::ExtractedCodebase;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

/// litho — local LLM-powered codebase documentation toolkit.
///
/// Run `litho <COMMAND> --help` for subcommand-specific flags.
#[derive(Debug, Parser)]
#[command(
    name = "litho",
    version,
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
}

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Summary,
}

// ---------------------------------------------------------------------------
// `generate` subcommand
// ---------------------------------------------------------------------------

#[derive(Debug, Parser)]
struct GenerateArgs {
    /// Root directory of the project to document.
    path: PathBuf,

    /// Documentation provider to use.
    ///
    /// Currently only `codex` (OpenAI Codex-CLI) is supported.
    #[arg(long, value_enum, default_value_t = Provider::Codex)]
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
    Codex,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Extract(args) => cmd_extract(args).await,
        Commands::Generate(args) => cmd_generate(args).await,
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

    let extracted = match args.config {
        Some(config_path) => {
            let cfg = LithoConfig::from_file(&config_path).with_context(|| {
                format!("failed to load config from {}", config_path.display())
            })?;
            litho_extract::extract_with_config(&project_path, &cfg)
        }
        None => litho_extract::extract(&project_path),
    }
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
        println!("Top {} most complex file(s):", stats.top_complex_files.len());
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

    let generator = match args.provider {
        Provider::Codex => CodexExecGenerator {
            model: args.model.unwrap_or_default(),
            sandbox: "read-only".into(),
        },
    };

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
