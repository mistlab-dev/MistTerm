# 🌫️ MistTerm

> Modern SSH terminal — built with Rust for DevOps and backend developers.

[![Website](https://img.shields.io/badge/website-mistlab.dev-blue)](https://mistlab.dev)
[![Docs](https://img.shields.io/badge/docs-docs.mistlab.dev-green)](https://docs.mistlab.dev)
[![GitHub](https://img.shields.io/badge/github-c--wind/MistTerm-black)](https://github.com/c-wind/MistTerm)

## ✨ Features

### Core Terminal
- 🚀 **Rust + async** — tokio + ssh2, non-blocking SSH sessions
- 🖥️ **GPU-accelerated rendering** — egui/eframe with alacritty_terminal backend
- 📑 **Multi-tab** — manage multiple sessions in one window
- 🔐 **Multi-auth** — password, SSH key, SSH certificate (Vault CA)

### File Transfer
- 📁 **SFTP browser** — drag & drop upload, right-click download, side-by-side with terminal
- 📤 **ZMODEM** — native `rz`/`sz` support with progress indication

### Command Snippets
- 🔧 **Fragment library** — personal command snippets with variables
- 🛒 **Marketplace** — 60+ pre-built templates (Linux, Docker, K8s, networking, databases…)
- 📊 **Analytics** — usage tracking, success rates, execution time aggregation

### Monitoring & Operations
- 📊 **System monitor** — real-time CPU, memory, disk, network for connected hosts
- 📝 **Batch execution** — run commands across multiple servers in parallel
- 📋 **Session logs** — automatic recording with searchable history

### Team & Sync
- 👥 **Team platform** — shared sessions, credentials, and fragment analytics via [mistlab.dev](https://mistlab.dev)
- ☁️ **Cloud sync** — Git-based sync across devices
- 🔐 **Vault SSH CA** — HashiCorp Vault integration for certificate-based auth

### AI
- 🤖 **AI assistant** — built-in panel for command suggestions and error analysis

### UX
- 🌐 **Bilingual** — English (default) / 简体中文, switch in Preferences
- 🎨 **Theming** — light/dark mode with consistent design language
- ⌨️ **Keyboard-first** — full shortcut support, quick snippet picker

## 🚀 Install

### macOS / Linux

```bash
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm
./scripts/install.sh
```

Binary → `~/.local/bin/Mist`. Override with `INSTALL_DIR=/usr/local/bin ./scripts/install.sh`.

### Windows (PowerShell)

```powershell
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm
.\scripts\install.ps1
```

### Build from source

```bash
cargo build --release --bin Mist
./target/release/Mist
```

**Dependencies:**

| Platform | Requirements |
|----------|-------------|
| macOS | Xcode CLI tools, `libssh2`, `pkg-config` |
| Linux | `libssh2-dev`, `libssl-dev`, `pkg-config` |
| Windows | Rust + MSVC, libssh2 via vcpkg |

See [docs/INSTALL.md](docs/INSTALL.md) for detailed instructions.

## 📖 Quick start

1. **Launch** → `Mist`
2. **Connect** — left sidebar → New Session (⌘N / Ctrl+N)
3. **Snippets** — bottom bar → Fragment icon, or ⌘K / Ctrl+K
4. **Files** — View → SFTP panel
5. **Monitor** — View → Monitor panel

## 🗺️ Architecture

```
src/
├── core/            # Business logic (sessions, fragments, market, team, sync)
├── ssh/             # SSH transport, ZMODEM, SCP
├── terminal/        # alacritty_terminal VT adaptation
├── ui/              # egui UI (app, terminal, sidebar, panels, dialogs)
├── platform/        # OS abstractions (shell, docs, shortcuts)
├── sync/            # Git-based cloud sync
├── security/        # Credential storage, keyring
└── i18n/            # English / Chinese localization
```

**Key dependencies:** [eframe/egui](https://github.com/emilk/egui) · [tokio](https://github.com/tokio-rs/tokio) · [ssh2](https://github.com/alexcrichton/ssh2-rs) · [alacritty_terminal](https://github.com/alacritty/alacritty)

## 📚 Documentation

| Document | Description |
|----------|-------------|
| [Product spec](docs/product/FUNCTIONAL_SPEC.md) | Full feature specification |
| [Architecture](docs/tech/ARCHITECTURE.md) | System design and data flow |
| [Module design](docs/tech/MODULE-DESIGN.md) | Module interfaces and contracts |
| [API reference](docs/tech/API.md) | Internal API documentation |
| [Deployment](docs/tech/DEPLOYMENT.md) | Build, package, and release |
| [Security](docs/tech/SECURITY.md) | Encryption, audit, credential management |
| [Terminal behavior](docs/tech/TERMINAL-BEHAVIOR.md) | VT/ANSI handling details |
| [ZMODEM](docs/tech/ZMODEM.md) | ZMODEM implementation and troubleshooting |
| [Smoke tests](docs/tech/SMOKE.md) | Multi-platform manual QA checklist |
| [Installation](docs/INSTALL.md) | Detailed install guide |

Full doc index: [docs/README.md](docs/README.md)

## 🧪 Testing

```bash
# Unit tests
cargo test

# ZMODEM integration
cargo test --test zmodem_integration_test
```

## 🤝 Contributing

Issues and pull requests welcome at [GitHub](https://github.com/c-wind/MistTerm).

## 📄 License

MIT License — see [LICENSE](LICENSE).

---

Made with 🦀 and ☕ — [mistlab.dev](https://mistlab.dev)
