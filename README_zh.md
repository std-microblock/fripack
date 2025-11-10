# Fripack  
### 将你的 Frida 脚本打包成可执行文件。

Frida 是一个强大的工具，但其体积较大且通常需要 root 权限，这使得将脚本分发给最终用户变得困难。这常常限制了 Frida 在开发面向更广泛用户的插件中的应用。

Fripack 通过将你的 Frida 脚本打包成各种可执行格式来解决这个问题——例如 Xposed 模块、Zygisk 模块、用于 `LD_PRELOAD` 的动态共享库，或可注入的 DLL——使得基于 Frida 的插件能够轻松分发和使用。

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
        "xz": true,
        "outputDir": "./fripack",
        "platform": "arm64-v8a",
        "packageName": "com.example.myxposedmodule",
        "keystore": "./.android/debug.keystore",
        "keystorePass": "android",
        "keystoreAlias": "androiddebugkey",
        "name": "我的 Xposed 模块"
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

---

### 通用配置选项

以下选项适用于所有目标类型：

- `xz` (默认: `false`): 使用 LZMA 压缩脚本。
- `entry` (必需): 要打包的入口脚本文件。
- `fridaVersion` (必需): 使用的 Frida 版本（必须为 17.5.1 或更新）。
- `outputDir` (默认: `./fripack`): 构建产物输出的目录。
- `platform`: 目标平台 (例如 `x86_64`, `arm64-v8a`)。
- `version`: 你的插件版本。
- `type`: 目标类型（定义了输出格式）。
- `inherit`: 要继承配置的另一个目标的键名。

使用继承来避免重复配置的示例：

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
        "name": "我的 Xposed 模块"
    },
    "raw-so": {
        "inherit": "base",
        "type": "android-so"
    }
}
```

只有包含 `type` 字段的目标才会被构建。

---

### 支持的目标类型

#### `xposed`

将你的 Frida 脚本构建成一个 Xposed 模块。  
**要求：** 系统中已安装 [`apktool`](https://apktool.org/)。

**额外选项：**

- `sign` (可选): 是否对生成的 APK 进行签名（需要 `apksigner`）。
  - `keystore` (签名时必需): 密钥库路径。
  - `keystorePass` (签名时必需): 密钥库密码。
  - `keystoreAlias` (签名时必需): 密钥库中的别名。
- `packageName` (必需): Xposed 模块的包名。
- `name` (必需): 模块的显示名称。
- `scope` (可选): 模块建议的作用范围。
- `description` (可选): 模块描述。

#### `android-so`

将你的 Frida 脚本构建成一个共享对象文件 (`.so`)，可以通过多种方式加载（例如 `LD_PRELOAD`）。

---

## 致谢

- [Frida](https://github.com/frida/frida)
- [Florida](https://github.com/Ylarod/Florida)
- [xmake](https://xmake.io/)