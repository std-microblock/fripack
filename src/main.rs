use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::{info, warn};
use std::path::PathBuf;

mod binary;
mod builder;
mod config;
mod downloader;

use builder::Builder;
use config::FripackConfig;
use downloader::Downloader;

#[derive(Parser)]
#[command(name = "fripack")]
#[command(about = "A cross-platform CLI tool for building Frida-based packages", long_about = None)]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new fripack configuration file
    Init {
        /// Path to create the configuration file (default: current directory)
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Build targets from configuration
    Build {
        /// Specific target to build (optional, builds all if not specified)
        target: Option<String>,
    },
    /// Cache management commands
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    /// Show cache statistics and list cached files
    Query,
    /// Clear all cached files
    Clear,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .filter_level(log::LevelFilter::Info)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            init_config(path).await?;
        }
        Commands::Build { target } => {
            build_target(target).await?;
        }
        Commands::Cache { action } => {
            handle_cache_action(action).await?;
        }
    }

    Ok(())
}

async fn init_config(path: PathBuf) -> Result<()> {
    info!("Initializing fripack configuration...");

    let config_path = if path.is_dir() {
        path.join("fripack.json")
    } else {
        path
    };

    if config_path.exists() {
        warn!("Configuration file already exists!");
        return Ok(());
    }

    let template_config = FripackConfig::template();
    let config_json = serde_json::to_string_pretty(&template_config)?;

    tokio::fs::write(&config_path, config_json).await?;

    info!(
        "✓ Created configuration file: {}",
        config_path.display()
    );

    Ok(())
}

async fn build_target(target: Option<String>) -> Result<()> {
    info!("Building fripack targets...");

    let config_path = find_config_file(std::env::current_dir()?)?;
    info!(
        "→ Using configuration: {}",
        config_path.display()
    );

    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    std::env::set_current_dir(config_dir)?;

    let config_content = tokio::fs::read_to_string(&config_path).await?;
    let config: FripackConfig = json5::from_str(&config_content)?;

    let resolved_config = config.resolve_inheritance()?;

    match target {
        Some(target_name) => {
            let target_config = resolved_config
                .targets
                .get(&target_name)
                .context("Failed to find the target")?;
            info!(
                "→ Building target: {target_name}"
            );
            let mut builder = Builder::new(&resolved_config);
            builder.build_target(&target_name, target_config).await?;
            info!(
                "✓ Successfully built target: {target_name}"
            );
        }
        None => {
            info!("Building all targets...");
            let mut builder = Builder::new(&resolved_config);
            builder.build_all().await?;
            info!("✓ Successfully built all targets!");
        }
    }

    info!("✓ All builds completed successfully!");
    Ok(())
}

fn find_config_file(start_dir: PathBuf) -> Result<PathBuf> {
    let mut current_dir = start_dir;

    loop {
        let fripack_json = current_dir.join("fripack.json");
        let fripack_config = current_dir.join("fripack.config.json");

        if fripack_json.exists() {
            return Ok(fripack_json);
        }
        if fripack_config.exists() {
            return Ok(fripack_config);
        }

        if let Some(parent) = current_dir.parent() {
            current_dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    anyhow::bail!("Could not find fripack configuration file in current or parent directories");
}

async fn handle_cache_action(action: CacheAction) -> Result<()> {
    let downloader = Downloader::new();

    match action {
        CacheAction::Query => {
            query_cache(&downloader).await?;
        }
        CacheAction::Clear => {
            clear_cache(&downloader).await?;
        }
    }

    Ok(())
}

async fn query_cache(downloader: &Downloader) -> Result<()> {
    info!("Cache Information");
    info!("================");

    let cache_dir = downloader.cache_dir();
    info!("Cache Directory: {}", cache_dir.display());

    let stats = downloader.get_cache_stats().await?;

    if stats.file_count == 0 {
        warn!("No cached files found.");
        return Ok(());
    }

    info!("Total Files: {}", stats.file_count);
    info!(
        "Total Size: {}",
        format_bytes(stats.total_size)
    );

    info!("\nCached Files:");
    info!("------------");

    for file_info in stats.files {
        info!(
            "  • {} ({})",
            file_info.name,
            format_bytes(file_info.size)
        );
    }

    Ok(())
}

async fn clear_cache(downloader: &Downloader) -> Result<()> {
    warn!("Clearing Cache");
    warn!("==============");

    let stats = downloader.get_cache_stats().await?;

    if stats.file_count == 0 {
        warn!("No cached files to clear.");
        return Ok(());
    }

    info!(
        "Found: {} files ({} total)",
        stats.file_count,
        format_bytes(stats.total_size)
    );

    let removed_count = downloader.clear_cache().await?;

    if removed_count > 0 {
        info!(
            "✓ Successfully removed {removed_count} cached files"
        );
    }

    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.2} {}", size, UNITS[unit_index])
    }
}
