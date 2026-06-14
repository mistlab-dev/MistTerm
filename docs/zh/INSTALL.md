# 安装 Mist

Mist 是本仓库构建的 SSH 终端 GUI（`cargo` 二进制名：**Mist**）。

## 系统要求

| 平台 | 最低版本 | 说明 |
|------|----------|------|
| macOS | 10.15+ | Xcode 命令行工具、`libssh2`、`pkg-config` |
| Linux | glibc 2.17+ | `libssh2`、OpenSSL 开发包、`pkg-config` |
| Windows | 10+ | Rust + MSVC；`libssh2` 通过 vcpkg（见下文） |

界面默认 **英文**，可在 **偏好设置 → 语言** 切换为简体中文（Unix 下写入 `~/.config/mistterm/settings.json`）。

## 快速安装

### macOS / Linux

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
chmod +x scripts/install.sh
./scripts/install.sh
```

默认安装到 `~/.local/bin/Mist`。自定义路径：

```bash
INSTALL_DIR=/usr/local/bin ./scripts/install.sh
```

### Windows

从 [GitHub Releases](https://github.com/mistlab-dev/MistTerm/releases) 下载 **`MistTerm-*-windows-x86_64-setup.exe`**，双击安装即可（开始菜单快捷方式、可选桌面图标、自带卸载，无需配置 PATH）。

便携版（zip）仍在 Release 中提供，解压后需自行运行 `Mist.exe`。

## 不安装直接运行

```bash
cargo run --release --bin Mist
```

### macOS：直接启动 GUI（推荐）

避免在 Terminal.app 里 `cargo run` 占用终端窗口，请打包为 `.app` 并用 `open` 启动（**不要**在 Finder 里双击裸二进制 `target/release/Mist`，否则会先弹出终端窗口）：

```bash
chmod +x scripts/run-macos-gui.sh
./scripts/run-macos-gui.sh
```

等价于 `scripts/bundle-macos.sh` 后执行 `open target/release/Mist.app`。

安装脚本在 macOS 上也会把 `Mist.app` 复制到 `~/Applications`，可从 Spotlight 或 Dock 启动。

调试日志：在终端执行 `MIST_LOG=1 ~/.local/bin/Mist` 或 `RUST_LOG=debug cargo run --release --bin Mist`。

中文界面可选 CJK 字体（发布构建在字体存在时会嵌入）：

```bash
./scripts/fetch-cjk-font.sh
cargo build --release --bin Mist
```

## 依赖

### macOS

```bash
xcode-select --install
brew install libssh2 pkg-config
```

### Ubuntu / Debian

```bash
sudo apt update
sudo apt install -y build-essential libssh2-1-dev pkg-config libssl-dev
```

### Windows

安装 [Rust](https://rustup.rs)，通过 [vcpkg](https://vcpkg.io) 提供 libssh2。详见 [DEPLOYMENT.md](../tech/DEPLOYMENT.md)。

## 日志

应用日志（`log` / `tracing`）及 `~/.config/mistterm/logs/` 下的**会话日志**均为 **英文**。

## 更多

- 编译与打包：[DEPLOYMENT.md](../tech/DEPLOYMENT.md)
- 单元测试：`cargo test --lib -- --test-threads=1`
