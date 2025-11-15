# Fripack  
### Package your Frida script into an executable.

[中文](./README_zh.md)

<img width="400" alt="image" src="https://github.com/user-attachments/assets/5a00307c-fd30-4991-a82e-2b23f3d115b7" />

Frida is a powerful tool, but its size and the need for root access make it challenging to distribute scripts to end-users. This often limits Frida’s use in developing plugins for wider audiences.

Fripack solves this by packaging your Frida scripts into various executable formats—such as Xposed Modules, patched apks, shared objects for `LD_PRELOAD`, or injectable DLLs—enabling easy distribution and use of Frida-based plugins.

### Binary Size Matters

The original Frida project comes with significant bulk. Fripack streamlines and compresses Frida, resulting in binary outputs under 10 MB on all platforms—except Linux.

<img width="345" height="168" alt="image" src="https://github.com/user-attachments/assets/bf6f1134-f9a0-4d31-b15a-e49ae5c545d8" />

### One-Click Build

Cross-platform Frida scripts often make it cumbersome to build deliverables for different targets—even with Frida Gadget. Fripack simplifies development by enabling one-command builds for multiple platforms at once.

## Installation

Download the latest binary from the [releases page](https://github.com/std-microblock/fripack/releases/latest) and install it as needed.

## Getting Started

### Basic Configuration

Fripack uses a configuration file named `fripack.json`, which supports JSON5 syntax. Here’s a basic example:

```json
{
    "xposed": {
        "type": "xposed",
        "version": "1.0.0",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "platform": "android-arm64",
        "xposed": {
            "packageName": "com.example.myxposedmodule",
            "name": "My Xposed Module"
        },
        "sign": {
            "keystore": "./.android/debug.keystore",
            "keystorePass": "android",
            "keystoreAlias": "androiddebugkey"
        }
    }
}
```

Each key in the configuration represents a build target. You can build all targets with:

```bash
fripack build
```

Or build a specific target (e.g., `xposed`) with:

```bash
fripack build xposed
```

Or watch a specific target for changes with:

```bash
fripack watch xposed
```

---

### Universal Configuration Options

The following options are available for all target types:

- `xz` (default: `false`): Compress the script using LZMA.
- `entry` (required): Entry point script to bundle.
- `fridaVersion` (required): Frida version to use (must be 17.5.1 or newer).
- `outputDir` (default: `./fripack`): Output directory for built artifacts.
- `platform`: Target platform (e.g., `android-arm64`, `windows-x86_64`).
  - Valid values: `android-arm32`, `android-arm64`, `android-x86`, `android-x64`, `windows-x64`, `linux-x64`
- `version`: Version of your plugin.
- `type`: Type of the target (defines the output format).
- `inherit`: Key of another target to inherit configuration from.
- `targetBaseName` (optional): Base name for output files (defaults to target key).
- `beforeBuild` (optional): Command to execute before building the target.
- `afterBuild` (optional): Command to execute after successfully building the target.
- `watchPath` (required for `watchpath` mode): Directory to watch for file changes.
- `pushPath` (required for `watchpath` mode): Destination path on device for pushing JavaScript files when in `watch` mode.

Example using inheritance to avoid repetition:

```json
{
    "base": {
        "version": "1.0.0",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "xz": true,
        "outputDir": "./fripack",
        "platform": "android-arm64"
    },
    "xposed": {
        "inherit": "base",
        "type": "xposed",
        "xposed": {
            "packageName": "com.example.myxposedmodule",
            "name": "My Xposed Module"
        },
        "sign": {
            "keystore": "./.android/debug.keystore",
            "keystorePass": "android",
            "keystoreAlias": "androiddebugkey"
        }
    },
    "raw-so": {
        "inherit": "base",
        "type": "shared"
    }
}
```

Only targets with a `type` field will be built.

---

### Supported Target Types

#### `xposed`

Builds your Frida script into an Xposed Module. Only supports `Android` platforms.

**Requires:** [`apktool`](https://apktool.org/) installed on your system.

**Additional options:**

- `xposed` (required): Xposed configuration object.
  - `packageName` (required): Package name for the Xposed module.
  - `name` (required): Display name of the module.
  - `icon` (optional): Path to the module icon (expects `ic_launcher.webp` and `ic_launcher_round.webp` in the same directory).
  - `scope` (optional): Suggested target scope for the module.
  - `description` (optional): Description of the module.
- `sign` (optional): Signing configuration. If provided as an object, the APK will be signed.
  - `keystore`: Path to the keystore.
  - `keystorePass`: Keystore passphrase.
  - `keystoreAlias`: Alias in the keystore.

#### `shared`

Builds your Frida script into a shared library (`.so` / `.dll`) that can be loaded via various methods (e.g., `LD_PRELOAD`).

#### `inject-apk`

Injects your Frida script into an existing APK by modifying one of its native libraries. Only supports `Android` platforms.

**Requires:** [`apktool`](https://apktool.org/) installed on your system.

It's also recommended to have [`zipalign`](https://developer.android.com/tools/zipalign) in your path.

**Additional options:**

- `injectApk` (required): Injection configuration object.
  - `sourceApkPath` (optional): Path to the source APK file to inject into.
  - `sourceApkPackageName` (optional): Package name of the APK to extract from a connected device.
    - Either `sourceApkPath` or `sourceApkPackageName` must be provided.
    - When using `sourceApkPackageName`, the APK will be extracted from the connected device and cached for future builds. This requires [`adb`](https://developer.android.com/studio/command-line/adb) to be installed on your system.
  - `injectMode` (optional): Injection mode. Currently only supports `"NativeAddNeeded"`.
  - `targetLib` (optional): Specific native library to target for injection (e.g., `"libnative-lib.so"`).
    - If not specified, will search for libraries in this priority order:
      1. `libCrashSight.so`, `libBugly.so`, `libmmkv.so` (whitelist)
      2. The smallest `.so` file in the lib directory (with warning)
- `sign` (optional): Signing configuration for the final APK (same format as Xposed).
  - `keystore`: Path to the keystore.
  - `keystorePass`: Keystore passphrase.
  - `keystoreAlias`: Alias in the keystore.
**Example configuration:**
```json
{
    "inject-apk": {
        "type": "inject-apk",
        "platform": "android-arm64",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "injectApk": {
            "sourceApkPackageName": "com.example.app",
            "injectMode": "NativeAddNeeded",
            "targetLib": "libnative-lib.so",
        },
        "sign": {
            "keystore": "./.android/debug.keystore",
            "keystorePass": "android",
            "keystoreAlias": "androiddebugkey"
        }
    }
}
```

#### `zygisk`

Builds your Frida script into a Zygisk module for Magisk. Only supports `Android` platforms.

**Additional options:**

- `zygisk` (required): Zygisk configuration object.
  - `id` (required): Module ID
  - `name` (required): Module display name
  - `version` (optional): Module version (defaults to "1.0").
  - `versionCode` (optional): Module version code (defaults to 1).
  - `author` (optional): Module author (defaults to "FriPack").
  - `description` (optional): Module description.
  - `scope` (required): Target applications for injection, separated by semicolons.

**Example configuration:**
```json
{
    "zygisk": {
        "type": "zygisk",
        "platform": "android-arm64",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "zygisk": {
            "id": "com.example.myzygiskmodule",
            "name": "My Zygisk Module",
            "version": "1.0.0",
            "versionCode": 1,
            "author": "Your Name",
            "description": "A Zygisk module that injects Frida scripts",
            "scope": "com.example.app1;com.example.app2"
        }
    }
}
```

### Developing Frida Script with Fripack

Fripack supports a watch mode for development that enables hot-reloading of JavaScript files without rebuilding the entire package.

#### Usage

Start watching a target:

```bash
fripack watch my-watch-target
```

The watch process will:
1. Build and install the target initially. Note that for targets with types other than `xposed`, you'll have to install the target manually.
2. Monitor for file changes
3. Automatically update when changes are detected
4. Continue running until you press Ctrl+C

**Note**: Watch mode requires `adb` to be installed and accessible in your PATH for pushing files and installing packages to Android devices.


---

## Notes
### How to check the logs?
On Android, logs are output through the Android logging system with the tag `FriPackInject`. You can view them using adb:
```bash
adb logcat FriPackInject:D *:S
```

On Windows, logs are written to both `stdout` and the Windows Debug Log. To view them, you can:
- Attach a debugger to the target application
- Use `AllocConsole` and `freopen` in your Frida script
- Start the target application in console
- Use [DebugView](https://learn.microsoft.com/en-us/sysinternals/downloads/debugview) to monitor the global system log

On other platforms, logs are directed to `stdout`.

### ReferenceError: 'Java' is not defined
Starting with Frida 17.0.0, bridges are no longer bundled with Frida’s GumJS runtime. This means that users now have to explicitly pull in the bridges they want to use.

You'll have to install the bridge and build your script through `frida-compile` before packaging. Check https://frida.re/docs/bridges/ for more details.

## Credits

- [Frida](https://github.com/frida/frida)
- [Florida](https://github.com/Ylarod/Florida)
- [xmake](https://xmake.io/)
