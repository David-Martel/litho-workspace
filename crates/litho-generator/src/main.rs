use crate::generator::workflow::launch;
use anyhow::Result;
use clap::Parser;

mod cache;
mod cli;
mod config;
mod generator;
mod i18n;
mod integrations;
mod llm;
mod memory;
mod types;
mod utils;

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
        crate::generator::workflow::launch_incremental(&config).await
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
