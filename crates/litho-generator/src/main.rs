use anyhow::Result;
use clap::Parser;
use litho_generator::{
    benchmark::{self, BenchmarkOptimizationArgs},
    cli, config,
    generator::workflow::{launch, launch_incremental, launch_repo_index_only},
    integrations,
};

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(msg) = litho_core::build_info::assert_expected_token_sync() {
        anyhow::bail!("Build token sync check failed: {msg}");
    }

    let args = cli::Args::parse();

    // Handle subcommands
    if let Some(command) = args.command {
        return handle_subcommand(command, args.config).await;
    }

    // Default: run documentation generation
    let incremental = args.incremental;
    let config = args.into_config();
    config
        .quality
        .validate()
        .map_err(|msg| anyhow::anyhow!("invalid [quality] configuration: {msg}"))?;

    if incremental {
        launch_incremental(&config).await
    } else {
        launch(&config).await
    }
}

/// Handle CLI subcommands
async fn handle_subcommand(
    command: cli::Commands,
    config_path: Option<std::path::PathBuf>,
) -> Result<()> {
    match command {
        cli::Commands::SyncKnowledge { config, force } => {
            sync_knowledge(config.or(config_path), force).await
        }
        cli::Commands::IndexRepo {
            config,
            project_path,
        } => {
            let mut cfg = if let Some(path) = config.or(config_path) {
                config::Config::from_file(&path)?
            } else {
                let default_path = std::path::PathBuf::from("litho.toml");
                if default_path.exists() {
                    config::Config::from_file(&default_path)?
                } else {
                    config::Config::default()
                }
            };
            if let Some(project_path) = project_path {
                cfg.project_path = project_path;
            }
            launch_repo_index_only(&cfg).await
        }
        cli::Commands::BenchmarkOptimize(cmd) => {
            benchmark::run_benchmark_optimization(BenchmarkOptimizationArgs {
                config: cmd.config.clone().or(config_path),
                project_path: cmd.project_path.clone(),
                output_dir: cmd.output_dir.clone(),
                models: cmd.models.clone(),
                context_windows: cmd.context_windows.clone(),
                num_predict: cmd.num_predict.clone(),
                temperatures: cmd.temperatures.clone(),
                top_p_values: cmd.top_p_values.clone(),
                top_k_values: cmd.top_k_values.clone(),
                repeat_penalty_values: cmd.repeat_penalty_values.clone(),
                max_in_flight_values: cmd.max_in_flight_values.clone(),
                runs_per_candidate: cmd.runs_per_candidate,
                warmup_runs: cmd.warmup_runs,
                max_candidates: cmd.max_candidates,
                run_timeout_seconds: cmd.run_timeout_seconds,
                min_quality: cmd.min_quality,
                gate_min_success_rate: cmd.gate_min_success_rate,
                gate_max_p95_seconds: cmd.gate_max_p95_seconds,
                gate_min_quality: cmd.gate_min_quality,
                weight_quality: cmd.weight_quality,
                weight_latency: cmd.weight_latency,
                weight_throughput: cmd.weight_throughput,
                weight_memory: cmd.weight_memory,
                weight_stability: cmd.weight_stability,
                keep_cache: cmd.keep_cache,
                retain_artifacts: cmd.retain_artifacts,
                dry_run: cmd.dry_run,
            })
            .await
            .map(|_| ())
        }
    }
}

/// Sync external knowledge sources
async fn sync_knowledge(config_path: Option<std::path::PathBuf>, force: bool) -> Result<()> {
    use integrations::KnowledgeSyncer;

    // Load configuration
    let config = if let Some(path) = config_path {
        config::Config::from_file(&path)?
    } else {
        // Try default location
        let default_path = std::path::PathBuf::from("litho.toml");
        if default_path.exists() {
            config::Config::from_file(&default_path)?
        } else {
            println!("⚠️  No configuration file found. Using defaults.");
            config::Config::default()
        }
    };

    // Create syncer
    let syncer = KnowledgeSyncer::new(config)?;

    // Check if sync is needed
    if !force && !syncer.should_sync()? {
        println!("✅ Knowledge cache is up to date. Use --force to sync anyway.");
        return Ok(());
    }

    // Perform sync
    syncer.sync_all().await?;

    Ok(())
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
