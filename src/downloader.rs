use anyhow::Result;
use dirs;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, warn};
use reqwest::Client;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::config::{Platform, PlatformConfig};

pub struct Downloader {
    client: Client,
    cache_dir: PathBuf,
}

impl Downloader {
    pub fn new() -> Self {
        let cache_dir = get_cache_dir();
        Self {
            client: Client::new(),
            cache_dir,
        }
    }

    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    pub async fn ensure_cache_dir(&self) -> Result<()> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).await?;
            info!("✓ Created cache directory: {}", self.cache_dir.display());
        }
        Ok(())
    }

    fn get_cache_file_path(&self, platform: &PlatformConfig, frida_version: &str) -> PathBuf {
        let filename = self.get_prebuilt_file_name(platform, frida_version);
        self.cache_dir.join(filename)
    }

    async fn is_file_cached(&self, platform: &PlatformConfig, frida_version: &str) -> bool {
        let cache_path = self.get_cache_file_path(platform, frida_version);
        cache_path.exists()
    }

    async fn load_cached_file(
        &self,
        platform: &PlatformConfig,
        frida_version: &str,
    ) -> Result<Vec<u8>> {
        let cache_path = self.get_cache_file_path(platform, frida_version);
        info!("→ Loading from cache: {}", cache_path.display());
        Ok(fs::read(&cache_path).await?)
    }

    async fn save_to_cache(
        &self,
        platform: &PlatformConfig,
        frida_version: &str,
        data: &[u8],
    ) -> Result<()> {
        self.ensure_cache_dir().await?;
        let cache_path = self.get_cache_file_path(platform, frida_version);
        fs::write(&cache_path, data).await?;
        info!("→ Cached to: {}", cache_path.display());
        Ok(())
    }

    pub async fn list_cached_files(&self) -> Result<Vec<PathBuf>> {
        if !self.cache_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&self.cache_dir).await?;
        let mut files = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |ext| ext == "so") {
                files.push(path);
            }
        }

        Ok(files)
    }

    pub async fn clear_cache(&self) -> Result<usize> {
        if !self.cache_dir.exists() {
            warn!("Cache directory does not exist.");
            return Ok(0);
        }

        let files = self.list_cached_files().await?;
        let mut count = 0;

        for file in &files {
            fs::remove_file(file).await?;
            count += 1;
        }

        if count > 0 {
            info!("✓ Removed {} cached files", count);
        } else {
            warn!("No cached files to remove.");
        }

        Ok(count)
    }

    pub async fn get_cache_stats(&self) -> Result<CacheStats> {
        if !self.cache_dir.exists() {
            return Ok(CacheStats {
                file_count: 0,
                total_size: 0,
                files: Vec::new(),
            });
        }

        let files = self.list_cached_files().await?;
        let mut total_size = 0u64;
        let mut file_info = Vec::new();

        for file in &files {
            let metadata = fs::metadata(file).await?;
            let size = metadata.len();
            total_size += size;

            if let Some(filename) = file.file_name().and_then(|n| n.to_str()) {
                file_info.push(CachedFileInfo {
                    name: filename.to_string(),
                    size,
                    path: file.clone(),
                });
            }
        }

        Ok(CacheStats {
            file_count: files.len(),
            total_size,
            files: file_info,
        })
    }

    pub fn get_prebuilt_file_name(&self, platform: &PlatformConfig, frida_version: &str) -> String {
        format!(
            "fripack-inject-{}-{}.{}",
            frida_version,
            platform,
            if platform.platform == Platform::Windows {
                "dll"
            } else {
                "so"
            }
        )
    }

    pub fn get_prebuilt_file_url(
        &self,
        platform: &PlatformConfig,
        frida_version: &str,
    ) -> String {
        format!(
            "https://github.com/FriRebuild/fripack-inject/releases/download/{}/{}",
            frida_version,
            self.get_prebuilt_file_name(platform, frida_version)
        )
    }

    pub async fn download_prebuilt_file(
        &self,
        platform: &PlatformConfig,
        frida_version: &str,
    ) -> Result<Vec<u8>> {
        if self.is_file_cached(platform, frida_version).await {
            return self.load_cached_file(platform, frida_version).await;
        }

        let url = self.get_prebuilt_file_url(platform, frida_version);
        let filename = self.get_prebuilt_file_name(platform, frida_version);

        info!("→ Downloading prebuilt file: {}", filename);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to download file: HTTP {}: {}",
                response.status(),
                url
            );
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

        self.save_to_cache(platform, frida_version, &data).await?;

        Ok(data)
    }
}

#[derive(Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
}

impl Default for Downloader {
    fn default() -> Self {
        Self::new()
    }
}

fn get_cache_dir() -> PathBuf {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home_dir.join(".fripack")
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub file_count: usize,
    pub total_size: u64,
    pub files: Vec<CachedFileInfo>,
}

#[derive(Debug, Clone)]
pub struct CachedFileInfo {
    pub name: String,
    pub size: u64,
    pub path: PathBuf,
}
