use anyhow::Result;
use colored::*;
use std::path::Path;
use tokio::fs;
use crate::config::{ResolvedConfig, ResolvedTarget};
use crate::downloader::Downloader;
use crate::binary::{BinaryProcessor, EmbeddedConfig};

pub struct Builder {
    config: ResolvedConfig,
    downloader: Downloader,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EmbeddedConfigData {
    mode: i32, // 1 = EmbeddedJs
    js_filepath: Option<String>,
    js_content: Option<String>,
}

impl Builder {
    pub fn new(config: &ResolvedConfig) -> Self {
        Self {
            config: config.clone(),
            downloader: Downloader::new(),
        }
    }
    
    pub async fn build_target(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        match target.target_type.as_deref() {
            Some("android-so") => self.build_android_so(target_name, target).await,
            Some("xposed") => self.build_xposed(target_name, target).await,
            Some(other) => anyhow::bail!("Unsupported target type: {}", other),
            None => anyhow::bail!("Missing required field: type"),
        }
    }
    
    async fn build_android_so(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        println!("{} {}", "→".blue(), format!("Building Android SO target: {}", target_name).blue());
        
        // Get required fields
        let platform = target.platform.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let frida_version = target.frida_version.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: fridaVersion"))?;
        let entry = target.entry.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: entry"))?;
        let use_xz = target.xz;
        
        // Get prebuilt file data
        let prebuilt_data = if let Some(override_file) = &target.override_prebuild_file {
            println!("{} {}", "→".blue(), format!("Using override prebuilt file: {}", override_file).blue());
            fs::read(override_file).await?
        } else {
            println!("{} {}", "→".blue(), format!("Downloading prebuilt file for platform: {}", platform).blue());
            self.downloader.download_prebuilt_file(platform, frida_version).await?
        };
        
        // Read entry file
        println!("{} {}", "→".blue(), format!("Reading entry file: {}", entry).blue());
        let entry_data = fs::read(entry).await?;
        
        // Process the binary
        println!("{} {}", "→".blue(), "Processing binary...".blue());
        let mut processor = BinaryProcessor::new(prebuilt_data)?;
        
        let config_data = EmbeddedConfigData {
            mode: 1,
            js_filepath: Some(entry.clone()),
            js_content: Some(String::from_utf8_lossy(&entry_data).to_string()),
        };

        let config_data = serde_json::to_string(&config_data)?;
        
        // Add embedded config section
        processor.add_embedded_config_data(config_data.as_bytes(), use_xz).unwrap();
        
        // Generate output filename
        let output_filename = format!("{}-{}.so", target_name, platform);
        
        // Write output file
        println!("{} {}", "→".blue(), format!("Writing output file: {}", output_filename).blue());
        let output_data = processor.into_data();
        fs::write(&output_filename, output_data).await?;
        
        println!("{} {}", "✓".green(), format!("Successfully built Android SO: {}", output_filename).green());
        
        Ok(())
    }
    
    async fn build_xposed(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        println!("{} {}", "→".blue(), format!("Building Xposed target: {}", target_name).blue());
        
        // Get required fields
        let platform = target.platform.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let package_name = target.package_name.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: packageName"))?;
        let keystore = target.keystore.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: keystore"))?;
        let name = target.name.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: name"))?;
        
        println!("{}", "⚠ Xposed module building is not yet implemented".yellow());
        println!("{} {}", "→".blue(), format!("Platform: {}", platform).blue());
        println!("{} {}", "→".blue(), format!("Package: {}", package_name).blue());
        println!("{} {}", "→".blue(), format!("Keystore: {}", keystore).blue());
        println!("{} {}", "→".blue(), format!("Name: {}", name).blue());
        
        unimplemented!();
    }
    
    pub async fn build_all(&mut self) -> Result<()> {
        println!("{}", "Building all targets...".blue().bold());
        
        let targets: Vec<(String, ResolvedTarget)> = self.config.targets.iter()
            .map(|(name, target)| (name.clone(), target.clone()))
            .collect();
        
        for (target_name, target) in targets {
            self.build_target(&target_name, &target).await?;
        }
        
        println!("{}", "✓ All targets built successfully!".green().bold());
        Ok(())
    }
}
