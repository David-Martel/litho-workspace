use crate::config::{Config, LLMProvider};
use crate::i18n::TargetLanguage;
use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

/// DeepWiki-RS - Project knowledge base generation engine powered by Rust and AI
#[derive(Parser, Debug)]
#[command(name = "Litho (deepwiki-rs)")]
#[command(
    about = "AI-based high-performance generation engine for documentation, It can intelligently analyze project structures, identify core modules, and generate professional architecture documentation."
)]
#[command(author = "Sopaco")]
#[command(version = env!("LITHO_BUILD_VERSION"))]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Project path
    #[arg(short, long, default_value = ".")]
    pub project_path: PathBuf,

    /// Output path
    #[arg(short, long, default_value = "./litho.docs")]
    pub output_path: PathBuf,

    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Project name
    #[arg(short, long)]
    pub name: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// High-efficiency model, prioritized for Litho engine's regular inference tasks
    #[arg(long)]
    pub model_efficient: Option<String>,

    /// High-quality model, prioritized for Litho engine's complex inference tasks, and as fallback when efficient fails
    #[arg(long)]
    pub model_powerful: Option<String>,

    /// LLM API base URL
    #[arg(long)]
    pub llm_api_base_url: Option<String>,

    /// LLM API KEY
    #[arg(long)]
    pub llm_api_key: Option<String>,

    /// Maximum number of tokens
    #[arg(long)]
    pub max_tokens: Option<u32>,

    /// Temperature parameter
    #[arg(long)]
    pub temperature: Option<f64>,

    /// Max parallelism parameter
    #[arg(long)]
    pub max_parallels: Option<usize>,

    /// LLM Provider (openai, mistral, openrouter, anthropic, deepseek)
    #[arg(long)]
    pub llm_provider: Option<String>,

    /// Target language (zh, en, ja, ko, de, fr, ru, vi)
    #[arg(long)]
    pub target_language: Option<String>,

    /// Auto use report assistant to view report after generation
    #[arg(long, default_value = "false", action = clap::ArgAction::SetTrue)]
    pub disable_preset_tools: bool,

    /// Disable cache
    #[arg(long)]
    pub no_cache: bool,

    /// Force regeneration (clear cache)
    #[arg(long)]
    pub force_regenerate: bool,

    /// Incremental mode: only regenerate docs for files changed since last run
    #[arg(long)]
    pub incremental: bool,

    /// Output format (md, html)
    #[arg(long, default_value = "md")]
    pub format: String,
}

/// CLI subcommands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Sync external knowledge sources (local docs, etc.)
    SyncKnowledge {
        /// Configuration file path
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Force sync even if cache is fresh
        #[arg(long)]
        force: bool,
    },
    /// Build/update repo index only (no LLM generation).
    IndexRepo {
        /// Configuration file path
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Optional project path override.
        #[arg(long)]
        project_path: Option<PathBuf>,
    },
    /// Benchmark model/parameter candidates and recommend an optimized profile.
    BenchmarkOptimize(Box<BenchmarkOptimizeCommand>),
}

#[derive(ClapArgs, Debug)]
pub struct BenchmarkOptimizeCommand {
    /// Configuration file path.
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Optional project path override.
    #[arg(long)]
    pub project_path: Option<PathBuf>,

    /// Output directory for benchmark artifacts/reports.
    #[arg(long, default_value = ".litho/benchmark")]
    pub output_dir: PathBuf,

    /// Comma-separated model list.
    #[arg(long)]
    pub models: Option<String>,

    /// Comma-separated context window values.
    #[arg(long)]
    pub context_windows: Option<String>,

    /// Comma-separated num_predict values.
    #[arg(long)]
    pub num_predict: Option<String>,

    /// Comma-separated temperature values.
    #[arg(long)]
    pub temperatures: Option<String>,

    /// Comma-separated top-p values.
    #[arg(long)]
    pub top_p_values: Option<String>,

    /// Comma-separated top-k values.
    #[arg(long)]
    pub top_k_values: Option<String>,

    /// Comma-separated repeat-penalty values.
    #[arg(long)]
    pub repeat_penalty_values: Option<String>,

    /// Comma-separated max-in-flight values.
    #[arg(long)]
    pub max_in_flight_values: Option<String>,

    /// Number of measured runs per candidate.
    #[arg(long, default_value_t = 3)]
    pub runs_per_candidate: usize,

    /// Warmup runs per candidate (not included in scoring).
    #[arg(long, default_value_t = 1)]
    pub warmup_runs: usize,

    /// Maximum number of candidates to evaluate.
    #[arg(long, default_value_t = 24)]
    pub max_candidates: usize,

    /// Hard timeout per benchmark run, in seconds.
    #[arg(long, default_value_t = 300)]
    pub run_timeout_seconds: u64,

    /// Minimum quality score required for recommendation.
    #[arg(long, default_value_t = 0.70)]
    pub min_quality: f64,

    /// Optional promotion gate: minimum required success rate [0.0, 1.0].
    #[arg(long)]
    pub gate_min_success_rate: Option<f64>,

    /// Optional promotion gate: maximum allowed p95 run duration in seconds.
    #[arg(long)]
    pub gate_max_p95_seconds: Option<f64>,

    /// Optional promotion gate: minimum required quality score [0.0, 1.0].
    #[arg(long)]
    pub gate_min_quality: Option<f64>,

    /// Composite-score quality weight.
    #[arg(long, default_value_t = 0.60)]
    pub weight_quality: f64,

    /// Composite-score latency weight.
    #[arg(long, default_value_t = 0.20)]
    pub weight_latency: f64,

    /// Composite-score throughput weight.
    #[arg(long, default_value_t = 0.10)]
    pub weight_throughput: f64,

    /// Composite-score memory weight.
    #[arg(long, default_value_t = 0.10)]
    pub weight_memory: f64,

    /// Composite-score stability weight.
    #[arg(long, default_value_t = 0.00)]
    pub weight_stability: f64,

    /// Keep cache enabled while benchmarking.
    #[arg(long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    pub keep_cache: bool,

    /// Keep per-run docs/artifacts on disk.
    #[arg(long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    pub retain_artifacts: bool,

    /// Build candidate matrix and output reports without executing generation runs.
    #[arg(long, default_value_t = false, action = clap::ArgAction::SetTrue)]
    pub dry_run: bool,
}

impl Args {
    /// Convert CLI arguments to configuration
    pub fn into_config(self) -> Config {
        // Determine target language early for proper message localization
        let target_lang = if let Some(ref lang_str) = self.target_language {
            lang_str.parse::<TargetLanguage>().unwrap_or_default()
        } else {
            TargetLanguage::default()
        };

        // Resolve the project path early so we can probe it for a litho.toml.
        // Canonicalize is best-effort; fall back to the raw value if the path
        // does not yet exist on disk.
        let resolved_project_path = self
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| self.project_path.clone());

        let mut config = if let Some(config_path) = &self.config {
            // Explicit --config flag takes highest priority.
            let msg = target_lang
                .msg_config_read_error()
                .replace("{:?}", &format!("{:?}", config_path));
            Config::from_file(config_path).expect(&msg)
        } else {
            // Auto-discovery: check the project directory first, then CWD.
            // Checking the project directory first means `--project-path
            // /some/repo` picks up `/some/repo/litho.toml` even when the
            // tool is invoked from a different working directory.
            let project_dir_toml = resolved_project_path.join("litho.toml");
            let cwd_toml = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("litho.toml");

            if project_dir_toml.exists() {
                let msg = target_lang
                    .msg_config_read_error()
                    .replace("{:?}", &format!("{:?}", project_dir_toml));
                Config::from_file(&project_dir_toml).expect(&msg)
            } else if cwd_toml.exists() {
                let msg = target_lang
                    .msg_config_read_error()
                    .replace("{:?}", &format!("{:?}", cwd_toml));
                Config::from_file(&cwd_toml).expect(&msg)
            } else {
                // No config file found anywhere; start from built-in defaults.
                Config::default()
            }
        };

        // Override settings from config file
        config.project_path = self.project_path.clone();
        config.output_path = self.output_path;
        config.internal_path = self.project_path.join(".litho");

        // Project name handling: CLI argument has highest priority, if CLI doesn't specify and config file doesn't have it, get_project_name() will auto-infer
        if let Some(name) = self.name {
            config.project_name = Some(name);
        }

        // Override LLM configuration
        if let Some(provider_str) = self.llm_provider {
            if let Ok(provider) = provider_str.parse::<LLMProvider>() {
                config.llm.provider = provider;
            } else {
                let msg = target_lang
                    .msg_unknown_provider()
                    .replace("{}", &provider_str);
                eprintln!("{}", msg);
            }
        }
        if let Some(llm_api_base_url) = self.llm_api_base_url {
            config.llm.api_base_url = llm_api_base_url;
        } else if config.llm.provider == LLMProvider::Ollama {
            config.llm.api_base_url = "http://localhost:11434".to_owned();
        }
        if let Some(llm_api_key) = self.llm_api_key {
            config.llm.api_key = llm_api_key;
        }
        if let Some(model_efficient) = self.model_efficient {
            config.llm.model_efficient = model_efficient;
        }
        if let Some(model_powerful) = self.model_powerful {
            config.llm.model_powerful = model_powerful;
        } else {
            config.llm.model_powerful = config.llm.model_efficient.to_string();
        }
        if let Some(max_tokens) = self.max_tokens {
            config.llm.max_tokens = max_tokens;
        }
        if let Some(temperature) = self.temperature {
            config.llm.temperature = Some(temperature);
        }
        if let Some(max_parallels) = self.max_parallels {
            config.llm.max_parallels = max_parallels;
        }
        config.llm.disable_preset_tools = self.disable_preset_tools;

        // Target language configuration
        if let Some(target_language_str) = self.target_language {
            if let Ok(target_language) = target_language_str.parse::<TargetLanguage>() {
                config.target_language = target_language;
            } else {
                let msg = target_lang
                    .msg_unknown_language()
                    .replace("{}", &target_language_str);
                eprintln!("{}", msg);
            }
        }

        // Cache configuration
        if self.no_cache {
            config.cache.enabled = false;
        }

        // Output format
        config.output_format = self.format;

        config
    }
}
