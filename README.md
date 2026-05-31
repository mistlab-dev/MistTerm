# 🌫️ MistTerm

[English](#english) · [简体中文](#简体中文)

[![Website](https://img.shields.io/badge/website-mistlab.dev-blue)](https://mistlab.dev)
[![Docs](https://img.shields.io/badge/docs-GitHub-green)](https://github.com/mistlab-dev/MistTerm/tree/main/docs)
[![GitHub](https://img.shields.io/badge/github-mistlab--dev/MistTerm-black)](https://github.com/mistlab-dev/MistTerm)

---

<a id="english"></a>

## English

> Modern SSH terminal — built with Rust for DevOps and backend developers.

### Features

**Terminal** — Rust + tokio + ssh2; GPU rendering (egui/alacritty); multi-tab; password, key, and Vault CA auth.

**Files** — SFTP panel; ZMODEM (`rz`/`sz`) with progress.

**Snippets** — Personal library with variables; marketplace templates; usage analytics.

**Ops** — Host monitor; batch exec; session logs.

**Team** — [mistlab.dev](https://mistlab.dev) team sync; Git cloud sync; HashiCorp Vault.

**AI** — Built-in assistant panel.

**UX** — English / 简体中文; themes; keyboard shortcuts.

### Install

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
./scripts/install.sh          # macOS / Linux → ~/.local/bin/Mist
# .\scripts\install.ps1       # Windows
cargo build --release --bin Mist
```

See [docs/en/INSTALL.md](docs/en/INSTALL.md). Repository docs: [docs/](docs/).

### Quick start

1. Launch `Mist`
2. New session — sidebar (⌘N / Ctrl+N)
3. Snippets — bottom bar or ⌘K / Ctrl+K
4. SFTP — View menu
5. Monitor — View menu

### Documentation

| | |
|---|---|
| [Doc index (EN)](docs/en/README.md) | [Doc index (ZH)](docs/zh/README.md) |
| [Install (EN)](docs/en/INSTALL.md) | [Install (ZH)](docs/zh/INSTALL.md) |

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

> 面向开发与运维的现代化 SSH 终端，Rust 构建。

### 功能

**终端** — tokio + ssh2 异步 SSH；egui/alacritty 渲染；多标签；密码、密钥、Vault 证书登录。

**文件** — SFTP 面板；ZMODEM（`rz`/`sz`）与进度显示。

**片段** — 个人命令库与变量；市场模板；使用统计。

**运维** — 主机监控；批量执行；会话日志。

**团队** — [mistlab.dev](https://mistlab.dev) 团队同步；Git 云同步；HashiCorp Vault。

**AI** — 内置助手面板。

**体验** — 中/英界面；主题；快捷键。

### 安装

```bash
git clone https://github.com/mistlab-dev/MistTerm.git
cd MistTerm
./scripts/install.sh          # macOS / Linux → ~/.local/bin/Mist
# .\scripts\install.ps1       # Windows
cargo build --release --bin Mist
```

详见 [docs/zh/INSTALL.md](docs/zh/INSTALL.md)。项目文档见 [docs/](docs/)。

### 快速上手

1. 运行 `Mist`
2. 侧栏新建连接（⌘N / Ctrl+N）
3. 底栏片段或 ⌘K / Ctrl+K
4. 菜单「视图」→ SFTP
5. 菜单「视图」→ 监控

### 文档

| | |
|---|---|
| [英文索引](docs/en/README.md) | [中文索引](docs/zh/README.md) |
| [Install (EN)](docs/en/INSTALL.md) | [安装 (ZH)](docs/zh/INSTALL.md) |

### 测试

```bash
cargo test
cargo test --test zmodem_integration_test
```

### 贡献与许可

Issue / PR：[github.com/mistlab-dev/MistTerm](https://github.com/mistlab-dev/MistTerm)。**AGPL-3.0**，见 [LICENSE](LICENSE)。

---

Made with 🦀 and ☕ — [mistlab.dev](https://mistlab.dev)
