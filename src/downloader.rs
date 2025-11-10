use anyhow::Result;
use colored::*;
use dirs;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

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
            println!(
                "{} {}",
                "✓".green(),
                format!("Created cache directory: {}", self.cache_dir.display()).green()
            );
        }
        Ok(())
    }

    fn get_cache_file_path(&self, platform: &str, frida_version: &str) -> PathBuf {
        let filename = format!("fripack-inject-{}-{}.so", platform, frida_version);
        self.cache_dir.join(filename)
    }

    async fn is_file_cached(&self, platform: &str, frida_version: &str) -> bool {
        let cache_path = self.get_cache_file_path(platform, frida_version);
        cache_path.exists()
    }

    async fn load_cached_file(&self, platform: &str, frida_version: &str) -> Result<Vec<u8>> {
        let cache_path = self.get_cache_file_path(platform, frida_version);
        println!(
            "{} {}",
            "→".blue(),
            format!("Loading from cache: {}", cache_path.display()).blue()
        );
        Ok(fs::read(&cache_path).await?)
    }

    async fn save_to_cache(&self, platform: &str, frida_version: &str, data: &[u8]) -> Result<()> {
        self.ensure_cache_dir().await?;
        let cache_path = self.get_cache_file_path(platform, frida_version);
        fs::write(&cache_path, data).await?;
        println!(
            "{} {}",
            "→".blue(),
            format!("Cached to: {}", cache_path.display()).blue()
        );
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
            println!("{}", "Cache directory does not exist.".yellow());
            return Ok(0);
        }

        let files = self.list_cached_files().await?;
        let mut count = 0;

        for file in &files {
            fs::remove_file(file).await?;
            count += 1;
        }

        if count > 0 {
            println!(
                "{} {}",
                "✓".green(),
                format!("Removed {} cached files", count).green()
            );
        } else {
            println!("{}", "No cached files to remove.".yellow());
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

    pub async fn download_prebuilt_file(
        &self,
        platform: &str,
        frida_version: &str,
    ) -> Result<Vec<u8>> {
        if self.is_file_cached(platform, frida_version).await {
            return self.load_cached_file(platform, frida_version).await;
        }

        let files = self.get_release_files(frida_version).await?;

        let matched_file = self.find_matching_file(&files, platform, frida_version)?;

        let url = matched_file.download_url;
        let filename = matched_file.name;

        println!(
            "{} {}",
            "→".blue(),
            format!("Downloading prebuilt file: {}", filename).blue()
        );

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

    pub async fn download_to_file(&self, url: &str, path: &Path) -> Result<()> {
        println!("{} {}", "→".blue(), format!("Downloading: {}", url).blue());

        let response = self.client.get(url).send().await?;

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

        println!(
            "{} {}",
            "✓".green(),
            format!("Saved to: {}", path.display()).green()
        );

        Ok(())
    }

    pub async fn get_available_releases(&self) -> Result<Vec<String>> {
        let url = "https://api.github.com/repos/FriRebuild/fripack-inject/releases";
        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch releases: HTTP {}: {}",
                response.status(),
                url
            );
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

        versions.sort_by(|a, b| b.cmp(a));

        Ok(versions)
    }

    pub async fn get_release_files(&self, frida_version: &str) -> Result<Vec<ReleaseAsset>> {
        let url = format!(
            "https://api.github.com/repos/FriRebuild/fripack-inject/releases/tags/{}",
            frida_version
        );
        let response = self
            .client
            .get(&url)
            .header("User-Agent", "fripack-downloader")
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Failed to fetch release: HTTP {}: {}",
                response.status(),
                url
            );
        }

        let release: serde_json::Value = response.json().await?;
        let assets = release
            .get("assets")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("No assets found in release"))?;

        let mut files = Vec::new();
        for asset in assets {
            if let (Some(name), Some(download_url)) = (
                asset.get("name").and_then(|v| v.as_str()),
                asset.get("browser_download_url").and_then(|v| v.as_str()),
            ) {
                files.push(ReleaseAsset {
                    name: name.to_string(),
                    download_url: download_url.to_string(),
                });
            }
        }

        Ok(files)
    }

    fn find_matching_file(
        &self,
        files: &[ReleaseAsset],
        platform: &str,
        frida_version: &str,
    ) -> Result<ReleaseAsset> {
        let platform_mappings = std::collections::HashMap::from([
            ("arm64-v8a", vec!["android-arm64", "arm64"]),
            ("armeabi-v7a", vec!["android-arm", "arm"]),
            ("x86", vec!["android-x86", "x86"]),
            ("x86_64", vec!["android-x86_64", "x86_64"]),
            ("linux-x86_64", vec!["linux-x86_64"]),
        ]);

        let platform_keywords = platform_mappings
            .get(platform)
            .unwrap_or(&vec![platform])
            .clone();

        for file in files {
            let filename = file.name.to_lowercase();
            let version_lower = frida_version.to_lowercase();

            if filename.contains(&version_lower) {
                for keyword in &platform_keywords {
                    if filename.contains(&keyword.to_lowercase()) {
                        return Ok(file.clone());
                    }
                }
            }
        }

        for file in files {
            let filename = file.name.to_lowercase();

            for keyword in &platform_keywords {
                if filename.contains(&keyword.to_lowercase()) {
                    println!(
                        "{} {}",
                        "⚠".yellow(),
                        format!(
                            "Warning: Found platform match but version may not match exactly: {}",
                            file.name
                        )
                        .yellow()
                    );
                    return Ok(file.clone());
                }
            }
        }

        for file in files {
            if file.name.ends_with(".so") {
                println!(
                    "{} {}",
                    "⚠".yellow(),
                    format!(
                        "Warning: Using fallback file (no platform match): {}",
                        file.name
                    )
                    .yellow()
                );
                return Ok(file.clone());
            }
        }

        anyhow::bail!(
            "No matching file found for platform: {} and version: {}",
            platform,
            frida_version
        )
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
