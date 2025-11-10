use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FripackConfig {
    #[serde(flatten)]
    pub targets: HashMap<String, TargetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    /// Inherit from another target configuration
    pub inherit: Option<String>,
    
    /// Target type (android-so, xposed, etc.)
    #[serde(rename = "type")]
    pub target_type: Option<String>,
    
    /// Target platform (arm64-v8a, x86_64, etc.)
    pub platform: Option<String>,
    
    /// Version
    pub version: Option<String>,
    
    /// Frida version
    #[serde(rename = "fridaVersion")]
    pub frida_version: Option<String>,
    
    /// Mode (embedjs, etc.)
    pub mode: Option<String>,
    
    /// Entry file
    pub entry: Option<String>,
    
    /// Whether to use XZ compression
    pub xz: Option<bool>,
    
    /// Override prebuilt file path
    #[serde(rename = "overridePrebuildFile")]
    pub override_prebuild_file: Option<String>,
    
    /// Package name (for Xposed modules)
    #[serde(rename = "packageName")]
    pub package_name: Option<String>,
    
    /// Keystore path (for Xposed modules)
    pub keystore: Option<String>,
    
    /// Module name (for Xposed modules)
    pub name: Option<String>,
    
    /// Icon path (for Xposed modules)
    pub icon: Option<String>,
}

impl FripackConfig {
    pub fn template() -> Self {
        let mut targets = HashMap::new();
        
        // Base configuration
        targets.insert("base".to_string(), TargetConfig {
            inherit: None,
            target_type: None,
            platform: None,
            version: Some("1.0.0".to_string()),
            frida_version: Some("17.5.1".to_string()),
            mode: Some("embedjs".to_string()),
            entry: Some("main.js".to_string()),
            xz: Some(false),
            override_prebuild_file: None,
            package_name: None,
            keystore: None,
            name: None,
            icon: None,
        });
        
        // Example Xposed module
        targets.insert("example-xposed".to_string(), TargetConfig {
            inherit: None,
            target_type: Some("xposed".to_string()),
            platform: Some("arm64-v8a".to_string()),
            version: Some("1.0.0".to_string()),
            frida_version: None,
            mode: None,
            entry: None,
            xz: None,
            override_prebuild_file: None,
            package_name: Some("com.example.myxposedmodule".to_string()),
            keystore: Some("C:\\Users\\YourUser\\.android\\debug.keystore".to_string()),
            name: Some("My Xposed Module".to_string()),
            icon: Some("res\\icon.png".to_string()),
        });
        
        // Example Android SO
        targets.insert("example-android-so".to_string(), TargetConfig {
            inherit: Some("base".to_string()),
            target_type: Some("android-so".to_string()),
            platform: Some("arm64-v8a".to_string()),
            version: None,
            frida_version: None,
            mode: None,
            entry: None,
            xz: None,
            override_prebuild_file: Some("./libfripack-inject.so".to_string()),
            package_name: None,
            keystore: None,
            name: None,
            icon: None,
        });
        
        Self { targets }
    }
    
    pub fn resolve_inheritance(&self) -> Result<ResolvedConfig> {
        let mut resolved_targets = HashMap::new();
        let mut processing = std::collections::HashSet::new();
        
        for (name, target) in &self.targets {
            self.resolve_target(name, target, &mut resolved_targets, &mut processing)?;
        }
        
        Ok(ResolvedConfig {
            targets: resolved_targets,
        })
    }
    
    fn resolve_target(
        &self,
        name: &str,
        target: &TargetConfig,
        resolved_targets: &mut HashMap<String, ResolvedTarget>,
        processing: &mut std::collections::HashSet<String>,
    ) -> Result<()> {
        if resolved_targets.contains_key(name) {
            return Ok(());
        }
        
        if processing.contains(name) {
            anyhow::bail!("Cyclic inheritance detected for target: {}", name);
        }
        
        processing.insert(name.to_string());
        
        let mut resolved = ResolvedTarget::default();
        
        // Resolve inheritance first
        if let Some(inherit_name) = &target.inherit {
            if let Some(parent_target) = self.targets.get(inherit_name) {
                self.resolve_target(inherit_name, parent_target, resolved_targets, processing)?;
                if let Some(parent_resolved) = resolved_targets.get(inherit_name) {
                    resolved = parent_resolved.clone();
                }
            } else {
                anyhow::bail!("Target not found: {}", inherit_name);
            }
        }
        
        // Override with current target values
        if let Some(target_type) = &target.target_type {
            resolved.target_type = Some(target_type.clone());
        }
        if let Some(platform) = &target.platform {
            resolved.platform = Some(platform.clone());
        }
        if let Some(version) = &target.version {
            resolved.version = Some(version.clone());
        }
        if let Some(frida_version) = &target.frida_version {
            resolved.frida_version = Some(frida_version.clone());
        }
        if let Some(mode) = &target.mode {
            resolved.mode = Some(mode.clone());
        }
        if let Some(entry) = &target.entry {
            resolved.entry = Some(entry.clone());
        }
        if let Some(xz) = target.xz {
            resolved.xz = xz;
        }
        if let Some(override_prebuild_file) = &target.override_prebuild_file {
            resolved.override_prebuild_file = Some(override_prebuild_file.clone());
        }
        if let Some(package_name) = &target.package_name {
            resolved.package_name = Some(package_name.clone());
        }
        if let Some(keystore) = &target.keystore {
            resolved.keystore = Some(keystore.clone());
        }
        if let Some(name) = &target.name {
            resolved.name = Some(name.clone());
        }
        if let Some(icon) = &target.icon {
            resolved.icon = Some(icon.clone());
        }
        
        processing.remove(name);
        resolved_targets.insert(name.to_string(), resolved);
        
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub targets: HashMap<String, ResolvedTarget>,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedTarget {
    pub target_type: Option<String>,
    pub platform: Option<String>,
    pub version: Option<String>,
    pub frida_version: Option<String>,
    pub mode: Option<String>,
    pub entry: Option<String>,
    pub xz: bool,
    pub override_prebuild_file: Option<String>,
    pub package_name: Option<String>,
    pub keystore: Option<String>,
    pub name: Option<String>,
    pub icon: Option<String>,
}