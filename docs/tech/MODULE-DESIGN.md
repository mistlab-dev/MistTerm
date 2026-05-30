# MistTerm 模块详细设计

## 📋 目录

1. [模块概览](#1-模块概览)
2. [UI 层模块](#2-ui-层模块)
3. [核心层模块](#3-核心层模块)
4. [SSH 层模块](#4-ssh-层模块)
5. [终端层模块](#5-终端层模块)
6. [模块接口](#6-模块接口)
7. [错误处理](#7-错误处理)

---

## 1. 模块概览

### 1.1 模块依赖图

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                               │
│                    (应用入口)                                 │
└─────────────────────────────────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
    ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
    │    ui/      │ │   core/     │ │  terminal/  │
    │  (UI 层)     │ │  (核心层)   │ │  (终端层)   │
    └──────┬──────┘ └──────┬──────┘ └──────┬──────┘
           │               │               │
           │         ┌─────┴─────┐         │
           │         ▼           │         │
           │   ┌───────────┐     │         │
           └──►│   ssh/    │◄────┘         │
               │  (SSH 层) │               │
               └────┬──────┘               │
                    │                      │
                    ▼                      │
              ┌───────────┐               │
              │  libssh2  │               │
              │  (C 库)   │               │
              └───────────┘               │
                                          │
                                    ┌───────────┐
                                    │   egui    │
                                    │  (渲染)   │
                                    └───────────┘
```

### 1.2 模块职责矩阵

| 模块 | 主要职责 | 依赖 | 被依赖 |
|-----|---------|-----|--------|
| main.rs | 应用启动 | ui::MistTermApp | - |
| ui::app | UI 渲染、事件处理 | core, ssh, terminal | main |
| core::session | 会话配置管理 | serde | ui, core::connection |
| core::connection | 连接状态管理 | ssh, terminal | ui |
| ssh::client | SSH 客户端实现 | libssh2 | ssh::manager |
| ssh::manager | 连接池管理 | ssh::client | core::connection |
| terminal::emulator | ANSI 解析 | - | core::connection |

---

## 2. UI 层模块

### 2.1 模块结构

```
ui/
├── mod.rs          # 模块导出
└── app.rs          # 主应用实现
```

### 2.2 MistTermApp 类设计

```rust
pub struct MistTermApp {
    /// 会话管理器 - 管理保存的会话配置
    session_manager: SessionManager,
    
    /// 连接管理器 - 管理活动连接
    connection_manager: Option<ConnectionManager>,
    
    /// SSH 消息接收器 - 接收 SSH 层消息
    message_rx: Option<std::sync::mpsc::Receiver<SshMessage>>,
    
    /// 当前选中的会话索引
    selected_session: Option<usize>,
    
    /// 是否显示连接对话框
    showing_connect_dialog: bool,
    
    /// 新连接配置（连接对话框使用）
    new_config: SessionConfig,
    
    /// 输入框文本缓存
    input_buffer: HashMap<String, String>,
}
```

### 2.3 核心方法设计

#### 2.3.1 update() - 主循环

```rust
fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame)
```

**职责**:
1. 处理 SSH 消息（非阻塞读取）
2. 渲染主界面
3. 渲染对话框（如果显示）

**调用流程**:
```
每帧调用
    │
    ├─► 处理 SSH 消息
    │       │
    │       └─► 更新终端输出
    │
    ├─► 渲染主界面
    │       │
    │       ├─► render_header()
    │       ├─► render_session_list()
    │       └─► render_terminal()
    │
    └─► 渲染对话框（可选）
            │
            └─► render_connect_dialog()
```

#### 2.3.2 connect_session() - 连接会话

```rust
fn connect_session(&mut self, idx: usize)
```

**职责**:
1. 获取会话配置
2. 设置连接状态为 Connecting
3. 启动异步连接线程
4. 连接成功后更新状态

**异步流程**:
```
主线程
    │
    └─► 启动新线程
            │
            ├─► SSH 连接
            ├─► 密码认证
            ├─► 打开 Shell 通道
            └─► 更新状态为 Connected/Error
```

#### 2.3.3 render_session_list() - 渲染会话列表

```rust
fn render_session_list(&mut self, ui: &mut egui::Ui)
```

**UI 布局**:
```
┌─────────────────────────────────────┐
│ Sessions:                           │
│ ┌─────────────────────────────────┐ │
│ │ ○ session1 - host1 (Connected)  │ │
│ │   [Connect] [X]                 │ │
│ ├─────────────────────────────────┤ │
│ │ ● session2 - host2 (Error)      │ │
│ │   [Connect]                     │ │
│ └─────────────────────────────────┘ │
└─────────────────────────────────────┘
```

**交互**:
- 点击会话名称 - 选中会话
- 点击 Connect - 发起连接
- 点击 X - 删除已连接会话

#### 2.3.4 render_terminal() - 渲染终端

```rust
fn render_terminal(&mut self, ui: &mut egui::Ui)
```

**UI 布局**:
```
┌─────────────────────────────────────┐
│ user@host:22                        │
│ ✓ Connected                         │
│ ─────────────────────────────────── │
│                                     │
│  ubuntu@server:~$                   │
│  ls -la                             │
│  total 24                           │
│  drwxr-xr-x  5 user  user  4096 ... │
│                                     │
│  ─────────────────────────────────  │
│  ➤ [输入框]                         │
└─────────────────────────────────────┘
```

#### 2.3.5 render_input() - 渲染输入框

```rust
fn render_input(&mut self, ui: &mut egui::Ui, idx: usize)
```

**职责**:
1. 渲染输入提示符（➤）
2. 渲染文本输入框
3. 自动聚焦
4. 检测 Enter 键
5. 发送命令到 SSH 通道

**事件处理**:
```
用户输入
    │
    ├─► 文本变化 ──► 更新缓存
    │
    └─► 按 Enter ──► 发送命令
                        │
                        ├─► 格式化（添加\n）
                        ├─► 发送到 SSH 通道
                        └─► 清空输入框
```

#### 2.3.6 render_connect_dialog() - 渲染连接对话框

```rust
fn render_connect_dialog(&mut self, ctx: &egui::Context)
```

**UI 布局**:
```
┌─────────────────────────────────────┐
│  Connect to Server            [×]   │
│ ─────────────────────────────────── │
│                                     │
│  Name:   [___________________]      │
│  Host:   [___________________]      │
│  Port:   [____]                     │
│  Username: [_________________]      │
│  Password: [________________] ●     │
│                                     │
│  ─────────────────────────────────  │
│  [Connect]       [Cancel]           │
└─────────────────────────────────────┘
```

**表单验证**:
- Host 不能为空
- Username 不能为空
- Port 范围 1-65535

---

## 3. 核心层模块

### 3.1 会话管理 (session.rs)

#### 3.1.1 SessionConfig 结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub name: String,      // 会话名称
    pub host: String,      // 服务器地址
    pub port: u16,         // 端口号
    pub username: String,  // 用户名
    pub password: String,  // 密码（明文存储，待加密）
}
```

#### 3.1.2 SessionManager 类

```rust
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    sessions_file: PathBuf,
}
```

**核心方法**:

| 方法 | 职责 | 返回值 |
|-----|------|-------|
| `new()` | 创建管理器，加载已保存会话 | Self |
| `add_session()` | 添加新会话 | usize (索引) |
| `remove_session()` | 删除会话 | () |
| `get_sessions()` | 获取所有会话 | &[SessionConfig] |
| `save()` | 持久化到文件 | Result<(), Error> |
| `load()` | 从文件加载 | Result<(), Error> |

**持久化格式**:
```json
[
  {
    "name": "Production Server",
    "host": "192.0.2.10",
    "port": 22,
    "username": "ubuntu",
    "password": "your_password"
  }
]
```

### 3.2 连接管理 (connection.rs)

#### 3.2.1 连接状态枚举

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,      // 未连接
    Connecting,        // 连接中
    Connected,         // 已连接
    Error(String),     // 错误（带错误信息）
}
```

#### 3.2.2 SshSessionState 结构

```rust
pub struct SshSessionState {
    pub config: SessionConfig,      // 会话配置
    pub state: ConnectionState,     // 连接状态
    pub terminal: Terminal,         // 终端模拟器
    pub handle: Option<SshSessionHandle>,  // SSH 通道句柄
}
```

**方法**:
```rust
impl SshSessionState {
    pub fn new(config: SessionConfig) -> Self
    pub fn status_text(&self) -> String
}
```

#### 3.2.3 ConnectionManager 类

```rust
pub struct ConnectionManager {
    sessions: Vec<Arc<Mutex<SshSessionState>>>,
    ssh_manager: SshManager,
    message_rx: Option<Receiver<SshMessage>>,
}
```

**核心方法**:

| 方法 | 职责 |
|-----|------|
| `new()` | 创建管理器 |
| `add_session()` | 添加新会话 |
| `get_session()` | 获取会话引用 |
| `get_sessions()` | 获取所有会话 |
| `get_ssh_manager()` | 获取 SSH 管理器 |
| `handle_ssh_message()` | 处理 SSH 消息 |
| `remove_session()` | 删除会话 |

**线程安全设计**:
```rust
// 使用 Arc<Mutex<T>> 实现线程安全
let session = Arc::new(Mutex::new(state));
sessions.push(session.clone());

// 访问时加锁
{
    let mut sess = session.lock();
    sess.state = ConnectionState::Connected;
}
```

---

## 4. SSH 层模块

### 4.1 SSH 客户端 (client.rs)

#### 4.1.1 SshConfig 结构

```rust
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}
```

#### 4.1.2 SshClient 类

```rust
pub struct SshClient {
    config: SshConfig,
    session: Option<ssh2::Session>,
}
```

**核心方法**:

| 方法 | 职责 | 返回值 |
|-----|------|-------|
| `new()` | 创建客户端 | Self |
| `connect()` | 建立连接 | Result<(), String> |
| `authenticate()` | 密码认证 | Result<(), String> |
| `open_shell()` | 打开 Shell 通道 | Result<Channel, String> |
| `is_connected()` | 检查连接状态 | bool |
| `disconnect()` | 断开连接 | () |

**连接流程**:
```
1. 创建 Session
   │
   ▼
2. 设置超时
   │
   ▼
3. TCP 连接
   │
   ▼
4. SSH 握手
   │
   ▼
5. 密码认证
   │
   ▼
6. 连接成功
```

**错误处理**:
```rust
pub fn connect(&mut self) -> Result<(), String> {
    // TCP 连接
    let addr = format!("{}:{}", self.config.host, self.config.port);
    let tcp = std::net::TcpStream::connect(addr)
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    
    // SSH 会话
    let mut session = Session::new()
        .map_err(|e| format!("Session create failed: {}", e))?;
    
    // 设置超时
    session.set_timeout(5000);
    
    // 握手
    session.handshake(&tcp)
        .map_err(|e| format!("SSH handshake failed: {}", e))?;
    
    // 认证
    self.authenticate()?;
    
    self.session = Some(session);
    Ok(())
}
```

### 4.2 连接管理器 (manager.rs)

#### 4.2.1 SshManager 类

```rust
pub struct SshManager {
    sessions: Arc<Mutex<Vec<SshClient>>>,
    message_tx: Sender<SshMessage>,
    next_session_id: usize,
}
```

#### 4.2.2 SshMessage 枚举

```rust
pub enum SshMessage {
    Connected,           // 连接成功
    Disconnected,        // 断开连接
    Output(Vec<u8>),     // 输出数据
    Error(String),       // 错误信息
}
```

**核心方法**:

| 方法 | 职责 |
|-----|------|
| `new()` | 创建管理器 |
| `create_session_async()` | 异步创建会话 |
| `start_interactive_shell()` | 启动交互 Shell |
| `handle_ssh_message()` | 处理消息 |
| `get_sessions()` | 获取会话列表 |

**异步连接设计**:
```rust
pub fn create_session_async(
    &mut self,
    config: SshConfig
) -> Result<SshSessionId, String> {
    let session_id = self.next_session_id;
    self.next_session_id += 1;
    
    let sessions = self.sessions.clone();
    let message_tx = self.message_tx.clone();
    
    // 在后台线程执行
    thread::spawn(move || {
        let mut client = SshClient::new(config);
        
        match client.connect() {
            Ok(_) => {
                // 添加到会话列表
                sessions.lock().unwrap().push(client);
                message_tx.send(SshMessage::Connected).ok();
            }
            Err(e) => {
                message_tx.send(SshMessage::Error(e)).ok();
            }
        }
    });
    
    Ok(session_id)
}
```

---

## 5. 终端层模块

### 5.1 ANSI 解析器 (emulator.rs)

#### 5.1.1 状态机设计

```rust
pub enum AnsiState {
    Normal,        // 正常模式
    Escape,        // ESC 键
    Csi,           // CSI 序列
    CsiParam(String),  // CSI 参数
}
```

#### 5.1.2 字符样式

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

#### 5.1.3 Terminal 类

```rust
pub struct Terminal {
    output: VecDeque<String>,  // 输出缓冲
    style: CharStyle,          // 当前样式
    ansi_state: AnsiState,     // ANSI 状态
}
```

**核心方法**:

| 方法 | 职责 |
|-----|------|
| `new()` | 创建终端 |
| `feed()` | 输入数据 |
| `get_formatted_output()` | 获取格式化输出 |
| `to_plain_text()` | 转换为纯文本 |

**ANSI 解析流程**:
```
输入字节
    │
    ├─► ESC ──► 进入 Escape 状态
    │
    ├─► [ ──► 进入 Csi 状态
    │
    └─► 普通字符 ──► 添加到输出
```

**支持的 ANSI 序列**:
- 颜色设置（前景/背景）
- 文本样式（粗体/斜体/下划线）
- 光标移动
- 清屏/清行

---

## 6. 模块接口

### 6.1 公共导出

```rust
// src/lib.rs (或各模块 mod.rs)

// UI 层
pub mod ui;
pub use ui::MistTermApp;

// 核心层
pub mod core;
pub use core::{SessionManager, SessionConfig, ConnectionManager, ConnectionState};

// SSH 层
pub mod ssh;
pub use ssh::{SshClient, SshConfig, SshManager, SshMessage};

// 终端层
pub mod terminal;
pub use terminal::Terminal;
```

### 6.2 模块间调用关系

```
UI 层调用:
    MistTermApp
        ├──► SessionManager::get_sessions()
        ├──► ConnectionManager::connect_session()
        └──► Terminal::get_formatted_output()

核心层调用:
    ConnectionManager
        ├──► SshManager::create_session_async()
        └──► Terminal::feed()

SSH 层调用:
    SshClient
        └──► libssh2 (FFI)
```

---

## 7. 错误处理

### 7.1 错误类型

```rust
// SSH 错误
pub enum SshError {
    ConnectionFailed(String),
    AuthenticationFailed(String),
    ChannelError(String),
    IoError(std::io::Error),
}

// 会话错误
pub enum SessionError {
    LoadError(std::io::Error),
    SaveError(std::io::Error),
    ParseError(serde_json::Error),
}
```

### 7.2 错误处理策略

| 错误类型 | 处理策略 |
|---------|---------|
| SSH 连接失败 | 显示错误信息，允许重试 |
| 认证失败 | 显示错误，要求重新输入密码 |
| 文件读写错误 | 显示警告，使用默认配置 |
| 解析错误 | 丢弃损坏数据，继续运行 |

### 7.3 错误展示

```
┌─────────────────────────────────────┐
│  Connection Error              [×]  │
│ ─────────────────────────────────── │
│                                     │
│  Failed to connect to server        │
│                                     │
│  Error: TCP connect failed:         │
│  Connection timed out               │
│                                     │
│  ─────────────────────────────────  │
│  [Retry]         [Cancel]           │
└─────────────────────────────────────┘
```

---

## 📚 相关文档

- [架构文档](./ARCHITECTURE.md)
- [技术栈](./TECH-STACK.md)
- [API 文档](./api.md) (待创建)
