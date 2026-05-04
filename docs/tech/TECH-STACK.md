# MistTerm 技术选型说明

> **文档版本**: 1.0  
> **最后更新**: 2026-04-24  
> **状态**: 已确定

---

## 📋 目录

1. [技术栈总览](#1-技术栈总览)
2. [核心框架](#2-核心框架)
3. [依赖库详解](#3-依赖库详解)
4. [开发工具](#4-开发工具)
5. [技术选型理由](#5-技术选型理由)
6. [版本管理](#6-版本管理)

---

## 1. 技术栈总览

```
┌────────────────────────────────────────────────────────────┐
│                      MistTerm 技术栈                        │
├────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                   应用层                              │  │
│  │   Rust + eframe + egui (即时模式 GUI 框架)              │  │
│  └──────────────────────────────────────────────────────┘  │
│                              │                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                   业务层                              │  │
│  │   标准库 (线程、通道、锁) + parking_lot               │  │
│  └──────────────────────────────────────────────────────┘  │
│                              │                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                   协议层                              │  │
│  │   ssh2 (libssh2 Rust 绑定) + SSH-2 协议              │  │
│  └──────────────────────────────────────────────────────┘  │
│                              │                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                   系统层                              │  │
│  │   macOS / Linux / Windows (跨平台)                   │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
└────────────────────────────────────────────────────────────┘
```

### 1.1 技术选型矩阵

| 层级 | 技术选型 | 类型 | 成熟度 |
|-----|---------|-----|--------|
| 编程语言 | Rust | 系统级语言 | 🟢 成熟 |
| GUI 框架 | egui | 即时模式 GUI | 🟢 成熟 |
| 应用框架 | eframe | 应用框架 | 🟢 成熟 |
| SSH 库 | ssh2/libssh2 | 协议库 | 🟢 成熟 |
| 并发 | std::sync + parking_lot | 并发原语 | 🟢 成熟 |
| 序列化 | serde/serde_json | 序列化框架 | 🟢 成熟 |
| 日志 | log/env_logger | 日志框架 | 🟢 成熟 |

---

## 2. 核心框架

### 2.1 Rust 编程语言

**版本**: 1.70+

**核心特性**:
- **所有权系统** - 编译时内存安全保证
- **零成本抽象** - 无运行时开销的高级抽象
- **模式匹配** - 强大的类型匹配能力
- **错误处理** - Result/Option 类型安全处理
- **并发安全** - 数据竞争在编译时阻止

**为什么选择 Rust**:

| 对比维度 | Rust | C/C++ | Go |
|---------|------|-------|-----|
| 内存安全 | ✅ 编译时保证 | ❌ 运行时 | ✅ GC |
| 性能 | ⚡ 接近 C | ⚡ 最快 | 🐢 GC 开销 |
| 并发安全 | ✅ 编译时保证 | ❌ 手动 | ✅ Goroutine |
| 学习曲线 | 📈 陡峭 | 📈 陡峭 | 📉 平缓 |
| 生态系统 | 🟢 增长中 | 🟢 丰富 | 🟢 丰富 |

### 2.2 egui 即时模式 GUI

**版本**: 0.24+

**核心特性**:
- **即时模式** - 每帧重绘，简单高效
- **跨平台** - 支持所有主流平台
- **无状态** - 组件无状态，易于测试
- **自定义** - 高度可定制的样式
- **轻量级** - 最小外部依赖

**架构**:

```
┌─────────────────────────────────────┐
│            egui 架构                 │
├─────────────────────────────────────┤
│                                     │
│  ┌──────────┐  ┌──────────┐        │
│  │  布局    │  │  响应    │        │
│  │  系统    │  │  系统    │        │
│  └──────────┘  └──────────┘        │
│         │            │              │
│         ▼            ▼              │
│  ┌──────────────────────────┐      │
│  │      渲染后端            │      │
│  │   (egui_glow/epaint)     │      │
│  └──────────────────────────┘      │
│                                     │
└─────────────────────────────────────┘
```

**为什么选择 egui**:

| 优势 | 说明 |
|-----|------|
| 简单 | API 直观，学习成本低 |
| 快速 | 即时模式，无状态管理 |
| 跨平台 | 一套代码，多平台运行 |
| 终端友好 | 适合终端类应用 |
| 可定制 | 完全可自定义样式 |

**GUI 框架选择：**

| 框架 | 模式 | 学习曲线 | 性能 | 适用场景 |
|-----|------|---------|------|---------|
| egui | 即时模式 | 📉 低 | ⚡ 快 | 工具类应用 |

### 2.3 eframe 应用框架

**版本**: 0.24+

**职责**:
- 提供跨平台窗口管理
- 集成 egui 渲染后端
- 处理应用生命周期
- 支持原生和 Web 构建

**核心 API**:

```rust
// 应用入口
eframe::run_native(
    "MistTerm",
    NativeOptions::default(),
    Box::new(|_cc| Box::new(MistTermApp::default()))
)

// 应用实现
impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 每帧调用
    }
}
```

---

## 3. 依赖库详解

### 3.1 SSH 相关

#### ssh2 (Rust 绑定)

**版本**: 0.9+

**功能**:
- SSH-2 协议完整实现
- 密码认证
- 密钥认证（待实现）
- Shell 通道
- SFTP 通道（待实现）
- 端口转发（待实现）

**核心 API**:

```rust
use ssh2::Session;

// 创建会话
let mut session = Session::new()?;

// 连接服务器
session.connect("server.com", 22)?;

// 认证
session.userauth_password("username", "password")?;

// 打开 Shell
let mut channel = session.channel_session()?;
channel.exec(true)?;
```

#### libssh2 (底层 C 库)

**版本**: 1.9+

**说明**: ssh2 crate 的底层实现，通过 FFI 调用

**依赖管理**:
```toml
[dependencies]
ssh2 = { version = "0.9", features = ["libssh2"] }

[build-dependencies]
pkg-config = "0.3"
```

### 3.2 并发相关

#### parking_lot

**版本**: 0.12+

**功能**:
- 更快的互斥锁
- 条件变量
- 读写锁
- 线程公园

**为什么选择 parking_lot**:

| 对比 | std::sync::Mutex | parking_lot::Mutex |
|-----|-----------------|-------------------|
| 性能 | 🐢 标准 | ⚡ 更快 |
| 功能 | 基础 | 丰富 |
| 死锁检测 | ❌ 无 | ✅ 支持 |
| 内存占用 | 📦 较大 | 📦 较小 |

**使用示例**:

```rust
use parking_lot::Mutex;
use std::sync::Arc;

// 线程安全共享
let data = Arc::new(Mutex::new(Vec::new()));

// 锁定访问
{
    let mut guard = data.lock();
    guard.push(42);
}
```

### 3.3 序列化相关

#### serde

**版本**: 1.0+

**功能**:
- 通用序列化框架
- 编译时代码生成
- 支持多种格式

**为什么选择 serde**:

| 优势 | 说明 |
|-----|------|
| 通用 | 支持多种格式 |
| 类型安全 | 编译时检查 |
| 零开销 | 代码生成，无运行时 |
| 生态 | 广泛支持 |

**使用示例**:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}
```

#### serde_json

**版本**: 1.0+

**功能**: JSON 格式序列化

**用途**: 会话配置持久化

### 3.4 日志相关

#### log

**版本**: 0.4+

**功能**:
- 日志记录框架
- 多级日志（Error/Warn/Info/Debug/Trace）
- 与多个后端集成

#### env_logger

**版本**: 0.10+

**功能**:
- 基于环境变量的日志配置
- 控制台输出
- 格式自定义

**使用示例**:

```rust
use log::{info, warn, error};

// 初始化
env_logger::Builder::from_env(
    env_logger::Env::default().default_filter_or("info")
).init();

// 记录日志
info!("Connected to server");
warn!("Connection unstable");
error!("Authentication failed");
```

---

## 4. 开发工具

### 4.1 构建工具

#### Cargo

**功能**:
- 包管理
- 依赖解析
- 构建编译
- 测试运行
- 文档生成

**常用命令**:

```bash
# 编译
cargo build

# 编译优化版本
cargo build --release

# 运行
cargo run

# 运行测试
cargo test

# 格式化代码
cargo fmt

# 检查代码
cargo clippy

# 生成文档
cargo doc --open
```

### 4.2 代码质量工具

#### rustfmt

**功能**: 自动格式化代码

**配置**: `.rustfmt.toml`

```toml
max_width = 100
tab_spaces = 4
edition = "2021"
```

#### clippy

**功能**: 代码 lint 检查

**运行**:
```bash
cargo clippy -- -D warnings
```

### 4.3 调试工具

#### gdb/lldb

**功能**: 调试器

**使用**:
```bash
# macOS
lldb ./target/debug/mistterm

# Linux
gdb ./target/debug/mistterm
```

#### cargo-flamegraph

**功能**: 性能分析

**使用**:
```bash
cargo flamegraph --bin mistterm
```

---

## 5. 选型理由

### 5.1 为什么选择即时模式 (egui)?

| 优势 | 说明 |
|-----|------|
| 无状态管理 | 即时模式每帧重绘，无需维护 widget 树状态 |
| 响应快 | egui 原生性能，适合高频刷新的终端场景 |
| 易测试 | 渲染逻辑可纯函数测试 |
| 易调试 | 问题容易定位，无异步状态干扰 |
| 终端友好 | 天然适合嵌入终端渲染器 |

### 5.2 技术选型总结

- **语言**: Rust（内存安全 + 高性能 + 跨平台）
- **GUI**: egui（即时模式，天然匹配终端刷新场景）
- **终端**: alacritty_terminal（成熟的纯 Rust VTE 实现）
- **SSH**: ssh2 crate（libssh2 的 Rust 绑定） + keyring（密钥链）
- **序列化**: serde_json（配置持久化）
- **并发**: parking_lot + std::sync（轻量并发）

---

## 6. 版本管理

### 6.1 版本要求

| 组件 | 最低版本 | 推荐版本 |
|-----|---------|---------|
| Rust | 1.70 | 1.75+ |
| egui | 0.24 | 0.25+ |
| eframe | 0.24 | 0.25+ |
| ssh2 | 0.9 | 0.9.4+ |
| serde | 1.0 | 1.0.190+ |

### 6.2 兼容性

| 平台 | 支持状态 | 说明 |
|-----|---------|------|
| macOS | ✅ 完全支持 | Apple Silicon + Intel |
| Linux | ✅ 完全支持 | glibc 2.17+ |
| Windows | ✅ 完全支持 | Windows 10+ |

### 6.3 依赖更新策略

```bash
# 查看可更新依赖
cargo outdated

# 更新依赖
cargo update

# 更新特定依赖
cargo update -p egui
```

---

## 📚 相关文档

- [架构文档](./ARCHITECTURE.md)
- [模块设计](./MODULE-DESIGN.md) (待创建)
- [API 文档](./api.md) (待创建)

---

**文档维护**: 技术团队  
**最后更新**: 2026-04-24  
**状态**: 已确定
