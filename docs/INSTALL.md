# Installing Mist

Mist is the SSH terminal GUI built from this repository (`cargo` binary name: **Mist**).

## System requirements

| Platform | Minimum | Notes |
|----------|---------|--------|
| macOS | 10.15+ | Xcode CLI tools, `libssh2`, `pkg-config` |
| Linux | glibc 2.17+ | `libssh2`, OpenSSL dev headers, `pkg-config` |
| Windows | 10+ | Rust + MSVC; `libssh2` via vcpkg (see below) |

UI defaults to **English**. Switch to Simplified Chinese in **Preferences → Language** (saved in `~/.config/mistterm/settings.json` on Unix).

## Quick install

### macOS / Linux

```bash
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm
chmod +x scripts/install.sh
./scripts/install.sh
```

Binary is copied to `~/.local/bin/Mist` by default. Override install location:

```bash
INSTALL_DIR=/usr/local/bin ./scripts/install.sh
```

### Windows (PowerShell)

```powershell
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm
.\scripts\install.ps1
```

Default install path: `%LOCALAPPDATA%\Programs\Mist\Mist.exe`.

## Run without installing

```bash
cargo run --release --bin Mist
```

Optional CJK font for Chinese UI (release builds embed the font when present):

```bash
./scripts/fetch-cjk-font.sh
cargo build --release --bin Mist
```

## Dependencies

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

Install [Rust](https://rustup.rs), then use [vcpkg](https://vcpkg.io) for libssh2. See [docs/tech/DEPLOYMENT.md](tech/DEPLOYMENT.md) for detailed steps.

## Logs

Application logs (`log` / `tracing`) and on-disk **session logs** under `~/.config/mistterm/logs/` are written in **English**.

## More

- Build and packaging details: [DEPLOYMENT.md](tech/DEPLOYMENT.md)
- Smoke checklist: [SMOKE.md](SMOKE.md)
