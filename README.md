# MistTerm

[English](#english) · [简体中文](#简体中文)

[![Release](https://img.shields.io/github/v/release/mistlab-dev/MistTerm)](https://github.com/mistlab-dev/MistTerm/releases/latest)
[![Website](https://img.shields.io/badge/website-mistlab.dev-blue)](https://mistlab.dev)
[![License](https://img.shields.io/badge/license-AGPL--3.0-lightgrey)](LICENSE)

Modern SSH terminal for DevOps and backend developers — Rust, GPU UI, multi-tab.

---

<a id="english"></a>

## English

### Install

**End users** — download from [GitHub Releases](https://github.com/mistlab-dev/MistTerm/releases/latest):

| Platform | Package |
|----------|---------|
| **Windows** | `MistTerm-*-windows-x86_64-setup.exe` (installer) or `.zip` |
| **macOS** | `Mist-macos-universal.tar.gz` / `.dmg` when published |
| **Linux** | `Mist-linux-x86_64.tar.gz` |

**From source** (developers):

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
./scripts/install.sh          # macOS / Linux → ~/.local/bin/Mist
# .\scripts\install.ps1       # Windows
cargo build --release --bin Mist
```

Details: [docs/en/INSTALL.md](docs/en/INSTALL.md).

### Features

| Area | What you get |
|------|----------------|
| **Terminal** | Async SSH (tokio + ssh2); egui + Alacritty grid; multi-tab / split panes; password, key, agent, Vault CA |
| **Files** | SFTP side panel; ZMODEM (`rz` / `sz`) with progress |
| **Snippets** | Personal library + variables; marketplace; usage analytics |
| **Ops** | Host monitor; port forward; batch exec; session logs |
| **Team** | [mistlab.dev](https://mistlab.dev) sync; Git cloud backup; HashiCorp Vault |
| **AI** | Built-in assistant panel (your API key, local config) |
| **UX** | English / 简体中文; themes; **Activity Rail** (hide with ⌘/Ctrl+B); Toast notifications (no bottom status bar) |

### Quick start

1. Launch **Mist**
2. **⌘N / Ctrl+N** — new session (or open the connection list from the left Activity Rail)
3. Connect — double-click a saved session, or **⌘T / Ctrl+T** for a new tab
4. **⌘K / Ctrl+K** — snippets; **View** menu or Activity Rail — SFTP / Monitor / AI / Forward
5. **⌘B / Ctrl+B** — show / hide Activity Rail (left-edge strip restores it when hidden)

### Documentation

| | |
|---|---|
| [Doc index (EN)](docs/en/README.md) | [Doc index (ZH)](docs/zh/README.md) |
| [Install](docs/en/INSTALL.md) | [Layout / chrome](docs/product/LAYOUT.md) |
| [Terminal behavior](docs/tech/TERMINAL-BEHAVIOR.md) | [User manual (ZH)](docs/manual/MistTerm_操作手册.html) |

### Testing

```bash
cargo test
cargo test --test zmodem_integration_test
```

### Contributing & license

Issues and PRs: [github.com/mistlab-dev/MistTerm](https://github.com/mistlab-dev/MistTerm). **AGPL-3.0** — see [LICENSE](LICENSE).

---

<a id="简体中文"></a>

## 简体中文

面向开发与运维的现代化 SSH 终端，Rust 构建。

### 安装

**普通用户**请从 [GitHub Releases](https://github.com/mistlab-dev/MistTerm/releases/latest) 下载：

| 平台 | 包名 |
|------|------|
| **Windows** | `MistTerm-*-windows-x86_64-setup.exe`（安装包）或 `.zip` 便携版 |
| **macOS** | `Mist-macos-universal.tar.gz` / 发布页中的 `.dmg` |
| **Linux** | `Mist-linux-x86_64.tar.gz` |

**从源码构建**（开发者）：

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
./scripts/install.sh          # macOS / Linux → ~/.local/bin/Mist
# .\scripts\install.ps1       # Windows
cargo build --release --bin Mist
```

详见 [docs/zh/INSTALL.md](docs/zh/INSTALL.md)。

### 功能

| 方向 | 说明 |
|------|------|
| **终端** | tokio + ssh2 异步 SSH；egui + Alacritty 网格；多标签 / 分屏；密码、密钥、Agent、Vault 证书 |
| **文件** | SFTP 侧栏；ZMODEM（`rz` / `sz`）与进度 |
| **片段** | 个人命令库与变量；市场模板；使用统计 |
| **运维** | 主机监控；端口转发；批量执行；会话日志 |
| **团队** | [mistlab.dev](https://mistlab.dev) 同步；Git 云备份；HashiCorp Vault |
| **AI** | 内置助手（自备 API Key，配置在本机） |
| **体验** | 中/英界面；主题；**活动栏**（⌘/Ctrl+B 可隐藏）；右下角 Toast（已无常驻底栏） |

### 快速上手

1. 启动 **Mist**
2. **⌘N / Ctrl+N** 新建会话（或从左侧**活动栏**打开连接列表）
3. 双击已保存连接，或 **⌘T / Ctrl+T** 开新标签
4. **⌘K / Ctrl+K** 片段；**视图**菜单或活动栏打开 SFTP / 监控 / AI / 转发
5. **⌘B / Ctrl+B** 显示 / 隐藏活动栏（隐藏后点左缘窄条可恢复）

### 文档

| | |
|---|---|
| [中文索引](docs/zh/README.md) | [英文索引](docs/en/README.md) |
| [安装说明](docs/zh/INSTALL.md) | [布局与 chrome](docs/product/LAYOUT.md) |
| [终端行为](docs/tech/TERMINAL-BEHAVIOR.md) | [操作手册](docs/manual/MistTerm_操作手册.html) |

### 测试

```bash
cargo test
cargo test --test zmodem_integration_test
```

### 贡献与许可

Issue / PR：[github.com/mistlab-dev/MistTerm](https://github.com/mistlab-dev/MistTerm)。**AGPL-3.0**，见 [LICENSE](LICENSE)。

---

Made with 🦀 — [mistlab.dev](https://mistlab.dev) · [Latest release](https://github.com/mistlab-dev/MistTerm/releases/latest)
