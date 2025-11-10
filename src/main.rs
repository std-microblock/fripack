use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

mod binary;
mod builder;
mod config;
mod downloader;

use builder::Builder;
use config::FripackConfig;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => {
            init_config(path).await?;
        }
        Commands::Build { target } => {
            build_target(target).await?;
        }
    }

    Ok(())
}

async fn init_config(path: PathBuf) -> Result<()> {
    println!("{}", "Initializing fripack configuration...".green().bold());

    let config_path = if path.is_dir() {
        path.join("fripack.json")
    } else {
        path
    };

    if config_path.exists() {
        println!("{}", "Configuration file already exists!".yellow());
        return Ok(());
    }

    let template_config = FripackConfig::template();
    let config_json = serde_json::to_string_pretty(&template_config)?;

    tokio::fs::write(&config_path, config_json).await?;

    println!(
        "{} {}",
        "✓".green(),
        format!("Created configuration file: {}", config_path.display()).green()
    );

    Ok(())
}

async fn build_target(target: Option<String>) -> Result<()> {
    println!("{}", "Building fripack targets...".green().bold());

    // Find the nearest configuration file
    let config_path = find_config_file(std::env::current_dir()?)?;
    println!(
        "{} {}",
        "→".blue(),
        format!("Using configuration: {}", config_path.display()).blue()
    );

    // Change working directory to config file location
    let config_dir = config_path.parent().unwrap_or(&std::path::Path::new("."));
    std::env::set_current_dir(config_dir)?;

    // Load and parse configuration
    let config_content = tokio::fs::read_to_string(&config_path).await?;
    let config: FripackConfig = json5::from_str(&config_content)?;

    // Resolve inheritance
    let resolved_config = config.resolve_inheritance()?;

    match target {
        Some(target_name) => {
            // Build specific target
            let target_config = resolved_config
                .targets
                .get(&target_name)
                .context("Failed to find the target")?;
            println!(
                "{} {}",
                "→".blue(),
                format!("Building target: {}", target_name).blue()
            );
            let mut builder = Builder::new(&resolved_config);
            builder.build_target(&target_name, target_config).await?;
            println!(
                "{} {}",
                "✓".green(),
                format!("Successfully built target: {}", target_name).green()
            );
        }
        None => {
            // Build all targets
            println!("{}", "Building all targets...".blue());
            let mut builder = Builder::new(&resolved_config);

            for (target_name, target_config) in &resolved_config.targets {
                println!(
                    "{} {}",
                    "→".blue(),
                    format!("Building target: {}", target_name).blue()
                );
                builder.build_target(target_name, target_config).await?;
                println!(
                    "{} {}",
                    "✓".green(),
                    format!("Successfully built target: {}", target_name).green()
                );
            }
        }
    }

    println!("{}", "✓ All builds completed successfully!".green().bold());
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
