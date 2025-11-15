use anyhow::{Context, Ok, Result};
use clap::{Parser, Subcommand};
use log::{info, warn};
use notify_debouncer_full::{
    notify::{Config, EventKind},
    DebounceEventResult,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

mod binary;
mod builder;
mod config;
mod downloader;

use builder::Builder;
use config::FripackConfig;
use downloader::Downloader;

use crate::config::ResolvedConfig;

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
    /// Watch and auto-rebuild targets when files change
    Watch {
        /// Specific target to watch (required)
        target: String,
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
        Commands::Watch { target } => {
            watch_target(target).await?;
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

    info!("✓ Created configuration file: {}", config_path.display());

    Ok(())
}

fn load_config(path: &PathBuf, watch_mode: bool) -> Result<ResolvedConfig> {
    let config_content = std::fs::read_to_string(path)?;
    let config: FripackConfig = json5::from_str(&config_content)?;
    let mut resolved_config = config.resolve_inheritance()?;
    resolved_config
        .targets
        .values_mut()
        .for_each(|target| {
            target.watch_mode = watch_mode;
            if watch_mode {
                target.push_path.get_or_insert_with(|| "/data/local/tmp/fripack_dev.js".to_string());
            }
        });
    Ok(resolved_config)
}

async fn build_target(target: Option<String>) -> Result<()> {
    info!("Building fripack targets...");

    let config_path = find_config_file(std::env::current_dir()?)?;
    info!("→ Using configuration: {}", config_path.display());

    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    std::env::set_current_dir(config_dir)?;
    let resolved_config = load_config(&config_path, false)?;

    match target {
        Some(target_name) => {
            let target_config = resolved_config
                .targets
                .get(&target_name)
                .context("Failed to find the target")?;
            info!("→ Building target: {target_name}");
            let mut builder = Builder::new();
            builder.build_target(&target_name, target_config).await?;
            info!("✓ Successfully built target: {target_name}");
        }
        None => {
            info!("Building all targets...");
            let mut builder = Builder::new();

            for (target_name, target_config) in &resolved_config.targets {
                info!("→ Building target: {target_name}");
                builder.build_target(target_name, target_config).await?;
            }

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
    info!("Total Size: {}", format_bytes(stats.total_size));

    info!("\nCached Files:");
    info!("------------");

    for file_info in stats.files {
        info!("  • {} ({})", file_info.name, format_bytes(file_info.size));
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
        info!("✓ Successfully removed {removed_count} cached files");
    }

    Ok(())
}

async fn rebuild_install_target(
    target: &str,
    target_config: &config::ResolvedTarget,
) -> Result<()> {
    if target_config.target_type.as_deref() == Some("xposed") {
        let mut builder = Builder::new();
        let output_path = builder.build_target(&target, target_config).await?.unwrap();

        info!("→ Installing APK to device...");
        let output = tokio::process::Command::new("adb")
            .arg("install")
            .arg(&output_path)
            .output()
            .await?;

        if !output.status.success() {
            warn!(
                "Failed to install APK: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            info!("✓ APK installed successfully");
        }
    }
    Ok(())
}

async fn update_target(
    target: &str,
    target_config: &config::ResolvedTarget,
    config_updated: bool,
) -> Result<()> {
    if config_updated {
        info!("→ Configuration changed, rebuilding the target...");
        rebuild_install_target(target, target_config).await?;
    }
    let entry = target_config.entry.as_ref().unwrap();
    if Path::new(entry).exists() {
        info!("→ Pushing JS file to device...");
        let output = tokio::process::Command::new("adb")
            .arg("push")
            .arg(entry)
            .arg(&target_config.push_path.as_ref().unwrap())
            .output()
            .await?;

        if !output.status.success() {
            warn!(
                "Failed to push JS file: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            info!("✓ JS file pushed successfully");
        }
    }

    Ok(())
}

fn update_watcher_targets(
    watcher: &mut notify_debouncer_full::Debouncer<
        notify_debouncer_full::notify::ReadDirectoryChangesWatcher,
        notify_debouncer_full::FileIdMap,
    >,
    target_config: &config::ResolvedTarget,
) -> Result<()> {
    watcher.watch(
        "./fripack.json",
        notify_debouncer_full::notify::RecursiveMode::NonRecursive,
    )?;
    if let Some(watch_path) = &target_config.watch_path {
        watcher.watch(
            watch_path,
            notify_debouncer_full::notify::RecursiveMode::Recursive,
        )?;
    }

    watcher.watch(
        target_config.entry.clone().unwrap(),
        notify_debouncer_full::notify::RecursiveMode::NonRecursive,
    )?;

    Ok(())
}

async fn watch_target(target: String) -> Result<()> {
    info!("Watching target: {target}");

    let config_path = find_config_file(std::env::current_dir()?)?;
    info!("→ Using configuration: {}", config_path.display());

    let config_dir = config_path.parent().unwrap_or(std::path::Path::new("."));
    std::env::set_current_dir(config_dir)?;

    let resolved_config = load_config(&config_path, true)?;
    let target_config_cloned = resolved_config.targets[&target].clone();
    if let Err(e) = update_target(&target, &target_config_cloned, true).await {
        warn!("Failed to update target first: {}", e);
    };

    let target_config = Arc::new(Mutex::new(resolved_config.targets[&target].clone()));
    let mut watcher: notify_debouncer_full::Debouncer<
        notify_debouncer_full::notify::ReadDirectoryChangesWatcher,
        notify_debouncer_full::FileIdMap,
    > = notify_debouncer_full::new_debouncer(
        Duration::from_millis(500),
        None,
        move |res: DebounceEventResult| {
            use std::result::Result::Ok;

            match res {
                Ok(events) => {
                    let mut config_updated = false;
                    for event in events {
                        match &event.kind {
                            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                                if event.paths.contains(&config_path) {
                                    config_updated = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let target_config = if config_updated {
                            match load_config(&config_path, true) {
                                Ok(new_target_config) => {
                                    info!("→ Configuration updated, reloading...");
                                    let new_config = new_target_config
                                        .targets[&target]
                                        .clone();

                                    if new_config.entry != target_config.lock().unwrap().entry || new_config.watch_path != target_config.lock().unwrap().watch_path {
                                        panic!("Target entry or watchPath changed, please restart the watcher.");
                                    }

                                    target_config.lock().unwrap().clone_from(&new_config);
                                    target_config.clone()
                                }
                                Err(e) => {
                                    panic!("Failed to reload configuration: {}", e);
                                }
                            }
                        } else {
                            target_config.clone()
                        };

                        if let Err(e) = update_target(
                            &target,
                            &target_config.lock().unwrap(),
                            config_updated,
                        )
                        .await
                        {
                            warn!("Failed to update target: {}", e);
                        };
                    });
                }
                Err(e) => warn!("Watch error: {:?}", e),
            }
        },
    )?;

    update_watcher_targets(&mut watcher, &target_config_cloned)?;
    info!("✓ Watching for changes... Press Ctrl+C to stop.");

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
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
