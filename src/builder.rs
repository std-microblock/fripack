use crate::binary::{BinaryProcessor, EmbeddedConfig};
use crate::config::{ResolvedConfig, ResolvedTarget};
use crate::downloader::Downloader;
use anyhow::Result;
use log::{info, warn};
use rand::Rng;
use std::path::{Path, PathBuf};
use tempfile;
use tokio::{fs, process::Command};

pub struct Builder {
    config: ResolvedConfig,
    downloader: Downloader,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum Mode {
    EmbedJs = 1,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct EmbeddedConfigData {
    mode: Mode,
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
            None => {
                warn!(
                    "Target type not specified for target: {}, skipping...",
                    target_name
                );
                Ok(())
            }
        }
    }

    async fn generate_binary(&mut self, target: &ResolvedTarget) -> Result<Vec<u8>> {
        // Get required fields
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let frida_version = target
            .frida_version
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: fridaVersion"))?;
        let entry = target
            .entry
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: entry"))?;
        let use_xz = target.xz.unwrap_or(false);

        // Get prebuilt file data
        let prebuilt_data = if let Some(override_file) = &target.override_prebuild_file {
            info!("→ Using override prebuilt file: {}", override_file);
            fs::read(override_file).await?
        } else {
            info!("→ Downloading prebuilt file for platform: {}", platform);
            self.downloader
                .download_prebuilt_file(platform, frida_version)
                .await?
        };

        // Read entry file
        info!("→ Reading entry file: {}", entry);
        let entry_data = fs::read(entry).await?;

        // Process the binary
        info!("→ Processing binary...");
        let mut processor = BinaryProcessor::new(prebuilt_data)?;

        let config_data = EmbeddedConfigData {
            mode: Mode::EmbedJs,
            js_filepath: Some(entry.clone()),
            js_content: Some(String::from_utf8_lossy(&entry_data).to_string()),
        };

        let config_data = serde_json::to_string(&config_data)?;

        // Add embedded config section
        processor
            .add_embedded_config_data(config_data.as_bytes(), use_xz)
            .unwrap();

        let output_data = processor.into_data();

        Ok(output_data)
    }

    async fn build_android_so(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        info!("→ Building Android SO target: {}", target_name);

        let output_dir = target.output_dir.as_deref().unwrap_or("./fripack");

        let output_data = self.generate_binary(target).await?;
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let output_filename = format!("{}-{}.so", target_name, platform);
        let output_file_path = std::path::Path::new(output_dir).join(&output_filename);
        std::fs::create_dir_all(output_dir)?;
        fs::write(&output_file_path, output_data).await?;

        info!(
            "✓ Successfully built Android SO: {}",
            output_file_path.display()
        );

        Ok(())
    }

    async fn build_xposed(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        info!("→ Building Xposed target: {}", target_name);

        // Get required fields
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let package_name = target
            .package_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: packageName"))?;
        let name = target
            .name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: name"))?;
        let sign = target.sign.unwrap_or(false);
        let output_dir = target.output_dir.as_deref().unwrap_or("./fripack");
        let binary_data = self.generate_binary(target).await?;

        let random_so_name = format!("lib{}.so", generate_random_string(8));

        // 3. Create a temporary directory for the apktool project
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path();
        info!("→ Created temporary directory: {}", temp_path.display());

        // Move the generated .so file to the temporary directory for now
        let temp_so_path = temp_path.join(&random_so_name);
        fs::write(&temp_so_path, &binary_data).await?;

        // 4. Create assets/native_init and assets/xposed_init files
        let assets_dir = temp_path.join("assets");
        fs::create_dir_all(&assets_dir).await?;

        let native_init_path = assets_dir.join("native_init");
        fs::write(&native_init_path, &random_so_name).await?;
        info!("→ Created native_init: {}", native_init_path.display());

        // 5. Generate a random class name for the smali file
        let random_class_name =
            format!("{}{}", generate_random_string(4), generate_random_string(4)); // e.g., "abcdABCD"

        let xposed_init_path = assets_dir.join("xposed_init");
        let xposed_init_content = format!("{}.{}", package_name, random_class_name);
        fs::write(&xposed_init_path, &xposed_init_content).await?;
        info!("→ Created xposed_init: {}", xposed_init_path.display());

        // 6. Copy the generated .so file to lib/架构/libxxxx.so within the temporary directory.

        let lib_dir = temp_path.join("lib").join(platform);
        fs::create_dir_all(&lib_dir).await?;
        let dest_so_path = lib_dir.join(&random_so_name);
        fs::copy(&temp_so_path, &dest_so_path).await?;
        info!("→ Copied .so to: {}", dest_so_path.display());

        info!("✓ Successfully built Xposed module: {}", target_name);

        // 7. Create the smali/com/xx/xx/xx/随机类名.smali file
        let smali_dir_path = temp_path.join("smali").join(package_name.replace(".", "/"));
        fs::create_dir_all(&smali_dir_path).await?;

        let smali_file_path = smali_dir_path.join(format!("{}.smali", random_class_name));

        let smali_content = format!(
            r#".class public L{}/{};
.super Ljava/lang/Object;
.implements Lde/robv/android/xposed/IXposedHookLoadPackage;
.implements Lde/robv/android/xposed/IXposedHookZygoteInit;

.method public constructor <init>()V
    .locals 0
    invoke-direct {{p0}}, Ljava/lang/Object;-><init>()V
    return-void
.end method

.method public initZygote(Lde/robv/android/xposed/IXposedHookZygoteInit$StartupParam;)V
    .locals 3
    iget-object v0, p1, Lde/robv/android/xposed/IXposedHookZygoteInit$StartupParam;->modulePath:Ljava/lang/String;
    
    new-instance v1, Ljava/io/File;
    invoke-direct {{v1, v0}}, Ljava/io/File;-><init>(Ljava/lang/String;)V
    invoke-virtual {{v1}}, Ljava/io/File;->getParent()Ljava/lang/String;
    move-result-object v0

    new-instance v1, Ljava/lang/StringBuilder;
    invoke-direct {{v1}}, Ljava/lang/StringBuilder;-><init>()V
    invoke-virtual {{v1, v0}}, Ljava/lang/StringBuilder;->append(Ljava/lang/String;)Ljava/lang/StringBuilder;
    const-string v2, "/lib/{}/{}"
    invoke-virtual {{v1, v2}}, Ljava/lang/StringBuilder;->append(Ljava/lang/String;)Ljava/lang/StringBuilder;
    invoke-virtual {{v1}}, Ljava/lang/StringBuilder;->toString()Ljava/lang/String;
    move-result-object v1
    
    invoke-static {{v1}}, Ljava/lang/System;->load(Ljava/lang/String;)V
    return-void
.end method

.method public handleLoadPackage(Lde/robv/android/xposed/callbacks/XC_LoadPackage$LoadPackageParam;)V
    .locals 0
    return-void
.end method
"#,
            package_name.replace(".", "/"),
            random_class_name,
            platform.split("-").next().unwrap_or("arm64"),
            random_so_name
        );

        fs::write(&smali_file_path, smali_content.as_bytes()).await?;
        info!("→ Created smali file: {}", smali_file_path.display());

        // 8. Copy ic_launcher.webp and ic_launcher_round.webp if specified in the config.
        if let Some(icon_path_str) = &target.icon {
            let icon_path = PathBuf::from(icon_path_str);
            let res_mipmap_xxhdpi_dir = temp_path.join("res").join("mipmap-xxhdpi");
            fs::create_dir_all(&res_mipmap_xxhdpi_dir).await?;

            let launcher_icon_name = "ic_launcher.webp";
            let launcher_round_icon_name = "ic_launcher_round.webp";

            let src_launcher_path = icon_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(launcher_icon_name);
            let src_launcher_round_path = icon_path
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(launcher_round_icon_name);

            if src_launcher_path.exists() {
                let dest_launcher_path = res_mipmap_xxhdpi_dir.join(launcher_icon_name);
                fs::copy(&src_launcher_path, &dest_launcher_path).await?;
                info!("→ Copied launcher icon: {}", dest_launcher_path.display());
            }

            if src_launcher_round_path.exists() {
                let dest_launcher_round_path = res_mipmap_xxhdpi_dir.join(launcher_round_icon_name);
                fs::copy(&src_launcher_round_path, &dest_launcher_round_path).await?;
                info!(
                    "→ Copied round launcher icon: {}",
                    dest_launcher_round_path.display()
                );
            }
        }

        // 9. Modify AndroidManifest.xml based on the configuration.
        let manifest_path = temp_path.join("AndroidManifest.xml");

        let icon_attributes = if target.icon.is_some() {
            r#"android:icon="@mipmap/ic_launcher" android:roundIcon="@mipmap/ic_launcher_round""#
                .to_string()
        } else {
            "".to_string()
        };

        let xposed_description = target
            .description
            .as_deref()
            .unwrap_or("Easy example which makes the status bar clock red and adds a smiley");
        let xposed_scope = target
            .scope
            .as_deref()
            .unwrap_or("com.example.a;com.example.b");

        let manifest_content = format!(
            r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" android:compileSdkVersion="36" android:compileSdkVersionCodename="16" package="{}" platformBuildVersionCode="36" platformBuildVersionName="16">
    <application android:debuggable="true" android:extractNativeLibs="true"
                {} android:label="{}">
        <meta-data android:name="xposedmodule" android:value="true"/>
        <meta-data android:name="xposeddescription" android:value="{}"/>
        <meta-data android:name="xposedminversion" android:value="53"/>
        <meta-data android:name="xposedscope" android:value="{}"/>
    </application>
</manifest>"#,
            package_name, icon_attributes, name, xposed_description, xposed_scope
        );

        fs::write(&manifest_path, manifest_content.as_bytes()).await?;
        info!("→ Created AndroidManifest.xml: {}", manifest_path.display());

        // 10. Create apktool.yml with the specified content.
        let apktool_yml_path = temp_path.join("apktool.yml");
        let apktool_yml_content = r#"apkFileName: app-debug.apk
isFrameworkApk: false
usesFramework:
  ids:
  - 1
  tag: null
sdkInfo:
  minSdkVersion: 24
  targetSdkVersion: 26
packageInfo:
  forcedPackageId: 127
  renameManifestPackage: null
versionInfo:
  versionCode: 1
  versionName: 1.0
resourcesAreCompressed: false
sharedLibrary: false
sparseResources: true
unknownFiles:
doNotCompress:
- resources.arsc
- webp"#;

        fs::write(&apktool_yml_path, apktool_yml_content.as_bytes()).await?;
        info!("→ Created apktool.yml: {}", apktool_yml_path.display());

        // 11. Build the APK using apktool b.
        info!("→ Building APK with apktool b...");
        let output = tokio::process::Command::new("apktool")
            .arg("b")
            .arg(temp_path.to_str().unwrap())
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "apktool build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        info!("✓ APK built successfully with apktool b.");

        // 12. Sign the APK using apksigner.
        if sign {
            info!("→ Signing APK with apksigner...");
            let unsigned_apk_path = temp_path.join("dist").join("app-debug.apk");
            let signed_apk_path = temp_path
                .join("dist")
                .join(format!("{}-{}-signed.apk", target_name, platform));

            let keystore = target
                .keystore
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing required field: keystore"))?;
            let keystore_pass = target
                .keystore_pass
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing required field: keystorePass"))?;
            let keystore_alias = target
                .keystore_alias
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Missing required field: keystoreAlias"))?;

            let mut command = if cfg!(target_os = "windows") {
                let mut cmd = Command::new("cmd");
                cmd.arg("/C");
                cmd.arg("apksigner");
                cmd
            } else {
                Command::new("apksigner")
            };
            command
                .arg("sign")
                .arg("--ks")
                .arg(keystore)
                .arg("--ks-key-alias")
                .arg(keystore_alias)
                .arg("--ks-pass")
                .arg(format!("pass:{}", keystore_pass));

            let output = command
                .arg("--out")
                .arg(&signed_apk_path)
                .arg(&unsigned_apk_path)
                .output()
                .await?;

            if !output.status.success() {
                anyhow::bail!(
                    "apksigner failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            info!("✓ APK signed successfully with apksigner.");

            // 13. Copy the signed APK back to the desired location.
            let final_apk_name = format!("{}-{}.apk", target_name, platform);
            let final_apk_path = std::path::Path::new(&output_dir).join(&final_apk_name);
            std::fs::create_dir_all(output_dir)?;
            fs::copy(&signed_apk_path, &final_apk_path).await?;
            info!("✓ Copied signed APK to: {}", final_apk_path.display());
        } else {
            // If not signing, just copy the unsigned APK
            let unsigned_apk_path = temp_path.join("dist").join("app-debug.apk");
            let final_apk_name = format!("{}-{}.apk", target_name, platform);
            let final_apk_path = std::path::Path::new(&output_dir).join(&final_apk_name);
            std::fs::create_dir_all(output_dir)?;
            fs::copy(&unsigned_apk_path, &final_apk_path).await?;
            info!("✓ Copied APK to: {}", final_apk_path.display());
        }

        Ok(())
    }

    pub async fn build_all(&mut self) -> Result<()> {
        info!("Building all targets...");

        let targets: Vec<(String, ResolvedTarget)> = self
            .config
            .targets
            .iter()
            .map(|(name, target)| (name.clone(), target.clone()))
            .collect();

        for (target_name, target) in targets {
            self.build_target(&target_name, &target).await?;
        }

        info!("✓ All targets built successfully!");
        Ok(())
    }
}

fn generate_random_string(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}
