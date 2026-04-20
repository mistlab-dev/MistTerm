# 🌫️ MistTerm

> A sleek Rust terminal emulator with seamless rzsz file transfer integration

Inspired by iTerm2, built with Rust. MistTerm brings modern terminal experience with integrated file transfer capabilities.

## ✨ Features

- 🚀 **Pure Rust** implementation for performance and safety
- 📁 **Integrated rzsz** (ZMODEM) file transfer
  - `rz` - Receive files from remote
  - `sz <file>` - Send files to remote
- 🎨 **Modern TUI** with customizable themes
- ⌨️ **Full keyboard support** with intuitive shortcuts
- 🔌 **Multi-connection support** (SSH, Serial)
- 🛠️ **Command palette** for quick actions
- 📱 **Tab-based interface** for session management

## 🚀 Quick Start

```bash
# Install from source
cargo install --path .

# Or install from crates.io (when published)
cargo install mistterm

# Run MistTerm
mistterm

# Connect via SSH
mistterm --host example.com --user admin

# Connect to serial device
mistterm --device /dev/ttyUSB0 --baud 115200
```

## 📖 Usage

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Ctrl+Q` | Quit MistTerm |
| `Ctrl+T` | Open command palette |
| `Tab` | Switch between views |
| `Enter` | Execute command |

### Commands

Type these in the input bar:

```bash
rz           # Receive file (ZMODEM)
sz <file>    # Send file (ZMODEM)
clear        # Clear screen
help         # Show help
quit         # Exit
```

### File Transfer Example

1. Connect to remote server: `mistterm --host server.com`
2. Type `rz` to receive files from remote
3. On remote server, type: `sz filename.txt`
4. File automatically saves to current directory

## 🛠️ Building from Source

```bash
# Clone repository
git clone https://github.com/c-wind/MistTerm.git
cd MistTerm

# Build release binary
cargo build --release

# Run
./target/release/mistterm
```

## 📝 Dependencies

- [tui-rs](https://github.com/fdehau/tui-rs) - Terminal UI
- [crossterm](https://github.com/crossterm-rs/crossterm) - Cross-platform terminal
- [clap](https://github.com/clap-rs/clap) - CLI argument parsing
- [tokio](https://github.com/tokio-rs/tokio) - Async runtime

## 🐛 Development Status

🚧 **Early Development** - Core terminal framework is working. rzsz integration in progress.

### Roadmap

- [x] Basic TUI framework
- [x] Command palette
- [ ] Full rzsz integration
- [ ] SSH connection support
- [ ] Serial port support
- [ ] Configuration file
- [ ] Plugin system
- [ ] Windows support

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

MIT License - see [LICENSE](LICENSE) for details

---

Made with ☕ and 🦀
