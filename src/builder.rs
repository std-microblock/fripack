use crate::binary::BinaryProcessor;
use crate::config::{Platform, ResolvedConfig, ResolvedTarget};
use crate::downloader::Downloader;
use anyhow::Result;
use log::{info, warn};
use rand::Rng;
use std::path::{Path, PathBuf};
use tokio::{fs, process::Command};

pub struct Builder {
    config: ResolvedConfig,
    downloader: Downloader,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum Mode {
    EmbedJs = 1,
    Noop = 2,
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
        // Run beforeBuild hook
        if let Some(cmd) = &target.before_build {
            self.run_hook(cmd).await?;
        }

        let build_result = match target.target_type.as_deref() {
            Some("shared") => self.build_shared(target_name, target).await,
            Some("xposed") => self.build_xposed(target_name, target).await,
            Some("inject-apk") => self.build_inject_apk(target_name, target).await,
            Some("zygisk") => self.build_zygisk(target_name, target).await,
            Some(other) => anyhow::bail!("Unsupported target type: {other}"),
            None => {
                warn!("Target type not specified for target: {target_name}, skipping...");
                Ok(())
            }
        };

        // Run afterBuild hook if build succeeded
        if build_result.is_ok() {
            if let Some(cmd) = &target.after_build {
                self.run_hook(cmd).await?;
            }
        }

        build_result
    }

    async fn run_hook(&self, cmd: &str) -> Result<()> {
        info!("→ Running build hook: {}", cmd);
        let output = if cfg!(target_os = "windows") {
            Command::new("cmd").arg("/C").arg(cmd).output().await
        } else {
            Command::new("sh").arg("-c").arg(cmd).output().await
        }?;

        if !output.status.success() {
            anyhow::bail!(
                "Build hook failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
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
            info!("→ Using override prebuilt file: {override_file}");

            if !override_file.ends_with(platform.platform.binary_ext()) {
                anyhow::bail!(
                    "Override prebuilt file extension {} does not match the platform expected extension: {}",
                    override_file,
                    platform.platform.binary_ext()
                );
            }

            fs::read(override_file).await?
        } else {
            info!("→ Downloading prebuilt file for platform: {platform:?}");
            self.downloader
                .download_prebuilt_file(platform, frida_version)
                .await?
        };

        // Read entry file
        info!("→ Reading entry file: {entry}");
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

        processor.anti_anti_frida()?;

        let output_data = processor.into_data();

        Ok(output_data)
    }

    async fn build_shared(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        let base_name = target.target_base_name.as_deref().unwrap_or(target_name);
        info!("→ Building Shared Library target: {target_name} (base name: {base_name})");

        let output_dir = target.output_dir.as_deref().unwrap_or("./fripack");

        let output_data = self.generate_binary(target).await?;
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let output_filename = format!("{base_name}-{platform}.{}", platform.platform.binary_ext());
        let output_file_path = std::path::Path::new(output_dir).join(&output_filename);
        std::fs::create_dir_all(output_dir)?;
        fs::write(&output_file_path, output_data).await?;

        info!(
            "✓ Successfully built shared library: {}",
            output_file_path.display()
        );

        Ok(())
    }

    async fn build_xposed(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        let base_name = target.target_base_name.as_deref().unwrap_or(target_name);
        info!("→ Building Xposed target: {target_name} (base name: {base_name})");

        // Get required fields
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let xposed_config = target
            .xposed
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: xposed"))?;
        let package_name = xposed_config
            .package_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: packageName"))?;
        let name = xposed_config
            .name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: name"))?;

        if platform.platform != Platform::Android {
            anyhow::bail!("Xposed target only supports Android platform");
        }

        let sign = target.sign.is_some();
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
        let xposed_init_content = format!("{package_name}.{random_class_name}");
        fs::write(&xposed_init_path, &xposed_init_content).await?;
        info!("→ Created xposed_init: {}", xposed_init_path.display());

        // 6. Copy the generated .so file to lib/架构/libxxxx.so within the temporary directory.

        let lib_dir = temp_path.join("lib").join(platform.android_abi()?);
        fs::create_dir_all(&lib_dir).await?;
        let dest_so_path = lib_dir.join(&random_so_name);
        fs::copy(&temp_so_path, &dest_so_path).await?;
        info!("→ Copied .so to: {}", dest_so_path.display());

        info!("✓ Successfully built Xposed module: {target_name}");

        // 7. Create the smali/com/xx/xx/xx/随机类名.smali file
        let smali_dir_path = temp_path.join("smali").join(package_name.replace(".", "/"));
        fs::create_dir_all(&smali_dir_path).await?;

        let smali_file_path = smali_dir_path.join(format!("{random_class_name}.smali"));

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
            platform.frida_arch()?,
            random_so_name
        );

        fs::write(&smali_file_path, smali_content.as_bytes()).await?;
        info!("→ Created smali file: {}", smali_file_path.display());

        // 8. Copy ic_launcher.webp and ic_launcher_round.webp if specified in the config.
        if let Some(icon_path_str) = xposed_config.icon.as_ref() {
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

        let icon_attributes = if xposed_config.icon.is_some() {
            r#"android:icon="@mipmap/ic_launcher" android:roundIcon="@mipmap/ic_launcher_round""#
                .to_string()
        } else {
            "".to_string()
        };

        let xposed_description = xposed_config
            .description
            .as_deref()
            .unwrap_or("Easy example which makes the status bar clock red and adds a smiley");
        let xposed_scope = xposed_config
            .scope
            .as_deref()
            .unwrap_or("com.example.a;com.example.b");

        let manifest_content = format!(
            r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android" android:compileSdkVersion="36" android:compileSdkVersionCodename="16" package="{package_name}" platformBuildVersionCode="36" platformBuildVersionName="16">
    <application android:debuggable="true" android:extractNativeLibs="true"
                {icon_attributes} android:label="{name}">
        <meta-data android:name="xposedmodule" android:value="true"/>
        <meta-data android:name="xposeddescription" android:value="{xposed_description}"/>
        <meta-data android:name="xposedminversion" android:value="53"/>
        <meta-data android:name="xposedscope" android:value="{xposed_scope}"/>
    </application>
</manifest>"#
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
            .arg("-o")
            .arg(temp_path.join("dist").join("app-debug.apk"))
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
                .join(format!("{base_name}-{platform}-signed.apk"));

            let sign_config = target.sign.as_ref().unwrap();
            let keystore = &sign_config.keystore;
            let keystore_pass = &sign_config.keystore_pass;
            let keystore_alias = &sign_config.keystore_alias;

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
                .arg(format!("pass:{keystore_pass}"));

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
            let final_apk_name = format!("{base_name}-{platform}.apk");
            let final_apk_path = std::path::Path::new(&output_dir).join(&final_apk_name);
            std::fs::create_dir_all(output_dir)?;
            fs::copy(&signed_apk_path, &final_apk_path).await?;
            info!("✓ Copied signed APK to: {}", final_apk_path.display());
        } else {
            // If not signing, just copy the unsigned APK
            let unsigned_apk_path = temp_path.join("dist").join("app-debug.apk");
            let final_apk_name = format!("{base_name}-{platform}.apk");
            let final_apk_path = std::path::Path::new(&output_dir).join(&final_apk_name);
            std::fs::create_dir_all(output_dir)?;
            fs::copy(&unsigned_apk_path, &final_apk_path).await?;
            info!("✓ Copied APK to: {}", final_apk_path.display());
        }

        Ok(())
    }

    async fn build_inject_apk(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        let base_name = target.target_base_name.as_deref().unwrap_or(target_name);
        info!("→ Building Inject APK target: {target_name} (base name: {base_name})");

        // Get required fields
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;

        if platform.platform != Platform::Android {
            anyhow::bail!("Inject APK target only supports Android platform");
        }

        let inject_config = target
            .inject_apk
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: injectApk"))?;

        // Validate that either sourceApkPath or sourceApkPackageName is provided
        if inject_config.source_apk_path.is_none()
            && inject_config.source_apk_package_name.is_none()
        {
            anyhow::bail!("Either sourceApkPath or sourceApkPackageName must be provided");
        }

        let output_dir = target.output_dir.as_deref().unwrap_or("./fripack");
        let injected_binary_data = self.generate_binary(target).await?;

        // Get source APK path (either from path or extract from device)
        let source_apk_path = if let Some(apk_path) = &inject_config.source_apk_path {
            info!("→ Using source APK path: {apk_path}");
            PathBuf::from(apk_path)
        } else {
            let package_name = inject_config.source_apk_package_name.as_ref().unwrap();
            info!("→ Extracting APK from device for package: {package_name}");
            self.extract_apk_from_device(package_name).await?
        };

        // Create temporary directory for APK manipulation
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path();
        info!("→ Created temporary directory: {}", temp_path.display());

        // Decompile APK using apktool
        let decompiled_dir = temp_path.join("decompiled");
        info!("→ Decompiling APK with apktool...");
        let output = tokio::process::Command::new("apktool")
            .arg("d")
            .arg("-f")
            .arg("-r")
            .arg("-s")
            .arg(&source_apk_path)
            .arg("-o")
            .arg(&decompiled_dir)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "apktool decompile failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        info!("✓ APK decompiled successfully");

        // Find target native library
        let lib_dir = decompiled_dir.join("lib").join(platform.android_abi()?);
        let target_lib_path = self
            .find_target_library(&lib_dir, &inject_config.target_lib)
            .await?;

        info!("→ Selected target library: {}", target_lib_path.display());

        // Read the target library
        let mut target_lib_data = fs::read(&target_lib_path).await?;

        // Inject our library using ELF manipulation
        let inject_lib_name = format!("lib{}.so", generate_random_string(8));
        info!("→ Injecting library as: {}", inject_lib_name);
        let mut processor = BinaryProcessor::new(target_lib_data.clone())?;
        processor.add_needed_library(&inject_lib_name)?;
        target_lib_data = processor.into_data();

        // Write the modified library back
        fs::write(&target_lib_path, &target_lib_data).await?;
        fs::write(
            Path::new(&target_lib_path)
                .parent()
                .unwrap()
                .join(&inject_lib_name),
            &injected_binary_data,
        )
        .await?;
        info!("→ Modified library written back");

        // Add our native lib path into the do_not_compress list in apktool.yml
        let apktool_yml_path = decompiled_dir.join("apktool.yml");
        let apktool_yml_content = fs::read_to_string(&apktool_yml_path).await?;
        let mut apktool_yml: serde_yaml::Value = serde_yaml::from_str(&apktool_yml_content)?;

        let inject_lib_relpath = format!("lib/{}/{}", platform.android_abi()?, inject_lib_name);
        if let Some(do_not_compress) = apktool_yml
            .get_mut("doNotCompress")
            .and_then(|v| v.as_sequence_mut())
        {
            do_not_compress.push(serde_yaml::Value::String(inject_lib_relpath));
        } else {
            apktool_yml["doNotCompress"] =
                serde_yaml::Value::Sequence(vec![serde_yaml::Value::String(inject_lib_relpath)]);
        }

        let apktool_yml_serialized = serde_yaml::to_string(&apktool_yml)?;
        fs::write(&apktool_yml_path, apktool_yml_serialized).await?;
        info!("→ Updated apktool.yml to avoid compressing injected library");

        // Rebuild APK using apktool
        info!("→ Rebuilding APK with apktool...");
        let rebuilt_apk_path = decompiled_dir.join("dist").join("app-debug.apk");
        let output = tokio::process::Command::new("apktool")
            .arg("b")
            .arg(&decompiled_dir)
            .arg("-o")
            .arg(&rebuilt_apk_path)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "apktool build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        info!("✓ APK rebuilt successfully with apktool");

        // Run zipalign on the rebuilt APK
        info!("→ Aligning APK with zipalign...");
        let aligned_apk_path = temp_path.join(format!("{base_name}-{platform}-aligned.apk"));
        let output = tokio::process::Command::new("zipalign")
            .arg("-v")
            .arg("-p")
            .arg("4")
            .arg(&rebuilt_apk_path)
            .arg(&aligned_apk_path)
            .output()
            .await?;
        let rebuilt_apk_path = if output.status.success() {
            info!("✓ APK aligned successfully");
            aligned_apk_path
        } else {
            warn!(
                "zipalign failed: {}, proceeding with unaligned APK. Apk may not install with reason 'INSTALL_FAILED_INVALID_APK: Failed to extract native libraries' for some applications.",
                String::from_utf8_lossy(&output.stderr)
            );
            rebuilt_apk_path
        };

        // Copy the rebuilt APK to output directory
        let final_apk_name = format!("{base_name}-{platform}-injected.apk");
        let final_apk_path = Path::new(output_dir).join(&final_apk_name);
        std::fs::create_dir_all(output_dir)?;

        // Sign the APK if signing configuration is provided
        if let Some(sign_config) = &target.sign {
            info!("→ Signing APK...");
            let signed_apk_path = temp_path.join(format!("{base_name}-{platform}-signed.apk"));

            let mut command = if cfg!(target_os = "windows") {
                let mut cmd = Command::new("cmd");
                cmd.arg("/C");
                cmd.arg("apksigner");
                cmd
            } else {
                Command::new("apksigner")
            };

            let output = command
                .arg("sign")
                .arg("--ks")
                .arg(&sign_config.keystore)
                .arg("--ks-key-alias")
                .arg(&sign_config.keystore_alias)
                .arg("--ks-pass")
                .arg(format!("pass:{}", sign_config.keystore_pass))
                .arg("--out")
                .arg(&signed_apk_path)
                .arg(&rebuilt_apk_path)
                .output()
                .await?;

            if !output.status.success() {
                anyhow::bail!(
                    "apksigner failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }

            // Copy signed APK to final location
            fs::copy(&signed_apk_path, &final_apk_path).await?;
            info!("✓ APK signed successfully");
        } else {
            fs::copy(&rebuilt_apk_path, &final_apk_path).await?;
        }

        info!(
            "✓ Successfully built inject APK: {}",
            final_apk_path.display()
        );
        Ok(())
    }

    async fn build_zygisk(&mut self, target_name: &str, target: &ResolvedTarget) -> Result<()> {
        let base_name = target.target_base_name.as_deref().unwrap_or(target_name);
        info!("→ Building Zygisk target: {target_name} (base name: {base_name})");

        // Get required fields
        let platform = target
            .platform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: platform"))?;
        let zygisk_config = target
            .zygisk
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: zygisk"))?;

        let scope = zygisk_config
            .scope
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: zygisk.scope"))?;

        let id = zygisk_config
            .id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: zygisk.id"))?;

        let name = zygisk_config
            .name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing required field: zygisk.name"))?;

        if platform.platform != Platform::Android {
            anyhow::bail!("Zygisk target only supports Android platform");
        }

        let output_dir = target.output_dir.as_deref().unwrap_or("./fripack");
        let binary_data = self.generate_binary(target).await?;

        // Create the final zip file directly
        let zip_filename = format!("{}-zygisk-{}.zip", base_name, platform);
        let zip_path = Path::new(output_dir).join(&zip_filename);
        std::fs::create_dir_all(output_dir)?;

        // Create zip file
        info!("→ Creating zygisk module zip: {}", zip_path.display());
        let zip_file = std::fs::File::create(&zip_path)?;
        let mut zip = zip::ZipWriter::new(zip_file);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);

        use std::io::Write;

        // Add fripack/inject.so
        zip.start_file("fripack/inject.so", options)?;
        zip.write_all(&binary_data)?;

        // Add fripack/scope
        let scope_content = scope
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        zip.start_file("fripack/scope", options)?;
        zip.write_all(scope_content.as_bytes())?;

        // Add module.prop
        let module_prop_content = format!(
            "id={}\nname={}\nversion={}\nversionCode={}\nauthor={}\ndescription={}\n",
            id,
            name,
            zygisk_config.version.as_deref().unwrap_or("1.0"),
            zygisk_config.version_code.unwrap_or(1),
            zygisk_config.author.as_deref().unwrap_or("FriPack"),
            zygisk_config
                .description
                .as_deref()
                .unwrap_or("A Zygisk module created by FriPack")
        );
        zip.start_file("module.prop", options)?;
        zip.write_all(module_prop_content.as_bytes())?;

        zip.start_file(&format!("zygisk/{}.so", platform.android_abi()?), options)?;
        let downloaded = self
            .downloader
            .download_zygisk_loader(&platform.android_abi()?)
            .await?;

        zip.write_all(&downloaded)?;

        zip.finish()?;

        info!("✓ Successfully built zygisk module: {}", zip_path.display());

        Ok(())
    }

    async fn extract_apk_from_device(&self, package_name: &str) -> Result<PathBuf> {
        let cache_dir = Path::new("./fripack_cache").join("apks");
        std::fs::create_dir_all(&cache_dir)?;

        let cached_apk_path = cache_dir.join(format!("{}.apk", package_name.replace(":", "_")));

        // Check if APK is already cached
        if cached_apk_path.exists() {
            info!("→ Using cached APK: {}", cached_apk_path.display());
            return Ok(cached_apk_path);
        }

        // Get APK path from device
        info!("→ Getting APK path from device...");
        let output = tokio::process::Command::new("adb")
            .arg("shell")
            .arg("pm")
            .arg("path")
            .arg(package_name)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to get APK path from device: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let apk_path_line = stdout
            .lines()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No APK path returned"))?;

        let device_apk_path = apk_path_line
            .strip_prefix("package:")
            .ok_or_else(|| anyhow::anyhow!("Invalid APK path format"))?;

        // Pull APK from device
        info!("→ Pulling APK from device: {}", device_apk_path);
        let output = tokio::process::Command::new("adb")
            .arg("pull")
            .arg(device_apk_path)
            .arg(&cached_apk_path)
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to pull APK from device: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        info!("✓ APK extracted and cached: {}", cached_apk_path.display());
        Ok(cached_apk_path)
    }

    async fn find_target_library(
        &self,
        lib_dir: &Path,
        target_lib: &Option<String>,
    ) -> Result<PathBuf> {
        if !lib_dir.exists() {
            anyhow::bail!("Library directory does not exist: {}", lib_dir.display());
        }

        // If target_lib is specified, try to find it
        if let Some(target_name) = target_lib {
            let target_path = lib_dir.join(target_name);
            if target_path.exists() {
                return Ok(target_path);
            }
            anyhow::bail!("Target library not found: {}", target_path.display());
        }

        // Search for libraries in whitelist
        let whitelist = ["libCrashSight.so", "libBugly.so", "libmmkv.so"];
        for lib_name in &whitelist {
            let lib_path = lib_dir.join(lib_name);
            if lib_path.exists() {
                info!("→ Found whitelist library: {}", lib_name);
                return Ok(lib_path);
            }
        }

        // If no whitelist library found, find the smallest .so file
        warn!("No whitelist library found, searching for smallest .so file");
        let mut entries = tokio::fs::read_dir(lib_dir).await?;
        let mut smallest_lib: Option<(PathBuf, u64)> = None;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("so") {
                let metadata = entry.metadata().await?;
                let size = metadata.len();

                if let Some((_, smallest_size)) = &smallest_lib {
                    if size < *smallest_size {
                        smallest_lib = Some((path, size));
                    }
                } else {
                    smallest_lib = Some((path, size));
                }
            }
        }

        if let Some((lib_path, size)) = smallest_lib {
            warn!(
                "→ Selected smallest library: {} ({} bytes)",
                lib_path.display(),
                size
            );
            Ok(lib_path)
        } else {
            anyhow::bail!("No .so files found in library directory");
        }
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
