# Fripack
### 将你的 Frida 脚本打包成可执行文件。

[English](./README.md)

<img width="400" alt="image" src="https://github.com/user-attachments/assets/5a00307c-fd30-4991-a82e-2b23f3d115b7" />

Frida 是一个强大的工具，但其体积较大且通常需要 root 权限，这使得将脚本分发给最终用户变得困难。这常常限制了 Frida 在开发面向更广泛用户的插件中的应用。

Fripack 通过将你的 Frida 脚本打包成各种可执行格式来解决这个问题——例如 Xposed 模块、修补的 APK、用于 `LD_PRELOAD` 的共享对象，或可注入的 DLL——使得基于 Frida 的插件能够轻松分发和使用。

### 二进制大小很重要

原始的 Frida 项目体积庞大。Fripack 通过精简和压缩 Frida，使得在所有平台（Linux 除外）上的二进制输出都小于 10 MB。

<img width="345" height="168" alt="image" src="https://github.com/user-attachments/assets/bf6f1134-f9a0-4d31-b15a-e49ae5c545d8" />

### 一键构建

跨平台的 Frida 脚本通常使得为不同目标构建交付物变得繁琐——即使使用 Frida Gadget 也是如此。Fripack 通过支持一次命令构建多个平台来简化开发。

## 安装

从 [发布页面](https://github.com/std-microblock/fripack/releases/latest) 下载最新的二进制文件，并根据需要进行安装。


## 快速开始

### 基础配置

Fripack 使用一个名为 `fripack.json` 的配置文件，该文件支持 JSON5 语法。以下是一个基础示例：

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
            "name": "我的 Xposed 模块"
        },
        "sign": {
            "keystore": "./.android/debug.keystore",
            "keystorePass": "android",
            "keystoreAlias": "androiddebugkey"
        }
    }
}
```

配置中的每个键代表一个构建目标。你可以使用以下命令构建所有目标：

```bash
fripack build
```

或者构建特定的目标（例如 `xposed`）：

```bash
fripack build xposed
```

或者监听特定目标的文件变化：

```bash
fripack watch xposed
```

---

### 通用配置选项

以下选项适用于所有目标类型：

- `xz` (默认: `false`): 使用 LZMA 压缩脚本。
- `entry` (必需): 要打包的入口脚本文件。
- `fridaVersion` (必需): 使用的 Frida 版本（必须为 17.5.1 或更新）。
- `outputDir` (默认: `./fripack`): 构建产物输出的目录。
- `platform`: 目标平台 (例如 `android-arm64`, `windows-x86_64`)。
  - 有效值: `android-arm32`, `android-arm64`, `android-x86`, `android-x64`, `windows-x64`, `linux-x64`
- `version`: 你的插件版本。
- `type`: 目标类型（定义了输出格式）。
- `inherit`: 要继承配置的另一个目标的键名。
- `targetBaseName` (可选): 输出文件的基础名称（默认为目标键名）。
- `beforeBuild` (可选): 在构建目标之前执行的命令。
- `afterBuild` (可选): 在成功构建目标之后执行的命令。
- `watchPath` (watchpath 模式必需): 监听文件变化的目录。
- `pushPath` (watch 模式必需): 在 watch 模式下推送 JavaScript 文件到设备的目标路径。

使用继承来避免重复配置的示例：

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
            "name": "我的 Xposed 模块"
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

只有包含 `type` 字段的目标才会被构建。

---

### 支持的目标类型

#### `xposed`

将你的 Frida 脚本构建成一个 Xposed 模块。

**要求：** 需安装 [`apktool`](https://apktool.org/)。

**额外选项：**

- `xposed` (必需): Xposed 配置对象。
  - `packageName` (必需): Xposed 模块的包名。
  - `name` (必需): 模块的显示名称。
  - `icon` (可选): 模块图标路径（期望同一目录下有 `ic_launcher.webp` 和 `ic_launcher_round.webp`）。
  - `scope` (可选): 模块建议的作用范围。
  - `description` (可选): 模块描述。
- `sign` (可选): 签名配置。如果提供对象，则对 APK 进行签名。
  - `keystore`: 密钥库路径。
  - `keystorePass`: 密钥库密码。
  - `keystoreAlias`: 密钥库中的别名。

#### `shared`

将你的 Frida 脚本构建成一个共享库 (`.so` / `.dll`)，可以通过多种方式加载（例如 `LD_PRELOAD`）。

#### `inject-apk`

通过修改现有 APK 的原生库来将你的 Frida 脚本注入其中。仅支持 `Android` 平台。

**要求：** 需安装 [`apktool`](https://apktool.org/)。

还建议安装 [`zipalign`](https://developer.android.com/tools/zipalign)。

**额外选项：**

- `injectApk` (必需): 注入配置对象。
  - `sourceApkPath` (可选): 要注入的源 APK 文件路径。
  - `sourceApkPackageName` (可选): 要从连接设备提取的 APK 包名。
    - 必须提供 `sourceApkPath` 或 `sourceApkPackageName` 中的一个。
    - 使用 `sourceApkPackageName` 时，APK 将从连接的设备提取并缓存以供后续构建使用。这要求系统中已安装 [`adb`](https://developer.android.com/studio/command-line/adb)。
  - `injectMode` (可选): 注入模式。目前仅支持 `"NativeAddNeeded"`。
  - `targetLib` (可选): 要注入的特定原生库（例如 `"libnative-lib.so"`）。
    - 如果未指定，将按以下优先级顺序搜索库：
      1. `libCrashSight.so`、`libBugly.so`、`libmmkv.so`（白名单）
      2. lib 目录中最小的 `.so` 文件（会显示警告）
- `sign` (可选): 最终 APK 的签名配置（格式与 Xposed 相同）。
  - `keystore`: 密钥库路径。
  - `keystorePass`: 密钥库密码。
  - `keystoreAlias`: 密钥库中的别名。

**配置示例：**
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

将你的 Frida 脚本构建成一个用于 Magisk 的 Zygisk 模块。仅支持 `Android` 平台。

**额外选项：**

- `zygisk` (必需): Zygisk 配置对象。
  - `id` (必须): 模块 ID
  - `name` (必须): 模块显示名称
  - `version` (可选): 模块版本（默认为 "1.0"）。
  - `versionCode` (可选): 模块版本代码（默认为 1）。
  - `author` (可选): 模块作者（默认为 "FriPack"）。
  - `description` (可选): 模块描述。
  - `scope` (必须): 注入的目标应用程序，用分号分隔。

**配置示例：**
```json
{
    "zygisk": {
        "type": "zygisk",
        "platform": "android-arm64",
        "fridaVersion": "17.5.1",
        "entry": "main.js",
        "zygisk": {
            "id": "com.example.myzygiskmodule",
            "name": "我的 Zygisk 模块",
            "version": "1.0.0",
            "versionCode": 1,
            "author": "你的名字",
            "description": "一个注入 Frida 脚本的 Zygisk 模块",
            "scope": "com.example.app1;com.example.app2"
        }
    }
}
```

### 使用 Fripack 开发 Frida 脚本

Fripack 支持用于开发的监听模式，可以实现 JavaScript 文件的热重载而无需重新构建整个包。

#### 使用方法

开始监听一个目标：

```bash
fripack watch my-watch-target
```

监听进程将会：
1. 初始构建并安装目标。注意，对于类型不是 `xposed` 的目标，你需要手动安装目标。
2. 监听文件变化
3. 检测到变化时自动更新
4. 持续运行直到你按下 Ctrl+C

**注意**：监听模式需要安装 `adb` 并在 PATH 中可访问，以便推送文件和安装包到 Android 设备。

---

## Notes
### 如何查看日志？
在 Android 上，日志通过 Android 日志系统输出，标签为 `FriPackInject`。你可以使用 adb 查看它们：
```bash
adb logcat FriPackInject:D *:S
```

在 Windows 上，日志会同时写入 `stdout` 和 Windows 调试日志。要查看它们，你可以：
- 将调试器附加到目标应用程序
- 在 Frida 脚本中使用 `AllocConsole` 和 `freopen`
- 在控制台中启动目标应用程序
- 使用 [DebugView](https://learn.microsoft.com/en-us/sysinternals/downloads/debugview) 监控全局系统日志

在其他平台上，日志会定向到 `stdout`。

### ReferenceError: 'Java' is not defined
从 Frida 17.0.0 开始，桥接器不再与 Frida 的 GumJS 运行时捆绑在一起。这意味着用户现在必须明确引入他们想要使用的桥接器。

你必须安装桥接器并通过 `frida-compile` 构建脚本，然后再进行打包。查看 https://frida.re/docs/bridges/ 了解更多详情。

## 致谢

- [Frida](https://github.com/frida/frida)
- [Florida](https://github.com/Ylarod/Florida)
- [xmake](https://xmake.io/)