# MistTerm 技术架构文档

## 目录

1. [系统概述](#1-系统概述)
2. [架构设计](#2-架构设计)
3. [技术栈](#3-技术栈)
4. [模块设计](#4-模块设计)
5. [数据流设计](#5-数据流设计)
6. [性能优化](#6-性能优化)
7. [安全设计](#7-安全设计)
8. [部署指南](#8-部署指南)

---

## 1. 系统概述

### 1.1 产品定位

MistTerm 是一款基于 Rust 和 egui 构建的跨平台 SSH 终端工具，提供：
- 安全的 SSH 连接管理
- 实时终端渲染
- 会话配置持久化
- 多会话并发支持

### 1.2 核心特性

| 特性 | 描述 | 状态 |
|-----|------|-----|
| SSH 连接 | 支持密码认证、密钥认证 | ✅ 已实现 |
| 终端渲染 | ANSI 转义码解析、实时渲染 | ✅ 已实现 |
| 会话管理 | 保存/加载/删除会话配置 | ✅ 已实现 |
| 多会话 | 支持多个并发 SSH 连接 | ✅ 已实现 |
| 跨平台 | macOS、Linux、Windows | ✅ 已实现 |

### 1.3 系统边界

```
┌─────────────────────────────────────────────────────┐
│                   MistTerm                          │
│  ┌─────────────────────────────────────────────┐   │
│  │              应用边界                        │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │   │
│  │  │  UI 层   │──│ 核心层   │──│  SSH 层   │  │   │
│  │  │ (egui)   │  │(业务逻辑) │  │(libssh2) │  │   │
│  │  └──────────┘  └──────────┘  └──────────┘  │   │
│  └─────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
                        │
                        ▼
              ┌───────────────────┐
              │   远程 SSH 服务器   │
              │  (任意 Linux/Unix) │
              └───────────────────┘
```

---

## 2. 架构设计

### 2.1 整体架构

```
┌────────────────────────────────────────────────────────────────┐
│                        应用层 (Application)                     │
├────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                      UI 层 (ui/)                           │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │  app.rs     │  │  渲染引擎   │  │  事件处理   │       │  │
│  │  │  (主应用)   │  │  (egui)     │  │  (输入/焦点)│       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                     核心层 (core/)                         │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │ session.rs  │  │ team/ 等   │  │  状态管理   │       │  │
│  │  │ (会话管理)  │  │ (连接管理)  │  │ (状态/消息) │       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                      SSH 层 (ssh/)                         │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │ client.rs   │  │ manager.rs  │  │  通道管理   │       │  │
│  │  │ (SSH 客户端) │  │ (连接池)    │  │ (Shell/SFTP)│       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                  │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    终端层 (terminal/)                      │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │  │
│  │  │emulator.rs  │  │  renderer   │  │  输出缓冲   │       │  │
│  │  │(ANSI 解析)   │  │  (egui 渲染) │  │  (输出缓存) │       │  │
│  │  └─────────────┘  └─────────────┘  └─────────────┘       │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
└────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌───────────────────┐
                    │  libssh2 (C 库)   │
                    │  (SSH 协议实现)   │
                    └───────────────────┘
```

### 2.2 分层架构说明

#### 2.2.1 UI 层 (ui/)

**职责**: 用户界面渲染和交互处理

**核心模块**:
- `app.rs` - 主应用状态管理和生命周期
- 渲染逻辑 - egui 组件树构建
- 事件处理 - 键盘输入、鼠标事件、焦点管理

**依赖**: 核心层

#### 2.2.2 核心层 (core/)

**职责**: 业务逻辑和状态管理

**核心模块**:
- `session.rs` - 会话配置管理（保存/加载/删除）
- `team/`、`fragment_*` 等 - 团队片段、审计、分析
- 消息路由 - SSH 消息分发和处理

**依赖**: SSH 层、终端层

#### 2.2.3 SSH 层 (ssh/)

**职责**: SSH 协议实现和连接管理

**核心模块**:
- `client.rs` - SSH 客户端实现（连接/认证/通道）
- `manager.rs` - 连接池管理（多会话并发）
- 通道管理 - Shell 通道、SFTP 通道

**依赖**: libssh2 C 库

#### 2.2.4 终端层 (terminal/)

**职责**: 终端模拟和输出渲染

**核心模块**:
- `emulator.rs` - ANSI 转义码解析器
- 输出缓冲 - 终端输出缓存和管理
- 渲染适配 - egui 文本渲染

**依赖**: egui

### 2.3 目录结构

```
MistTerm/
├── src/
│   ├── main.rs              # 应用入口
│   ├── ui/                  # UI 层
│   │   ├── mod.rs
│   │   └── app.rs           # 主应用
│   ├── core/                # 核心层
│   │   ├── mod.rs
│   │   ├── session.rs       # 会话管理
│   │   └── team/            # 团队平台客户端
│   ├── ssh/                 # SSH 层
│   │   ├── mod.rs
│   │   ├── client.rs        # SSH 客户端
│   │   └── manager.rs       # 连接管理器
│   └── terminal/            # 终端层
│       ├── mod.rs
│       └── emulator.rs      # ANSI 解析器
├── Cargo.toml
├── sessions.json            # 会话配置持久化
└── docs/
    ├── tech/                # 技术文档
    └── product/             # 产品文档
```

---

## 3. 技术栈

具体版本与选型理由见 [TECH-STACK.md](./TECH-STACK.md)，这里只留架构视角需要的那几条：

| 组件 | 技术 | 在本架构里的位置 |
|-----|------|------|
| **编程语言** | Rust（MSRV 1.95） | 全工程 |
| **GUI 框架** | egui / eframe 0.37 | UI 层，即时模式每帧重绘 |
| **SSH 库** | ssh2 0.9（libssh2 绑定） | SSH 层，同步 API |
| **终端内核** | alacritty_terminal 0.26 | 核心层，VTE 网格与解析 |
| **异步运行时** | tokio 1.35 | 连接、文件传输、后台任务 |
| **锁** | std::sync + parking_lot 0.12 | 共享状态 |
| **序列化** | serde / serde_json | 配置文件与团队接口 |
| **日志** | tracing 0.1（`log` 经 tracing-log 兼容） | 全工程 |

### 3.1 依赖与分层的关系

- `ui/` 只依赖 `core/` 暴露的类型和事件，不直接碰 ssh2 会话。
- `core/` 持有会话状态与终端网格，SSH 调用在这里发起。
- 连接、SFTP、监控采集都跑在 tokio 任务里，UI 每帧只读快照，不在渲染路径上等 IO。

---

## 4. 模块设计

### 4.1 UI 层（ui/）

| 文件 | 职责 |
|------|------|
| [`app.rs`](../../src/ui/app.rs) | `MistTermApp`：顶层状态、活动栏、右抽屉互斥、Toast |
| [`workspace.rs`](../../src/ui/workspace.rs) | 面板注册顺序（top_chrome → activity_rail → 右抽屉 → CentralPanel → Foreground） |
| [`chrome.rs`](../../src/ui/chrome.rs) | 顶栏、Tab、Toast 绘制 |
| [`sidebar.rs`](../../src/ui/sidebar.rs) | 连接列表（默认收起，rail「连接」展开） |

#### 4.1.1 顶层状态分组

`MistTermApp` 字段多且随功能增长，按用途大致分这几组（具体以源文件为准）：

| 分组 | 代表字段 | 作用 |
|------|----------|------|
| 动作与扩展 | `action_bus`、`module_registry` | 全局动作总线；扩展模块注册表 |
| 会话与标签 | `session_manager`、`selected_session_id`、`tabs`、`active_tab` | 已保存会话、当前选中、终端标签 |
| 布局 | `sidebar_collapsed`、`activity_rail_collapsed`、`sidebar_width` | 连接栏、活动栏、响应式档位 |
| 域状态 | `fragment_state`、`send_replay` | 片段 / 命令面板 / 市场 / 发送序列 / 粘贴历史 |
| 规则 | `output_trigger_rules`、`display_link_rules`、`cmd_audit_personal_rules` | 输出触发、链接识别、个人审计 |
| 提示与更新 | `status_message`、`update_ui`、`crash_notice` | Toast、自动更新、崩溃提示 |

#### 4.1.2 一帧的流程

1. `update()` 每帧从 egui 取输入事件（键盘、IME、滚动、拖放）。
2. 键盘事件先判焦点：终端有焦点才进 PTY；无焦点时先给快捷键和命令面板。
3. 面板按固定顺序注册（见 [LAYOUT.md](../product/LAYOUT.md)），右侧抽屉同一时刻只开一个。
4. 需要告知用户的结果统一走 Toast：`status_message` → `tick_status_toast` → `paint_status_toast`。
5. 远程数据（会话输出、监控、SFTP 进度）由 tokio 任务产生，UI 每帧只取快照，不在渲染里等 IO。

### 4.2 核心层 (core/)

#### 4.2.1 会话管理 (session.rs)

`SessionConfig` 是保存下来的会话：`id`、`name`、`group`、`host`、`port`、`username`、`password`、`private_key_path`、`use_ssh_agent`，之后还有认证模式、keepalive、跳板与转发等字段。`SessionManager` 负责加载、增删、保存这些配置，落盘为加密的 JSON。

字段会随功能增加，以 [`src/core/session.rs`](../../src/core/session.rs) 为准。

#### 4.2.2 SSH 连接与会话 (ssh/)

连接状态与多会话并发由 **SSH 层**（`ssh/client.rs`、`ssh/manager.rs`）与 UI 中的 `SshSessionHandle` 管理，不再使用已移除的 `core/connection.rs`。

### 4.3 SSH 层 (ssh/)

#### 4.3.1 SSH 客户端 (client.rs)

`SshConfig` 是一次连接需要的全部参数：账号密码、私钥路径、ssh-agent / Pageant、FIDO2 sk 认证模式、keepalive、ProxyJump / ProxyCommand，以及本地 / 远程 / SOCKS 三类端口转发。

`SshClient` 持有 `session: Option<Session>`、`config: SshConfig` 和本会话登记的 `profile_forwards`，方法围绕"建连 → 认证 → 开通道 → 读写"展开。字段以 [`src/ssh/client.rs`](../../src/ssh/client.rs) 为准，这里不复制。

#### 4.3.2 连接管理器 (manager.rs)

`SshManager` 是全局会话表：

- `sessions: Arc<Mutex<HashMap<SshSessionId, SshClient>>>`
- `message_tx: Sender<SshMessage>`
- `next_session_id: Arc<AtomicUsize>`

UI 通过 `SshSessionHandle` 操作单个会话；后台线程把输出与事件以 `SshMessage` 送回主线程。
- 消息通道管理

### 4.4 终端层 (terminal/)

| 文件 | 职责 |
|------|------|
| [`alacritty.rs`](../../src/terminal/alacritty.rs) | alacritty_terminal 适配层：VTE 网格 → egui 渲染、选区、滚动、OSC 分块 |
| [`emulator.rs`](../../src/terminal/emulator.rs) | 自带的终端模拟器：网格、滚屏历史、备用屏幕、光标与样式 |
| [`command_block.rs`](../../src/terminal/command_block.rs) | 命令块索引、块导航与块复制 |
| [`sem_osc.rs`](../../src/terminal/sem_osc.rs) | 语义 OSC：目录（9;9）、徽标（7501）、命令块（133）等 |
| [`program_status.rs`](../../src/terminal/program_status.rs) | 程序状态：等待 / 完成，供侧栏与标签状态灯 |

#### 4.4.1 终端模拟器 (emulator.rs)

`Terminal` 维护 `height × width` 的字符网格、滚出屏幕的 `lines` 历史、光标位置和当前样式；`AnsiState` 是解析状态机（Normal / Escape / Csi / CsiParam / Osc / CharsetDesignate）。

职责：
- 解析 ANSI 转义码与 OSC 序列，维护网格和滚屏
- 处理 DEC 备用屏幕（vim / less 等全屏程序）与光标保存 / 恢复
- 样式落在 `CharStyle` / `Cell` 上

---

## 5. 数据流设计

### 5.1 连接流程

```text
新建/选中会话（Ctrl+N 或侧栏双击）
        │
        ▼
SessionConfig ──► 保存到 SessionManager（加密 JSON）
        │
        ▼
SshManager 分配 session id，SshSessionHandle 交给 UI
        │
        ▼
后台线程：TCP 连接（可经 ProxyJump / ProxyCommand / SOCKS）
        │
        ▼
SSH 握手 ──► 主机密钥校验（known_hosts）──► 认证
                                        （密码 / 密钥 / agent / FIDO2 sk）
        │
        ▼
开 PTY + Shell 通道 ──► 启动读泵线程
        │
        ▼
SshMessage 回主线程 ──► 标签状态转 online ──► Toast 提示
```

### 5.2 键盘输入

终端是**模拟器**，不是"输入框 + 发送按钮"：

1. 终端有焦点时，按键直接写进 PTY（`\r` 由 Enter 产生，不是由程序拼 `\n`）。
2. 本地命令策略开启时，Enter 前先检查当前行；命中拦截的行不发出去，留在提示符上。
3. 全屏程序（vim / less / top）、密码提示、rz/sz 传输期间不做本地检查，按键直接放行。
4. 终端没有焦点时，按键先给快捷键和命令面板（例如 Ctrl+K 打开面板）。

### 5.3 输出接收

```text
PTY 通道收到字节
        │
        ▼
读泵线程按批读出 ──► SshMessage::Output
        │
        ▼
主线程把字节喂给 VTE 解析（alacritty_terminal / Processor）
        │
        ▼
网格与命令块索引更新 ──► 语义 OSC 解析（目录 / 徽标 / 程序状态）
        │
        ▼
egui 下一帧重绘可见网格（只取快照，不等 IO）
```

---

## 6. 并发与内存

### 6.1 线程模型

- **UI 主线程**：egui 每帧渲染，只读写快照，不做阻塞 IO。
- **每个会话一个读泵线程**：ssh2 是同步库，通道读取放在这条线程里，读到的字节打包成 `SshMessage` 送回主线程。
- **tokio**：承担连接建立、SFTP、监控采集、更新检查等异步任务。
- 会话表是 `Arc<Mutex<HashMap<SshSessionId, SshClient>>>`，一个会话失败不影响其它会话。

### 6.2 消息传递

```text
读泵线程 / tokio 任务 ──► Sender<SshMessage> ──► UI 线程（每帧 drain）
```

单向数据流：后台只发消息，UI 只收消息，共享可变状态尽量少。

### 6.3 内存

- 每个会话的网格由 VTE 持有；滚屏历史上限由 `max_lines` 控制，不会无限增长。
- 共享会话对象用 `Arc`，锁按会话粒度分开，避免一把全局大锁拖住所有会话。
- 输出过长的场景（批量执行、命令块复制）走截断，避免一次性拷贝整段历史。

---

## 7. 安全设计

### 7.1 认证安全

| 认证方式 | 状态 | 说明 |
|---------|-----|------|
| 密码认证 | ✅ 已实现 | 依赖 SSH 传输层加密 |
| 密钥认证 | ✅ 已实现 | 指定私钥路径 + `~/.ssh` 默认密钥 |
| SSH Agent | ✅ 已实现 | ssh-agent / Pageant（会话可开关） |
| FIDO2 / 安全密钥 | ✅ 已实现 | 会话认证类型。只经 OpenSSH agent 使用 `ecdsa-sk` / `ed25519-sk`，不自研 FIDO 栈；触摸 / PIN 为系统提示 |
| 证书认证 | — | **Vault SSH CA 由服务端签发**；桌面端从 Vault KV 读凭证，不实现 CA 签名 |

### 7.2 数据存储

本地敏感 JSON（`sessions.json`、`settings.json`、`fragments.json`、`credentials.json` 等）使用 **`device_key` + AES-256-GCM** 信封加密（格式 `mistterm-aes-v1`）。详见 [SECURITY.md](./SECURITY.md)。

首次启动或升级时，仍为明文 JSON 的旧文件会在加载时自动迁移为加密信封。

### 7.3 连接安全

- ✅ SSH-2 协议（加密传输）
- ✅ 服务器主机密钥验证（`known_hosts` 首次信任）
- ✅ Vault KV 凭证（团队 `vault_credential_path` → 连接前读取密码/私钥）
- 短期 SSH 证书：由 **Vault SSH CA / 运维** 在服务端签发，非桌面客户端职责

---

## 8. 部署指南

构建、打包与 CI 的完整流程见 [DEPLOYMENT.md](./DEPLOYMENT.md)。这里只列环境要求：

| 要求 | 版本 | 说明 |
|-----|------|------|
| Rust | 1.95+ | eframe 0.37 的 MSRV |
| libssh2 | 1.11+ | SSH 库 |
| pkg-config | - | 依赖查找 |

各平台的依赖安装、交叉编译与打包步骤写在这些文档里，不在这里重复：

- [DEPLOYMENT.md](./DEPLOYMENT.md) — 构建、打包、CI
- [中文安装说明](../zh/INSTALL.md) / [English install guide](../en/INSTALL.md)

---

## 相关文档

- [产品文档](../product/FUNCTIONAL_SPEC.md)
- [模块设计](./MODULE-DESIGN.md)
- [API 文档](./API.md)
- [安全与审计](./SECURITY.md)
