# MistTerm 技术栈

> 版本与依赖以仓库根 `Cargo.toml` 为准；本文记录"用的是什么、为什么这么选"，不再复述通用科普。

## 1. 技术栈总览

```text
┌──────────────────────────────────────────────┐
│                   MistTerm                   │
├──────────────────────────────────────────────┤
│  UI      eframe / egui 0.37（即时模式）        │
│  终端    alacritty_terminal 0.26（VTE 网格）   │
│  SSH     ssh2 0.9（libssh2 绑定）+ keyring 2.3 │
│  并发    tokio 1.35 + parking_lot 0.12        │
│  数据    serde / serde_json                   │
│  日志    tracing 0.1（log 经 tracing-log 兼容） │
└──────────────────────────────────────────────┘
```

### 1.1 选型矩阵

| 层级 | 技术 | 说明 |
|------|------|------|
| 编程语言 | Rust 1.95+ | 编译期内存安全；eframe 0.37 的 MSRV 要求 1.95 |
| GUI 框架 | egui / eframe 0.37 | 即时模式，每帧重绘；运行时显式选 Glow 渲染器 |
| 终端内核 | alacritty_terminal 0.26 | 纯 Rust 的 VTE 网格与解析状态机 |
| SSH | ssh2 0.9 | libssh2 绑定；同步 API，由运行时调度 |
| 凭证存储 | keyring 2.3 | 跨平台系统钥匙串 |
| 异步运行时 | tokio 1.35 | `features = ["full"]` |
| 锁 | parking_lot 0.12 | 常规锁用 std::sync，热点路径用 parking_lot |
| 序列化 | serde 1.0 / serde_json 1.0 | 配置与团队接口 |
| 日志 | tracing 0.1 | `tracing-subscriber` + `env-filter`；`log` 宏经 `tracing-log` 转接 |
| 命令行 | clap 4 | `mist` 子命令 |
| HTTP | reqwest 0.11 | 团队同步与更新检查，`rustls-tls` |
| 剪贴板 | arboard 3.3 | 与 egui-winit 的剪贴板路径并存 |
| 文件对话框 | rfd 0.12 | 可选 feature `file-dialogs`；静态 CLI 构建时关闭 |

## 2. 核心框架

### 2.1 Rust

- MSRV 随 eframe 0.37 定在 **1.95**（`Cargo.toml` 里没有单独的 `rust-version` 字段，以注释与 CI 工具链为准）。
- 项目实际吃到的语言特性：`Arc`/`Mutex` 共享状态、`Result` 处理连接与 IO 错误、`serde` derive、`clap` derive。
- 不用宏做 UI 抽象；界面代码是普通函数 + 显式传入的 `&mut` 状态。

### 2.2 egui / eframe 0.37

- **即时模式**：界面每帧重绘，没有 widget 树要维护。终端画面本来就要按帧刷新，两者节奏一致。
- eframe 负责窗口、事件循环、渲染后端；Mist 在 `NativeOptions` 里显式指定 Glow。
- `default-features = false`，`clipboard` feature 必须保留（见 `Cargo.toml` 中 #367 的说明），否则 egui-winit 拿不到系统剪贴板。
- Linux 下 x11 与 wayland 自 0.30 起要显式开启。
- `winit` 用 `vendor/winit` 打过补丁的版本（0.30.13 基线），原因见 `vendor/winit/MISTTERM_PATCH.md`。

### 2.3 alacritty_terminal 0.26

终端不是自己写的：VT 解析、网格、光标、选区都交给 `alacritty_terminal`，Mist 负责把网格画到 egui 上并处理键盘、IME、滚动。这样 escape 序列的兼容性跟着上游走，不用逐条补。

## 3. 依赖说明

### 3.1 SSH 与凭证

**ssh2 0.9**（libssh2 绑定）
- 密码、密钥、SSH Agent、FIDO2 sk 密钥都走这条链路。
- 同步 API，调用放在专门的线程/任务里，不阻塞 UI 帧。
- Windows 需要 vcpkg 提供 libssh2；CI 可通过 `vendored-openssl` feature 自带 OpenSSL 源码编译。

**keyring 2.3**
- 密码落系统钥匙串（Windows Credential Manager / macOS Keychain / Linux Secret Service）。
- 配置文件里的会话元数据加密保存，不含明文密码。

### 3.2 并发

**tokio 1.35 + parking_lot 0.12**
- tokio 跑 IO 任务（连接、SFTP、会话保活）；ssh2 的同步调用不直接占用 tokio worker。
- 共享状态多为 `Arc<Mutex<_>>`：默认 std，锁竞争明显的热点用 parking_lot，理由是它没有中毒语义、开销更小。

### 3.3 序列化

**serde 1.0 / serde_json 1.0**
- 会话、片段、设置、团队接口都用 JSON。
- `#[derive(Serialize, Deserialize)]` 直接标注在结构体上；新增字段要给出默认值，保证旧配置能读。

### 3.4 日志

**tracing 0.1**
- 应用日志用 `tracing` 宏，`tracing-subscriber` + `env-filter` 按 `RUST_LOG` 过滤。
- `log` 0.4 仍留作兼容层（`tracing-log` 把 `log` 宏转进 tracing），不是在用 `env_logger`。
- 调试：`MIST_LOG=1` 或 `RUST_LOG=debug`。

## 4. 开发工具

### 4.1 Cargo

常用命令按本机与 CI 的实际用法：

```bash
cargo run --bin Mist            # 开发时直接起桌面版
cargo build --release --bin Mist
cargo test                      # 单元 + 集成测试
cargo test --test zmodem_integration_test
```

内存受限的机器不要跑全量 `cargo build` / `cargo test`，验证交给 CI；本地只做小范围 `cargo check`。原因见 `AGENTS.md`。

### 4.2 格式与 lint

仓库**没有**提交 `.rustfmt.toml`，格式按 rustfmt 默认（`edition = 2021` 取自 `Cargo.toml`）。

```bash
cargo fmt
cargo clippy -- -D warnings
```

### 4.3 调试

```bash
# macOS
lldb ./target/debug/Mist
# Linux
gdb ./target/debug/Mist
# 性能
cargo flamegraph --bin Mist
```

## 5. 为什么这么选

- **即时模式 GUI**：终端本身就是"每帧都要重画"的画面，egui 不用维护 widget 树，滚动和选区直接按帧算。
- **alacritty_terminal 而不是自研 VT 解析**：escape 序列的坑太多，跟着上游走比自己补快，也更容易对齐 vim / tmux / htop 的行为。
- **ssh2 而不是纯 Rust SSH**：libssh2 支持面完整（含 sk 密钥、agent），代价是要处理 C 依赖，构建脚本里已经解决。
- **tokio 只做调度**：ssh2 是同步库，套异步运行时容易出隐蔽阻塞，所以 IO 任务的边界划得很清楚。
- **JSON 而不是二进制格式**：用户要能看见、能手改、能备份，出问题也能自己查。

## 6. 版本与平台

### 6.1 关键版本

| 组件 | 当前 | 说明 |
|------|------|------|
| Rust | 1.95（MSRV） | 由 eframe 0.37 决定 |
| eframe / egui | 0.37 | `egui_extras` 同版本 |
| egui_plot | 0.38 | 监控图表 |
| alacritty_terminal | 0.26.0 | 终端内核 |
| ssh2 | 0.9 | libssh2 绑定 |
| tokio | 1.35 | `full` |
| winit | 0.30.13（vendored 补丁） | 见 `vendor/winit` |

### 6.2 平台

| 平台 | 状态 | 说明 |
|------|------|------|
| macOS | 支持 | Apple Silicon 为主，Intel 可选 |
| Linux | 支持 | 预编译包需要 glibc 2.39+ |
| Windows | 支持 | Windows 10+，libssh2 走 vcpkg |

### 6.3 升级依赖

```bash
cargo outdated
cargo update
cargo update -p egui
```

egui / eframe 升级要连带检查 `egui_extras`、`egui_plot`、`winit` 补丁和 IME 行为，别只看编译过没过。

## 相关文档

- [架构文档](./ARCHITECTURE.md)
- [模块设计](./MODULE-DESIGN.md)
- [API 文档](./API.md)
- [构建与发布](./DEPLOYMENT.md)
