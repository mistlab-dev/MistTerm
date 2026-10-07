<p align="center">
  <img src="assets/app-icon-preview.png" width="128" height="128" alt="MistTerm">
</p>

<h1 align="center">MistTerm</h1>

<p align="center">
  Modern SSH terminal for DevOps and backend developers — Rust, GPU UI, multi-tab.
</p>

<p align="center">
  <a href="https://github.com/mistlab-dev/MistTerm/releases/latest"><img src="https://img.shields.io/github/v/release/mistlab-dev/MistTerm" alt="Release"></a>
  <a href="https://mistlab.dev"><img src="https://img.shields.io/badge/website-mistlab.dev-blue" alt="Website"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-lightgrey" alt="License"></a>
</p>

<p align="center">
  <a href="#english">English</a> · <a href="#简体中文">简体中文</a>
</p>

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
| **CLI** | `mist` headless CLI sharing the same session store — `ls` / `exec` (single + `--group` / `--all` batch) / `ssh` / `rls` / `get` / `put` / `fwd` / `frag` / `import-ssh-config` / `sop extract`. See [docs/tech/CLI-DESIGN.md](docs/tech/CLI-DESIGN.md) |
| **Team** | [mistlab.dev](https://mistlab.dev) sync; Git cloud backup; HashiCorp Vault |
| **AI** | Built-in assistant panel (your API key, local config) |
| **UX** | English / 简体中文; themes; **Activity Rail** (hide with ⌘/Ctrl+B); Toast notifications (no bottom status bar) |

### Quick start

Running the GUI binary `Mist` with **no arguments** launches the desktop UI directly (builds since v1.1.17).

1. Launch **Mist**
2. **⌘N / Ctrl+N** — new session (or open the connection list from the left Activity Rail)
3. Connect — double-click a saved session, or **⌘T / Ctrl+T** for a new tab
4. **⌘K / Ctrl+K** — snippets; **View** menu or Activity Rail — SFTP / Monitor / AI / Forward
5. **⌘B / Ctrl+B** — show / hide Activity Rail (left-edge strip restores it when hidden)

### Updates & privacy

Mist checks for a new version shortly after it starts and then once a day. It only tells you about it: installing waits for your click, and Mist never restarts on its own. On Linux and Windows you can update with one click; on macOS Mist shows you how to update by hand. From the terminal: `mist update --check`, `mist update` (same as `mist self-update`), and `mist update --rollback`.

The check is a single request to mistlab.dev and GitHub that carries only the Mist version and system type, with no account or device information. As with any website, those servers can see your IP address. Updates are signed and checked before anything is installed. To turn checks off, clear **Preferences → General → Check for updates automatically**, or set `MIST_DISABLE_UPDATE_CHECK=1`, which also blocks manual checks. Details: [docs/release/AUTO_UPDATE.md](docs/release/AUTO_UPDATE.md).

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

Issues and PRs: [github.com/mistlab-dev/MistTerm](https://github.com/mistlab-dev/MistTerm). **AGPL-3.0** — see [LICENSE](LICENSE). Contributions are accepted under the [contributor license terms](CONTRIBUTING.md#contributor-license-terms). Third-party notices: [resources/THIRD_PARTY_LICENSES.txt](resources/THIRD_PARTY_LICENSES.txt) (also in the app under **About → Open-source licenses**).

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
| **命令行** | `mist` 无头 CLI，与 GUI 共享同一份会话库——`ls` / `exec`（单机 + `--group` / `--all` 批量）/ `ssh` / `rls` / `get` / `put` / `fwd` / `frag` / `import-ssh-config` / `sop extract`。详见 [docs/tech/CLI-DESIGN.md](docs/tech/CLI-DESIGN.md) |
| **团队** | [mistlab.dev](https://mistlab.dev) 同步；Git 云备份；HashiCorp Vault |
| **AI** | 内置助手（自备 API Key，配置在本机） |
| **体验** | 中/英界面；主题；**活动栏**（⌘/Ctrl+B 可隐藏）；右下角 Toast（已无常驻底栏） |

### 快速上手

GUI 二进制 `Mist` **不带任何参数**运行即直接拉起桌面界面（v1.1.17 起）。

1. 启动 **Mist**
2. **⌘N / Ctrl+N** 新建会话（或从左侧**活动栏**打开连接列表）
3. 双击已保存连接，或 **⌘T / Ctrl+T** 开新标签
4. **⌘K / Ctrl+K** 片段；**视图**菜单或活动栏打开 SFTP / 监控 / AI / 转发
5. **⌘B / Ctrl+B** 显示 / 隐藏活动栏（隐藏后点左缘窄条可恢复）

### 更新与隐私

Mist 启动后过一会儿检查一次有没有新版本，之后每天检查一次。有新版本只会提醒你，装不装由你点按钮决定，Mist 也不会自己重启。Linux 和 Windows 可以一键更新；macOS 上会告诉你怎么手动更新。命令行里可以用 `mist update --check`、`mist update`（也可以写 `mist self-update`）和 `mist update --rollback`（退回上一个版本）。

检查更新只会向 mistlab.dev 和 GitHub 发一次请求，只带 Mist 版本号和系统类型，不带账号或设备信息。和访问任何网站一样，对方能看到你的 IP 地址。新版本都有签名，安装前会先核对。不想检查的话，可以在 **偏好设置 → 常规** 里取消「自动检查更新」；或者设置环境变量 `MIST_DISABLE_UPDATE_CHECK=1`，这样连手动检查也会关掉。详见 [docs/release/AUTO_UPDATE.md](docs/release/AUTO_UPDATE.md)。

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

Issue / PR：[github.com/mistlab-dev/MistTerm](https://github.com/mistlab-dev/MistTerm)。**AGPL-3.0**，见 [LICENSE](LICENSE)。提交贡献即表示同意[贡献者授权条款](CONTRIBUTING.md#贡献者授权条款中文摘要)。第三方许可声明见 [resources/THIRD_PARTY_LICENSES.txt](resources/THIRD_PARTY_LICENSES.txt)(App 内：**关于 → 开源许可**)。

---

Made with 🦀 — [mistlab.dev](https://mistlab.dev) · [Latest release](https://github.com/mistlab-dev/MistTerm/releases/latest)
