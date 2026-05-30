# MistTerm API 文档

## 📋 目录

1. [概述](#1-概述)
2. [SSH 层 API](#2-ssh-层-api)
3. [核心层 API](#3-核心层-api)
4. [UI 层 API](#4-ui-层-api)
5. [终端层 API](#5-终端层-api)
6. [数据格式](#6-数据格式)
7. [错误码](#7-错误码)

---

## 1. 概述

本文档描述 MistTerm 内部模块的 API 接口。

### 1.1 API 分层

```
┌─────────────────────────────────────┐
│          UI Layer API               │
│         (egui 交互接口)              │
├─────────────────────────────────────┤
│        Core Layer API               │
│        (业务逻辑接口)                │
├─────────────────────────────────────┤
│         SSH Layer API               │
│        (SSH 连接接口)                │
├─────────────────────────────────────┤
│       Terminal Layer API            │
│       (终端模拟接口)                 │
└─────────────────────────────────────┘
```

---

## 2. SSH 层 API

### 2.1 SshConfig

SSH 连接配置结构。

```rust
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}
```

| 字段 | 类型 | 必填 | 说明 |
|-----|------|-----|------|
| host | String | ✅ | 服务器地址（IP 或域名） |
| port | u16 | ✅ | 端口号（1-65535） |
| username | String | ✅ | 登录用户名 |
| password | String | ✅ | 登录密码 |

**示例**:
```rust
let config = SshConfig {
    host: "192.0.2.10".to_string(),
    port: 22,
    username: "ubuntu".to_string(),
    password: "your_password".to_string(),
};
```

### 2.2 SshClient

SSH 客户端类。

```rust
pub struct SshClient {
    config: SshConfig,
    session: Option<ssh2::Session>,
}
```

#### 2.2.1 构造方法

```rust
impl SshClient {
    /// 创建新的 SSH 客户端
    pub fn new(config: SshConfig) -> Self
}
```

**参数**:
- `config` - SSH 连接配置

**返回**: `SshClient` 实例

#### 2.2.2 连接方法

```rust
pub fn connect(&mut self) -> Result<(), String>
```

**职责**: 建立 TCP 连接并完成 SSH 握手和认证

**返回**: 
- `Ok(())` - 连接成功
- `Err(String)` - 错误信息

**可能错误**:
- `TCP connect failed: ...` - TCP 连接失败
- `SSH handshake failed: ...` - SSH 握手失败
- `Authentication failed: ...` - 认证失败

**示例**:
```rust
let mut client = SshClient::new(config);
match client.connect() {
    Ok(_) => println!("Connected!"),
    Err(e) => eprintln!("Error: {}", e),
}
```

#### 2.2.3 认证方法

```rust
pub fn authenticate(&mut self) -> Result<(), String>
```

**职责**: 使用密码进行认证

**返回**: 
- `Ok(())` - 认证成功
- `Err(String)` - 认证失败

#### 2.2.4 通道方法

```rust
pub fn open_shell(&mut self) -> Result<Channel, String>
```

**职责**: 打开一个 Shell 通道

**返回**: 
- `Ok(Channel)` - Shell 通道
- `Err(String)` - 打开失败

**前置条件**: 必须先调用 `connect()` 成功

#### 2.2.5 数据发送

```rust
pub fn send(&mut self, data: &[u8]) -> Result<usize, String>
```

**参数**:
- `data` - 要发送的字节数据

**返回**: 
- `Ok(usize)` - 实际发送的字节数
- `Err(String)` - 发送失败

**示例**:
```rust
let cmd = b"ls -la\n";
let sent = client.send(cmd)?;
println!("Sent {} bytes", sent);
```

#### 2.2.6 状态检查

```rust
pub fn is_connected(&self) -> bool
pub fn disconnect(&mut self)
```

### 2.3 SshManager

SSH 连接管理器。

```rust
pub struct SshManager {
    sessions: Arc<Mutex<Vec<SshClient>>>,
    message_tx: Sender<SshMessage>,
    next_session_id: usize,
}
```

#### 2.3.1 构造方法

```rust
impl SshManager {
    /// 创建新的管理器
    pub fn new() -> (Self, Receiver<SshMessage>)
}
```

**返回**: `(SshManager, Receiver<SshMessage>)`

#### 2.3.2 异步连接

```rust
pub fn create_session_async(
    &mut self,
    config: SshConfig
) -> Result<SshSessionId, String>
```

**参数**:
- `config` - SSH 连接配置

**返回**: 
- `Ok(SshSessionId)` - 会话 ID
- `Err(String)` - 创建失败

**说明**: 在后台线程执行连接，通过消息通道通知结果

#### 2.3.3 Shell 启动

```rust
pub fn start_interactive_shell(
    &mut self,
    session_id: SshSessionId
) -> Result<SshSessionHandle, String>
```

**参数**:
- `session_id` - 会话 ID

**返回**: 
- `Ok(SshSessionHandle)` - 会话句柄
- `Err(String)` - 启动失败

#### 2.3.4 消息处理

```rust
pub fn handle_ssh_message(
    &self,
    msg: SshMessage,
    selected_session: Option<usize>
)
```

**参数**:
- `msg` - SSH 消息
- `selected_session` - 当前选中的会话索引

#### 2.3.5 会话管理

```rust
pub fn get_sessions(&self) -> &[SshClient]
pub fn session_count(&self) -> usize
```

### 2.4 SshMessage

SSH 消息枚举。

```rust
pub enum SshMessage {
    Connected,
    Disconnected,
    Output(Vec<u8>),
    Error(String),
}
```

| 变体 | 说明 | 数据 |
|-----|------|-----|
| Connected | 连接成功 | - |
| Disconnected | 断开连接 | - |
| Output | 输出数据 | `Vec<u8>` |
| Error | 错误信息 | `String` |

---

## 3. 核心层 API

### 3.1 SessionConfig

会话配置结构。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}
```

| 字段 | 类型 | 说明 |
|-----|------|-----|
| name | String | 会话名称（用户自定义） |
| host | String | 服务器地址 |
| port | u16 | 端口号 |
| username | String | 用户名 |
| password | String | 密码 |

### 3.2 SessionManager

会话管理器。

```rust
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    sessions_file: PathBuf,
}
```

#### 3.2.1 构造方法

```rust
impl SessionManager {
    /// 创建新的管理器（自动加载已保存会话）
    pub fn new() -> Self
}
```

#### 3.2.2 会话操作

```rust
/// 添加新会话
pub fn add_session(&mut self, config: SessionConfig) -> usize

/// 删除会话
pub fn remove_session(&mut self, idx: usize)

/// 获取所有会话
pub fn get_sessions(&self) -> &[SessionConfig]

/// 获取会话数量
pub fn count(&self) -> usize
```

#### 3.2.3 持久化

```rust
/// 保存到文件
pub fn save(&self) -> Result<(), std::io::Error>

/// 从文件加载
pub fn load(&mut self) -> Result<(), std::io::Error>
```

### 3.3 ConnectionState

连接状态枚举。

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

| 状态 | 说明 | UI 显示 |
|-----|------|-------|
| Disconnected | 未连接 | - |
| Connecting | 连接中 | "Connecting..." |
| Connected | 已连接 | "✓ Connected" |
| Error | 错误 | "Error: xxx" |

### 3.4 SshSessionState

SSH 会话状态。

```rust
pub struct SshSessionState {
    pub config: SessionConfig,
    pub state: ConnectionState,
    pub terminal: Terminal,
    pub handle: Option<SshSessionHandle>,
}
```

#### 3.4.1 构造方法

```rust
impl SshSessionState {
    pub fn new(config: SessionConfig) -> Self
}
```

#### 3.4.2 状态方法

```rust
/// 获取状态文本（用于 UI 显示）
pub fn status_text(&self) -> String
```

**返回**:
- "Connecting..." - 连接中
- "Connected" - 已连接
- "Error: xxx" - 错误
- "Disconnected" - 未连接

### 3.5 ConnectionManager

连接管理器。

```rust
pub struct ConnectionManager {
    sessions: Vec<Arc<Mutex<SshSessionState>>>,
    ssh_manager: SshManager,
    message_rx: Option<Receiver<SshMessage>>,
}
```

#### 3.5.1 构造方法

```rust
impl ConnectionManager {
    /// 创建新的管理器
    pub fn new() -> (Self, Receiver<SshMessage>)
}
```

#### 3.5.2 会话管理

```rust
/// 添加新会话
pub fn add_session(&mut self, config: SessionConfig) -> usize

/// 获取会话
pub fn get_session(&self, idx: usize) -> Option<Arc<Mutex<SshSessionState>>>

/// 获取所有会话
pub fn get_sessions(&self) -> &[Arc<Mutex<SshSessionState>>]

/// 删除会话
pub fn remove_session(&mut self, idx: usize)
```

#### 3.5.3 SSH 管理器访问

```rust
/// 获取 SSH 管理器引用
pub fn get_ssh_manager(&self) -> &SshManager
```

#### 3.5.4 消息处理

```rust
/// 处理 SSH 消息
pub fn handle_ssh_message(
    &mut self,
    msg: SshMessage,
    selected_session: Option<usize>
)
```

---

## 4. UI 层 API

### 4.1 MistTermApp

主应用类。

```rust
pub struct MistTermApp {
    session_manager: SessionManager,
    connection_manager: Option<ConnectionManager>,
    message_rx: Option<Receiver<SshMessage>>,
    selected_session: Option<usize>,
    showing_connect_dialog: bool,
    new_config: SessionConfig,
    input_buffer: HashMap<String, String>,
}
```

#### 4.1.1 构造方法

```rust
impl Default for MistTermApp {
    fn default() -> Self
}
```

#### 4.1.2 eframe 实现

```rust
impl eframe::App for MistTermApp {
    /// 主更新循环（每帧调用）
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)
}
```

#### 4.1.3 私有方法

| 方法 | 职责 |
|-----|------|
| `render_header()` | 渲染顶部栏 |
| `render_session_list()` | 渲染会话列表 |
| `render_terminal()` | 渲染终端窗口 |
| `render_input()` | 渲染命令输入框 |
| `render_connect_dialog()` | 渲染连接对话框 |
| `connect_session(idx)` | 连接指定会话 |

---

## 5. 终端层 API

### 5.1 Terminal

终端模拟器。

```rust
pub struct Terminal {
    output: VecDeque<String>,
    style: CharStyle,
    ansi_state: AnsiState,
}
```

#### 5.1.1 构造方法

```rust
impl Terminal {
    /// 创建新的终端实例
    pub fn new() -> Self
}
```

#### 5.1.2 数据输入

```rust
/// 输入数据（调用者负责调用）
pub fn feed(&mut self, data: &[u8])
```

**参数**:
- `data` - 原始字节数据

**说明**: 解析 ANSI 转义码并更新输出缓冲

#### 5.1.3 输出获取

```rust
/// 获取格式化输出（用于 egui 渲染）
pub fn get_formatted_output(&self) -> String

/// 转换为纯文本
pub fn to_plain_text(&self) -> String
```

### 5.2 CharStyle

字符样式。

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharStyle {
    pub foreground: Color32,
    pub background: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}
```

### 5.3 AnsiState

ANSI 解析状态机。

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum AnsiState {
    Normal,
    Escape,
    Csi,
    CsiParam(String),
}
```

---

## 6. 数据格式

### 6.1 会话配置文件格式

**文件**: `sessions.json`

```json
[
  {
    "name": "Production Server",
    "host": "192.0.2.10",
    "port": 22,
    "username": "ubuntu",
    "password": "your_password"
  },
  {
    "name": "Development Server",
    "host": "192.168.1.100",
    "port": 2222,
    "username": "developer",
    "password": "dev_password"
  }
]
```

### 6.2 SSH 消息格式

```rust
// 内部消息格式（非序列化）
enum SshMessage {
    Connected,           // 无数据
    Disconnected,        // 无数据
    Output(Vec<u8>),     // 字节数组
    Error(String),       // 错误信息
}
```

---

## 7. 错误码

### 7.1 SSH 错误

| 错误类型 | 错误信息前缀 | 说明 |
|---------|------------|-----|
| 连接失败 | `TCP connect failed:` | TCP 连接超时或拒绝 |
| SSH 握手 | `SSH handshake failed:` | SSH 协议协商失败 |
| 认证失败 | `Authentication failed:` | 用户名或密码错误 |
| 通道错误 | `Channel error:` | Shell 通道创建失败 |
| IO 错误 | `IO error:` | 读写错误 |

### 7.2 会话错误

| 错误类型 | 说明 |
|---------|-----|
| 加载失败 | 配置文件不存在或格式错误 |
| 保存失败 | 文件写入失败 |
| 解析错误 | JSON 解析失败 |

### 7.3 错误处理建议

```rust
// 示例：错误处理
match client.connect() {
    Ok(_) => {
        // 连接成功
    }
    Err(e) if e.contains("TCP connect failed") => {
        // 检查网络、防火墙、服务器状态
        show_error("无法连接到服务器，请检查网络");
    }
    Err(e) if e.contains("Authentication failed") => {
        // 提示重新输入密码
        show_error("用户名或密码错误");
    }
    Err(e) => {
        // 显示原始错误
        show_error(format!("连接失败：{}", e));
    }
}
```

---

## 📚 相关文档

- [架构文档](./ARCHITECTURE.md)
- [模块设计](./MODULE-DESIGN.md)
- [技术栈](./TECH-STACK.md)
