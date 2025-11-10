# Fripack  
### Package your Frida script into an executable.

[中文](./README_zh.md)

<img width="400" alt="image" src="https://github.com/user-attachments/assets/5a00307c-fd30-4991-a82e-2b23f3d115b7" />

Frida is a powerful tool, but its size and the need for root access make it challenging to distribute scripts to end-users. This often limits Frida’s use in developing plugins for wider audiences.

Fripack solves this by packaging your Frida scripts into various executable formats—such as Xposed Modules, Zygisk Modules, shared objects for `LD_PRELOAD`, or injectable DLLs—enabling easy distribution and use of Frida-based plugins.


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
        "xz": true,
        "outputDir": "./fripack",
        "platform": "arm64-v8a",
        "packageName": "com.example.myxposedmodule",
        "keystore": "./.android/debug.keystore",
        "keystorePass": "android",
        "keystoreAlias": "androiddebugkey",
        "name": "My Xposed Module"
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

---

### Universal Configuration Options

The following options are available for all target types:

- `xz` (default: `false`): Compress the script using LZMA.
- `entry` (required): Entry point script to bundle.
- `fridaVersion` (required): Frida version to use (must be 17.5.1 or newer).
- `outputDir` (default: `./`): Output directory for built artifacts.
- `platform`: Target platform (e.g., `x86_64`, `arm64-v8a`).
- `version`: Version of your plugin.
- `type`: Type of the target (defines the output format).
- `inherit`: Key of another target to inherit configuration from.

Example using inheritance to avoid repetition:

```json
{
    "base": {
        "version": "1.0.0",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "xz": true,
        "outputDir": "./fripack",
        "platform": "arm64-v8a"
    },
    "xposed": {
        "inherit": "base",
        "type": "xposed",
        "packageName": "com.example.myxposedmodule",
        "keystore": "./.android/debug.keystore",
        "keystorePass": "android",
        "keystoreAlias": "androiddebugkey",
        "name": "My Xposed Module"
    },
    "raw-so": {
        "inherit": "base",
        "type": "android-so"
    }
}
```

Only targets with a `type` field will be built.

---

### Supported Target Types

#### `xposed`

Builds your Frida script into an Xposed Module.  
**Requires:** [`apktool`](https://apktool.org/) installed on your system.

**Additional options:**

- `sign` (optional): Whether to sign the generated APK (requires `apksigner`).
  - `keystore` (required if signing): Path to the keystore.
  - `keystorePass` (required if signing): Keystore passphrase.
  - `keystoreAlias` (required if signing): Alias in the keystore.
- `packageName` (required): Package name for the Xposed module.
- `name` (required): Display name of the module.
- `scope` (optional): Suggested target scope for the module.
- `description` (optional): Description of the module.

#### `android-so`

Builds your Frida script into a shared object (`.so`) that can be loaded via various methods (e.g., `LD_PRELOAD`).

---

## Credits

- [Frida](https://github.com/frida/frida)
- [Florida](https://github.com/Ylarod/Florida)
- [xmake](https://xmake.io/)
