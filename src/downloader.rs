use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use futures_util::StreamExt;

pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
    
    pub async fn download_prebuilt_file(&self, platform: &str, frida_version: &str) -> Result<Vec<u8>> {
        let filename = format!("libfripack-inject-{}-{}.so", platform, frida_version);
        let url = format!("https://github.com/FriRebuild/fripack-inject/releases/download/v{}/{}", frida_version, filename);
        
        println!("{} {}", "→".blue(), format!("Downloading prebuilt file: {}", filename).blue());
        
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to download file: HTTP {}: {}", response.status(), url);
        }
        
        let total_size = response.content_length().unwrap_or(0);
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-")
        );
        
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();
        let mut data = Vec::new();
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            data.extend_from_slice(&chunk);
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }
        
        pb.finish_with_message("Download complete!");
        
        Ok(data)
    }
    
    pub async fn download_to_file(&self, url: &str, path: &Path) -> Result<()> {
        println!("{} {}", "→".blue(), format!("Downloading: {}", url).blue());
        
        let response = self.client.get(url).send().await?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to download file: HTTP {}: {}", response.status(), url);
        }
        
        let total_size = response.content_length().unwrap_or(0);
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-")
        );
        
        let mut file = File::create(path).await?;
        let mut downloaded = 0u64;
        let mut stream = response.bytes_stream();
        
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }
        
        file.flush().await?;
        pb.finish_with_message("Download complete!");
        
        println!("{} {}", "✓".green(), format!("Saved to: {}", path.display()).green());
        
        Ok(())
    }
    
    pub async fn get_available_releases(&self) -> Result<Vec<String>> {
        let url = "https://api.github.com/repos/FriRebuild/fripack-inject/releases";
        let response = self.client.get(url).send().await?;
        
        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch releases: HTTP {}: {}", response.status(), url);
        }
        
        let releases: Vec<serde_json::Value> = response.json().await?;
        let mut versions = Vec::new();
        
        for release in releases {
            if let Some(tag_name) = release.get("tag_name").and_then(|v| v.as_str()) {
                if let Some(version) = tag_name.strip_prefix('v') {
                    versions.push(version.to_string());
                }
            }
        }
        
        versions.sort_by(|a, b| b.cmp(a)); // Sort in descending order
        
        Ok(versions)
    }
}

impl Default for Downloader {
    fn default() -> Self {
        Self::new()
    }
}