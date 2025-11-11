use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

macro_rules! merge_fields {
    ($self:expr, $other:expr, $($field:ident),*) => {
        $(
            if let Some(ref $field) = $other.$field {
                $self.$field = Some($field.clone());
            }
        )*
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignConfig {
    pub keystore: String,
    #[serde(rename = "keystorePass")]
    pub keystore_pass: String,
    #[serde(rename = "keystoreAlias")]
    pub keystore_alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InjectMode {
    #[serde(rename = "NativeAddNeeded")]
    NativeAddNeeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectApkConfig {
    #[serde(rename = "sourceApkPath")]
    pub source_apk_path: Option<String>,
    #[serde(rename = "sourceApkPackageName")]
    pub source_apk_package_name: Option<String>,
    #[serde(rename = "injectMode")]
    pub inject_mode: InjectMode,
    #[serde(rename = "targetLib")]
    pub target_lib: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FripackConfig {
    #[serde(flatten)]
    pub targets: HashMap<String, TargetConfig>,
}

impl FripackConfig {
    pub fn template() -> Self {
        let mut targets = HashMap::new();

        // Base configuration
        targets.insert(
            "base".to_string(),
            TargetConfig {
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
                name: None,
                icon: None,
                scope: None,
                description: None,
                sign: None,
                output_dir: None,
                target_base_name: None,
                before_build: None,
                after_build: None,
                inject_apk: None,
            },
        );

        // Example Xposed module
        targets.insert(
            "example-xposed".to_string(),
            TargetConfig {
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
                name: Some("My Xposed Module".to_string()),
                icon: Some("res\\icon.png".to_string()),
                scope: Some("com.example.a;com.example.b".to_string()),
                description: Some(
                    "Easy example which makes the status bar clock red and adds a smiley"
                        .to_string(),
                ),
                sign: Some(SignConfig {
                    keystore: "C:\\Users\\YourUser\\.android\\debug.keystore".to_string(),
                    keystore_pass: "android".to_string(),
                    keystore_alias: "androiddebugkey".to_string(),
                }),
                output_dir: None,
                target_base_name: None,
                before_build: None,
                after_build: None,
                inject_apk: None,
            },
        );

        // Example Android SO
        targets.insert(
            "example-android-so".to_string(),
            TargetConfig {
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
                name: None,
                icon: None,
                scope: None,
                description: None,
                sign: None,
                output_dir: None,
                target_base_name: None,
                before_build: None,
                after_build: None,
                inject_apk: None,
            },
        );

        // Example Inject APK
        targets.insert(
            "example-inject-apk".to_string(),
            TargetConfig {
                inherit: None,
                target_type: Some("inject-apk".to_string()),
                platform: Some("arm64-v8a".to_string()),
                version: Some("1.0.0".to_string()),
                frida_version: Some("17.5.1".to_string()),
                mode: Some("embedjs".to_string()),
                entry: Some("main.js".to_string()),
                xz: Some(false),
                override_prebuild_file: None,
                package_name: None,
                name: None,
                icon: None,
                scope: None,
                description: None,
                output_dir: None,
                target_base_name: None,
                before_build: None,
                after_build: None,
                inject_apk: Some(InjectApkConfig {
                    source_apk_path: None,
                    source_apk_package_name: Some("com.example.app".to_string()),
                    inject_mode: InjectMode::NativeAddNeeded,
                    target_lib: Some("libnative-lib.so".to_string()),
                }),
                sign: Some(SignConfig {
                    keystore: "C:\\Users\\YourUser\\.android\\debug.keystore".to_string(),
                    keystore_pass: "android".to_string(),
                    keystore_alias: "androiddebugkey".to_string(),
                }),
            },
        );

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
            anyhow::bail!("Cyclic inheritance detected for target: {name}");
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
                anyhow::bail!("Target not found: {inherit_name}");
            }
        }

        // Override with current target values
        resolved.merge_from(target);

        processing.remove(name);
        resolved_targets.insert(name.to_string(), resolved);

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub targets: HashMap<String, ResolvedTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetConfig {
    pub inherit: Option<String>,
    #[serde(rename = "type")]
    pub target_type: Option<String>,
    pub platform: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "fridaVersion")]
    pub frida_version: Option<String>,
    pub mode: Option<String>,
    pub entry: Option<String>,
    pub xz: Option<bool>,
    #[serde(rename = "overridePrebuildFile")]
    pub override_prebuild_file: Option<String>,
    #[serde(rename = "packageName")]
    pub package_name: Option<String>,
    pub sign: Option<SignConfig>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub scope: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "outputDir")]
    pub output_dir: Option<String>,
    #[serde(rename = "targetBaseName")]
    pub target_base_name: Option<String>,
    #[serde(rename = "beforeBuild")]
    pub before_build: Option<String>,
    #[serde(rename = "afterBuild")]
    pub after_build: Option<String>,
    #[serde(rename = "injectApk")]
    pub inject_apk: Option<InjectApkConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arch {
    Arm32,
    Arm64,
    X86,
    X86_64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Android,
    Windows,
    Linux,
    MacOS,
}

impl Platform {
    pub fn binary_ext(&self) -> &'static str {
        match self {
            Platform::Android => "so",
            Platform::Windows => "dll",
            Platform::Linux => "so",
            Platform::MacOS => "dylib",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlatformConfig {
    pub arch: Arch,
    pub platform: Platform,
}

impl std::fmt::Display for PlatformConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-{}",
            self.platform_str().unwrap(),
            self.frida_arch().unwrap(),
        )
    }
}

impl PlatformConfig {
    pub fn from_str(platform_desc: String) -> Result<Self> {
        let parts: Vec<&str> = platform_desc.split('-').collect();

        let (platform, arch) = match parts.as_slice() {
            ["android", "arm32"] => (Platform::Android, Arch::Arm32),
            ["android", "arm64"] => (Platform::Android, Arch::Arm64),
            ["android", "x86"] => (Platform::Android, Arch::X86),
            ["android", "x86_64"] => (Platform::Android, Arch::X86_64),
            ["android", "x64"] => (Platform::Android, Arch::X86_64),
            ["windows", "x86"] => (Platform::Windows, Arch::X86),
            ["windows", "x86_64"] => (Platform::Windows, Arch::X86_64),
            ["windows", "x64"] => (Platform::Windows, Arch::X86_64),
            ["linux", "x86"] => (Platform::Linux, Arch::X86),
            ["linux", "x86_64"] => (Platform::Linux, Arch::X86_64),
            ["linux", "x64"] => (Platform::Linux, Arch::X86_64),
            ["macos", "x86_64"] => (Platform::MacOS, Arch::X86_64),
            ["macos", "arm64"] => (Platform::MacOS, Arch::Arm64),
            _ => anyhow::bail!("Unsupported platform description: {platform_desc}"),
        };
        Ok(PlatformConfig { arch, platform })
    }

    pub fn android_abi(&self) -> Result<String> {
        match self.arch {
            Arch::Arm32 => Ok("armeabi-v7a".to_string()),
            Arch::Arm64 => Ok("arm64-v8a".to_string()),
            Arch::X86 => Ok("x86".to_string()),
            Arch::X86_64 => Ok("x86_64".to_string()),
        }
    }

    pub fn frida_arch(&self) -> Result<String> {
        match self.arch {
            Arch::Arm32 => Ok("arm".to_string()),
            Arch::Arm64 => Ok("arm64".to_string()),
            Arch::X86 => Ok("x86".to_string()),
            Arch::X86_64 => Ok("x86_64".to_string()),
        }
    }

    pub fn platform_str(&self) -> Result<String> {
        match self.platform {
            Platform::Android => Ok("android".to_string()),
            Platform::Windows => Ok("windows".to_string()),
            Platform::Linux => Ok("linux".to_string()),
            Platform::MacOS => Ok("macos".to_string()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedTarget {
    pub target_type: Option<String>,
    pub platform: Option<PlatformConfig>,
    pub version: Option<String>,
    pub frida_version: Option<String>,
    pub mode: Option<String>,
    pub entry: Option<String>,
    pub xz: Option<bool>,
    pub override_prebuild_file: Option<String>,
    pub package_name: Option<String>,
    pub sign: Option<SignConfig>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub scope: Option<String>,
    pub description: Option<String>,
    pub output_dir: Option<String>,
    pub target_base_name: Option<String>,
    pub before_build: Option<String>,
    pub after_build: Option<String>,
    pub inject_apk: Option<InjectApkConfig>,
}

impl ResolvedTarget {
    pub fn merge_from(&mut self, other: &TargetConfig) {
        merge_fields!(
            self,
            other,
            target_type,
            version,
            frida_version,
            mode,
            entry,
            xz,
            override_prebuild_file,
            package_name,
            name,
            icon,
            scope,
            description,
            sign,
            output_dir,
            target_base_name,
            before_build,
            after_build,
            inject_apk
        );

        if let Some(platform_str) = &other.platform {
            self.platform = Some(PlatformConfig::from_str(platform_str.clone()).unwrap());
        }
    }
}
