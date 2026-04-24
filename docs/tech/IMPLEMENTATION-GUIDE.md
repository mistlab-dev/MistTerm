# MistTerm 技术实现指南

> **文档版本**: 1.0  
> **最后更新**: 2026-04-24  
> **状态**: 开发中  
> **目标**: 指导完整实现 MistTerm v1.0

---

## 📋 目录

1. [项目概述](#1-项目概述)
2. [技术架构](#2-技术架构)
3. [核心模块实现](#3-核心模块实现)
4. [lrzsz 协议实现](#4-lrzsz-协议实现)
5. [Git 同步实现](#5-git-同步实现)
6. [三平台打包](#6-三平台打包)
7. [系统密钥链](#7-系统密钥链)
8. [测试指南](#8-测试指南)

---

## 1. 项目概述

### 1.1 产品定位

MistTerm 是一款面向开发者和运维人员的现代化 SSH 终端工具，核心特性：

- ✅ **lrzsz 协议** - 终端内直接文件传输（P0 必须）
- ✅ **Git 同步** - 团队共享配置（P1 必须）
- ✅ **三平台支持** - macOS/Windows/Linux（P0 必须）
- ✅ **系统密钥链** - 安全存储密码（P1 必须）

### 1.2 技术栈

| 模块 | 技术选型 | 说明 |
|-----|---------|------|
| UI 框架 | `egui` + `eframe` | 跨平台即时模式 GUI |
| SSH | `ssh2` | libssh2 的 Rust 绑定 |
| 终端 | `vte` + `unicode-width` | ANSI 解析 |
| Git | `git2` | libgit2 的 Rust 绑定 |
| 密钥链 | `keyring` | 跨平台密钥存储 |
| 文件对话框 | `rfd` | 原生文件选择器 |
| 异步运行时 | `tokio` | 异步 I/O |
| 日志 | `tracing` + `tracing-subscriber` | 结构化日志 |

### 1.3 项目结构

```
MistTerm/
├── Cargo.toml              # 项目配置
├── src/
│   ├── main.rs             # 应用入口
│   ├── lib.rs              # 库导出
│   ├── ui/                 # UI 层
│   │   ├── mod.rs
│   │   ├── app.rs          # 主应用
│   │   ├── terminal.rs     # 终端渲染
│   │   ├── sidebar.rs      # 侧边栏
│   │   └── dialogs.rs      # 对话框
│   ├── core/               # 核心层
│   │   ├── mod.rs
│   │   ├── session.rs      # 会话管理
│   │   ├── connection.rs   # 连接管理
│   │   └── fragment.rs     # 命令片段
│   ├── ssh/                # SSH 层
│   │   ├── mod.rs
│   │   ├── client.rs       # SSH 客户端
│   │   └── manager.rs      # 连接池
│   ├── terminal/           # 终端层
│   │   ├── mod.rs
│   │   ├── emulator.rs     # 终端模拟
│   │   └── ansi.rs         # ANSI 解析
│   ├── sync/               # 同步层
│   │   ├── mod.rs
│   │   ├── git.rs          # Git 同步
│   │   └── config.rs       # 配置同步
│   ├── security/           # 安全层
│   │   ├── mod.rs
│   │   └── keyring.rs      # 密钥链
│   └── lrzsz/              # lrzsz 层
│       ├── mod.rs
│       ├── detector.rs     # 命令检测
│       ├── zmodem.rs       # ZMODEM 协议
│       └── transfer.rs     # 传输管理
├── resources/              # 资源文件
│   ├── icons/
│   └── themes/
└── tests/                  # 测试
    ├── integration/
    └── fixtures/
```

### 1.4 Cargo.toml 配置

```toml
[package]
name = "mistterm"
version = "0.1.0"
edition = "2021"
authors = ["MistTerm Team"]

[dependencies]
# UI
eframe = "0.24"
egui = "0.24"
egui_extras = { version = "0.24", features = ["all_loaders"] }

# SSH
ssh2 = "0.9"

# 终端
vte = "0.11"
unicode-width = "0.1"

# Git
git2 = "0.18"

# 安全
keyring = "2.3"

# 文件对话框
rfd = "0.12"

# 异步
tokio = { version = "1.35", features = ["full"] }
async-trait = "0.1"

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# 跨平台
[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["winuser"] }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = "3.9"
mockall = "0.12"

[profile.release]
opt-level = 3
lto = true
strip = true
```

---

## 2. 技术架构

### 2.1 分层架构

```
┌─────────────────────────────────────────────────────────────┐
│                        main.rs                               │
│                    (应用入口)                                 │
└─────────────────────────────────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
    ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
    │    ui/      │ │   core/     │ │   lrzsz/    │
    │  (UI 层)     │ │  (核心层)   │ │ (传输层)    │
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

### 2.2 数据流

```
用户输入
    │
    ▼
┌─────────────┐
│  egui 事件   │
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  ui::app    │
│  update()   │
└──────┬──────┘
       │
       ├─────────────────┬─────────────────┐
       ▼                 ▼                 ▼
┌───────────┐    ┌───────────┐    ┌───────────┐
│ SSH 命令   │    │ lrzsz 检测 │    │ 配置修改  │
└─────┬─────┘    └─────┬─────┘    └─────┬─────┘
      │                │                │
      ▼                ▼                ▼
┌───────────┐    ┌───────────┐    ┌───────────┐
│ ssh::     │    │ lrzsz::   │    │ sync::    │
│ client    │    │ detector  │    │ git       │
└─────┬─────┘    └─────┬─────┘    └─────┬─────┘
      │                │                │
      ▼                ▼                ▼
┌───────────┐    ┌───────────┐    ┌───────────┐
│ libssh2   │    │ ZMODEM    │    │ git2      │
│ (C 库)     │    │ 协议      │    │ (C 库)     │
└───────────┘    └───────────┘    └───────────┘
```

### 2.3 线程模型

```
主线程 (egui 事件循环)
├── UI 渲染
├── 事件处理
└── 消息接收

SSH 工作线程池
├── 连接线程 1
├── 连接线程 2
└── 数据收发线程

lrzsz 处理线程
├── 命令检测
├── ZMODEM 协议
└── 文件传输

Git 同步线程
├── 定时拉取
└── 定时推送
```

---

## 3. 核心模块实现

### 3.1 会话管理 (core/session.rs)

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::fs;

/// 会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub id: String,           // 唯一 ID
    pub name: String,         // 会话名称
    pub host: String,         // 服务器地址
    pub port: u16,            // 端口号
    pub username: String,     // 用户名
    pub password_id: Option<String>, // 密码 ID（存储在密钥链）
    pub created_at: u64,      // 创建时间戳
    pub updated_at: u64,      // 更新时间戳
    pub last_connected: Option<u64>, // 最后连接时间
}

/// 会话管理器
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    sessions_file: PathBuf,
}

impl SessionManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        let sessions_file = Self::get_default_path();
        let sessions = Self::load_from_file(&sessions_file).unwrap_or_default();
        
        SessionManager {
            sessions,
            sessions_file,
        }
    }
    
    /// 获取默认配置文件路径
    fn get_default_path() -> PathBuf {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MistTerm");
        
        // 创建配置目录
        fs::create_dir_all(&config_dir).ok();
        
        config_dir.join("sessions.json")
    }
    
    /// 添加会话
    pub fn add_session(&mut self, config: SessionConfig) -> Result<(), Box<dyn std::error::Error>> {
        self.sessions.push(config);
        self.save()?;
        Ok(())
    }
    
    /// 删除会话
    pub fn remove_session(&mut self, id: &str) -> bool {
        let initial_len = self.sessions.len();
        self.sessions.retain(|s| s.id != id);
        if self.sessions.len() != initial_len {
            self.save().ok();
            true
        } else {
            false
        }
    }
    
    /// 获取所有会话
    pub fn get_sessions(&self) -> &[SessionConfig] {
        &self.sessions
    }
    
    /// 获取会话
    pub fn get_session(&self, id: &str) -> Option<&SessionConfig> {
        self.sessions.iter().find(|s| s.id == id)
    }
    
    /// 保存会话
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.sessions)?;
        fs::write(&self.sessions_file, json)?;
        Ok(())
    }
    
    /// 从文件加载
    fn load_from_file(path: &PathBuf) -> Result<Vec<SessionConfig>, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        
        let json = fs::read_to_string(path)?;
        let sessions: Vec<SessionConfig> = serde_json::from_str(&json)?;
        Ok(sessions)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
```

### 3.2 SSH 客户端 (ssh/client.rs)

```rust
use ssh2::Session as Ssh2Session;
use std::net::TcpStream;
use std::sync::mpsc::{Sender, Receiver};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SshError {
    #[error("TCP 连接失败：{0}")]
    ConnectionFailed(String),
    
    #[error("SSH 握手失败：{0}")]
    HandshakeFailed(String),
    
    #[error("认证失败：{0}")]
    AuthenticationFailed(String),
    
    #[error("通道错误：{0}")]
    ChannelError(String),
    
    #[error("IO 错误：{0}")]
    IoError(#[from] std::io::Error),
}

/// SSH 消息
#[derive(Debug, Clone)]
pub enum SshMessage {
    Connected,
    Disconnected,
    Output(Vec<u8>),
    Error(String),
    LrzszReady(LrzszType),
}

/// lrzsz 类型
#[derive(Debug, Clone)]
pub enum LrzszType {
    Upload,
    Download(String),
}

/// SSH 客户端
pub struct SshClient {
    host: String,
    port: u16,
    username: String,
    password: String,
    session: Option<Ssh2Session>,
    channel: Option<ssh2::Channel>,
}

impl SshClient {
    /// 创建新的 SSH 客户端
    pub fn new(host: String, port: u16, username: String, password: String) -> Self {
        SshClient {
            host,
            port,
            username,
            password,
            session: None,
            channel: None,
        }
    }
    
    /// 建立 SSH 连接
    pub fn connect(&mut self) -> Result<(), SshError> {
        // TCP 连接
        let addr = format!("{}:{}", self.host, self.port);
        let tcp = TcpStream::connect(&addr)
            .map_err(|e| SshError::ConnectionFailed(e.to_string()))?;
        
        // SSH 会话
        let mut session = Ssh2Session::new()?;
        session.set_timeout(5000);
        
        // SSH 握手
        session.handshake(&tcp)
            .map_err(|e| SshError::HandshakeFailed(e.to_string()))?;
        
        // 密码认证
        session.userauth_password(&self.username, &self.password)
            .map_err(|e| SshError::AuthenticationFailed(e.to_string()))?;
        
        self.session = Some(session);
        Ok(())
    }
    
    /// 打开 Shell 通道
    pub fn open_shell(&mut self) -> Result<(), SshError> {
        let session = self.session.as_ref()
            .ok_or_else(|| SshError::ConnectionFailed("未连接".to_string()))?;
        
        let mut channel = session.channel_session()
            .map_err(|e| SshError::ChannelError(e.to_string()))?;
        
        channel.shell()
            .map_err(|e| SshError::ChannelError(e.to_string()))?;
        
        self.channel = Some(channel);
        Ok(())
    }
    
    /// 发送数据
    pub fn send(&mut self, data: &[u8]) -> Result<usize, SshError> {
        let channel = self.channel.as_mut()
            .ok_or_else(|| SshError::ConnectionFailed("未打开通道".to_string()))?;
        
        let sent = channel.write(data)?;
        channel.flush()?;
        Ok(sent)
    }
    
    /// 接收数据（非阻塞）
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, SshError> {
        let channel = self.channel.as_mut()
            .ok_or_else(|| SshError::ConnectionFailed("未打开通道".to_string()))?;
        
        match channel.read(buf) {
            Ok(0) => Err(SshError::ConnectionFailed("连接已关闭".to_string())),
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(0),
            Err(e) => Err(SshError::IoError(e)),
        }
    }
    
    /// 检查是否有数据可读
    pub fn has_data(&self) -> bool {
        if let Some(ref channel) = self.channel {
            channel.eof() == false
        } else {
            false
        }
    }
    
    /// 断开连接
    pub fn disconnect(&mut self) {
        if let Some(ref mut channel) = self.channel {
            channel.send_eof().ok();
        }
        self.channel = None;
        self.session = None;
    }
    
    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.session.is_some() && self.channel.is_some()
    }
}

impl Drop for SshClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}
```

### 3.3 终端模拟器 (terminal/emulator.rs)

```rust
use std::collections::VecDeque;
use vte::Perform;
use unicode_width::UnicodeWidthChar;

const MAX_BUFFER_SIZE: usize = 10000; // 最大缓冲区行数

/// 终端模拟器
pub struct Terminal {
    buffer: VecDeque<String>,
    current_line: String,
    cursor_x: usize,
    cursor_y: usize,
}

impl Terminal {
    /// 创建新的终端
    pub fn new() -> Self {
        Terminal {
            buffer: VecDeque::new(),
            current_line: String::new(),
            cursor_x: 0,
            cursor_y: 0,
        }
    }
    
    /// 输入数据
    pub fn feed(&mut self, data: &[u8]) {
        // 简单实现：逐字节处理
        for &byte in data {
            self.process_byte(byte);
        }
        
        // 限制缓冲区大小
        while self.buffer.len() > MAX_BUFFER_SIZE {
            self.buffer.pop_front();
        }
    }
    
    /// 处理单个字节
    fn process_byte(&mut self, byte: u8) {
        match byte {
            b'\n' | b'\r' => {
                // 换行
                self.buffer.push_back(self.current_line.clone());
                self.current_line.clear();
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            b'\x08' => {
                // 退格
                if self.cursor_x > 0 {
                    self.current_line.pop();
                    self.cursor_x -= 1;
                }
            }
            b'\x1b' => {
                // ESC 序列，简化处理
                // 完整实现需要解析 ANSI 转义码
            }
            _ if byte >= 32 && byte < 127 => {
                // 可打印字符
                if let Some(c) = char::from_u32(byte as u32) {
                    self.current_line.push(c);
                    self.cursor_x += c.width().unwrap_or(1);
                }
            }
            _ => {
                // 忽略其他控制字符
            }
        }
    }
    
    /// 获取格式化输出
    pub fn get_output(&self) -> String {
        let mut output = String::new();
        
        for line in &self.buffer {
            output.push_str(line);
            output.push('\n');
        }
        
        output.push_str(&self.current_line);
        output
    }
    
    /// 清空屏幕
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.current_line.clear();
        self.cursor_x = 0;
        self.cursor_y = 0;
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## 4. lrzsz 协议实现

### 4.1 命令检测 (lrzsz/detector.rs)

```rust
use std::sync::mpsc::Sender;
use crate::ssh::SshMessage;

/// lrzsz 检测器
pub struct LrzszDetector {
    buffer: String,
    last_output: String,
}

/// lrzsz 事件
#[derive(Debug, Clone)]
pub enum LrzszEvent {
    UploadReady,
    DownloadReady(String),
}

impl LrzszDetector {
    /// 创建新的检测器
    pub fn new() -> Self {
        LrzszDetector {
            buffer: String::new(),
            last_output: String::new(),
        }
    }
    
    /// 检测 lrzsz 命令
    pub fn detect(&mut self, output: &str) -> Option<LrzszEvent> {
        self.buffer.push_str(output);
        self.last_output = output.to_string();
        
        // 检测 rz 命令（上传）
        if self.buffer.contains("rz ready") || 
           self.buffer.contains("rj") ||
           self.buffer.contains("Zmodem") {
            return Some(LrzszEvent::UploadReady);
        }
        
        // 检测 sz 命令（下载）
        if let Some(filename) = self.parse_sz_command(output) {
            return Some(LrzszEvent::DownloadReady(filename));
        }
        
        None
    }
    
    /// 解析 sz 命令
    fn parse_sz_command(&self, output: &str) -> Option<String> {
        // 匹配 "sz filename" 或 "Sending filename"
        if output.starts_with("sz ") {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }
        
        // 匹配 "Sending filename.txt"
        if output.starts_with("Sending ") {
            let parts: Vec<&str> = output.split_whitespace().collect();
            if parts.len() >= 2 {
                return Some(parts[1].to_string());
            }
        }
        
        None
    }
    
    /// 重置检测器
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_output.clear();
    }
}

impl Default for LrzszDetector {
    fn default() -> Self {
        Self::new()
    }
}
```

### 4.2 ZMODEM 协议 (lrzsz/zmodem.rs)

```rust
use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use crate::ssh::SshClient;

/// ZMODEM 传输状态
#[derive(Debug, Clone, PartialEq)]
pub enum ZmodemState {
    Idle,
    WaitingHeader,
    SendingHeader,
    SendingData,
    ReceivingData,
    Verifying,
    Finished,
}

/// ZMODEM 传输器
pub struct ZmodemTransfer {
    state: ZmodemState,
    file_path: Option<PathBuf>,
    file_size: u64,
    bytes_sent: u64,
    crc: u32,
}

impl ZmodemTransfer {
    /// 创建新的传输器
    pub fn new() -> Self {
        ZmodemTransfer {
            state: ZmodemState::Idle,
            file_path: None,
            file_size: 0,
            bytes_sent: 0,
            crc: 0,
        }
    }
    
    /// 上传文件
    pub async fn upload(
        &mut self,
        file_path: PathBuf,
        ssh_client: &mut SshClient,
        progress_tx: Sender<TransferProgress>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 打开文件
        let file = File::open(&file_path)?;
        let mut reader = BufReader::new(file);
        
        // 获取文件大小
        self.file_size = reader.get_ref().metadata()?.len();
        self.file_path = Some(file_path.clone());
        
        // 发送 ZRQINIT
        self.send_zrqinit(ssh_client).await?;
        
        // 发送文件头
        self.send_file_header(&file_path, ssh_client).await?;
        
        // 发送数据块
        self.send_data_blocks(&mut reader, ssh_client, &progress_tx).await?;
        
        // 发送结束
        self.send_eof(ssh_client).await?;
        
        self.state = ZmodemState::Finished;
        Ok(())
    }
    
    /// 下载文件
    pub async fn download(
        &mut self,
        save_path: PathBuf,
        ssh_client: &mut SshClient,
        progress_tx: Sender<TransferProgress>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 创建文件
        let file = File::create(&save_path)?;
        let mut writer = BufWriter::new(file);
        
        self.file_path = Some(save_path.clone());
        
        // 接收文件头
        let file_info = self.recv_file_header(ssh_client).await?;
        self.file_size = file_info.size;
        
        // 接收数据块
        self.recv_data_blocks(&mut writer, ssh_client, &progress_tx).await?;
        
        // 验证 CRC
        self.verify_crc(ssh_client).await?;
        
        self.state = ZmodemState::Finished;
        Ok(())
    }
    
    /// 发送 ZRQINIT
    async fn send_zrqinit(&mut self, client: &mut SshClient) -> Result<(), Box<dyn std::error::Error>> {
        // ZRQINIT: 31 30 30 30 30 04
        let zrqinit = b"\x80\x80\x80\x80\x04";
        client.send(zrqinit)?;
        Ok(())
    }
    
    /// 发送文件头
    async fn send_file_header(
        &mut self,
        path: &PathBuf,
        client: &mut SshClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 文件头格式（简化）
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        let header = format!("D0 {} 0 {} 0 0\n", filename, self.file_size);
        client.send(header.as_bytes())?;
        Ok(())
    }
    
    /// 发送数据块
    async fn send_data_blocks(
        &mut self,
        reader: &mut BufReader<File>,
        client: &mut SshClient,
        progress_tx: &Sender<TransferProgress>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = [0u8; 1024];
        
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            
            // 发送数据块（简化，实际需要 ZMODEM 帧格式）
            client.send(&buffer[..n])?;
            self.bytes_sent += n as u64;
            
            // 发送进度
            progress_tx.send(TransferProgress {
                bytes_transferred: self.bytes_sent,
                total_bytes: self.file_size,
                filename: self.file_path.as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            }).ok();
        }
        
        Ok(())
    }
    
    /// 发送 EOF
    async fn send_eof(&mut self, client: &mut SshClient) -> Result<(), Box<dyn std::error::Error>> {
        // ZEOF: 04
        client.send(&[0x04])?;
        Ok(())
    }
    
    /// 接收文件头
    async fn recv_file_header(
        &mut self,
        client: &mut SshClient,
    ) -> Result<FileInfo, Box<dyn std::error::Error>> {
        // 简化实现，实际需要解析 ZMODEM 帧
        Ok(FileInfo {
            name: String::new(),
            size: 0,
        })
    }
    
    /// 接收数据块
    async fn recv_data_blocks(
        &mut self,
        writer: &mut BufWriter<File>,
        client: &mut SshClient,
        progress_tx: &Sender<TransferProgress>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 简化实现
        Ok(())
    }
    
    /// 验证 CRC
    async fn verify_crc(
        &mut self,
        client: &mut SshClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 简化实现
        Ok(())
    }
    
    /// 获取当前状态
    pub fn state(&self) -> ZmodemState {
        self.state.clone()
    }
    
    /// 获取传输进度
    pub fn progress(&self) -> f32 {
        if self.file_size == 0 {
            0.0
        } else {
            (self.bytes_sent as f32 / self.file_size as f32) * 100.0
        }
    }
}

/// 文件信息
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
}

/// 传输进度
#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub filename: String,
}

impl Default for ZmodemTransfer {
    fn default() -> Self {
        Self::new()
    }
}
```

### 4.3 传输管理 (lrzsz/transfer.rs)

```rust
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use crate::ssh::SshClient;
use crate::lrzsz::{LrzszEvent, ZmodemTransfer, TransferProgress};

/// 传输管理器
pub struct TransferManager {
    transfers: Vec<ActiveTransfer>,
    progress_tx: Option<mpsc::Sender<TransferProgress>>,
}

/// 活动传输
pub struct ActiveTransfer {
    id: usize,
    file_path: PathBuf,
    state: TransferState,
}

#[derive(Debug, Clone)]
pub enum TransferState {
    Pending,
    Transferring,
    Completed,
    Failed(String),
}

impl TransferManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        TransferManager {
            transfers: Vec::new(),
            progress_tx: None,
        }
    }
    
    /// 设置进度回调
    pub fn set_progress_callback(&mut self, tx: mpsc::Sender<TransferProgress>) {
        self.progress_tx = Some(tx);
    }
    
    /// 处理 lrzsz 事件
    pub async fn handle_event(
        &mut self,
        event: LrzszEvent,
        ssh_client: &mut SshClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            LrzszEvent::UploadReady => {
                // 弹出文件选择器
                if let Some(file_path) = Self::show_upload_dialog() {
                    self.start_upload(file_path, ssh_client).await?;
                }
            }
            LrzszEvent::DownloadReady(filename) => {
                // 弹出保存对话框
                if let Some(save_path) = Self::show_download_dialog(&filename) {
                    self.start_download(filename, save_path, ssh_client).await?;
                }
            }
        }
        Ok(())
    }
    
    /// 开始上传
    async fn start_upload(
        &mut self,
        file_path: PathBuf,
        ssh_client: &mut SshClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = self.transfers.len();
        
        let mut transfer = ZmodemTransfer::new();
        let (progress_tx, mut progress_rx) = mpsc::channel(100);
        
        self.transfers.push(ActiveTransfer {
            id,
            file_path: file_path.clone(),
            state: TransferState::Transferring,
        });
        
        // 在后台执行传输
        tokio::spawn(async move {
            if let Err(e) = transfer.upload(file_path, ssh_client, progress_tx).await {
                eprintln!("上传失败：{}", e);
            }
        });
        
        Ok(())
    }
    
    /// 开始下载
    async fn start_download(
        &mut self,
        filename: String,
        save_path: PathBuf,
        ssh_client: &mut SshClient,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = self.transfers.len();
        
        let mut transfer = ZmodemTransfer::new();
        let (progress_tx, mut progress_rx) = mpsc::channel(100);
        
        self.transfers.push(ActiveTransfer {
            id,
            file_path: save_path.clone(),
            state: TransferState::Transferring,
        });
        
        // 在后台执行传输
        tokio::spawn(async move {
            if let Err(e) = transfer.download(save_path, ssh_client, progress_tx).await {
                eprintln!("下载失败：{}", e);
            }
        });
        
        Ok(())
    }
    
    /// 显示上传对话框
    fn show_upload_dialog() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("选择要上传的文件")
            .pick_file()
    }
    
    /// 显示下载对话框
    fn show_download_dialog(filename: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("选择保存位置")
            .set_file_name(filename)
            .save_file()
    }
    
    /// 获取传输列表
    pub fn get_transfers(&self) -> &[ActiveTransfer] {
        &self.transfers
    }
}

impl Default for TransferManager {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## 5. Git 同步实现

### 5.1 Git 仓库管理 (sync/git.rs)

```rust
use git2::{Repository, Signature, Index};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GitError {
    #[error("初始化仓库失败：{0}")]
    InitError(String),
    
    #[error("克隆仓库失败：{0}")]
    CloneError(String),
    
    #[error("提交失败：{0}")]
    CommitError(String),
    
    #[error("推送失败：{0}")]
    PushError(String),
    
    #[error("拉取失败：{0}")]
    PullError(String),
}

/// Git 仓库
pub struct GitRepo {
    repo: Repository,
    remote_url: String,
    branch: String,
}

impl GitRepo {
    /// 初始化本地仓库
    pub fn init(local_path: &PathBuf) -> Result<Self, GitError> {
        let repo = Repository::init(local_path)
            .map_err(|e| GitError::InitError(e.to_string()))?;
        
        Ok(GitRepo {
            repo,
            remote_url: String::new(),
            branch: "main".to_string(),
        })
    }
    
    /// 克隆远程仓库
    pub fn clone(remote_url: &str, local_path: &PathBuf) -> Result<Self, GitError> {
        let repo = Repository::clone(remote_url, local_path)
            .map_err(|e| GitError::CloneError(e.to_string()))?;
        
        Ok(GitRepo {
            repo,
            remote_url: remote_url.to_string(),
            branch: "main".to_string(),
        })
    }
    
    /// 设置远程 URL
    pub fn set_remote(&mut self, url: &str) -> Result<(), GitError> {
        if let Ok(mut remote) = self.repo.find_remote("origin") {
            remote.set_url(url).map_err(|e| GitError::PushError(e.to_string()))?;
            self.remote_url = url.to_string();
        }
        Ok(())
    }
    
    /// 添加文件到暂存区
    pub fn add(&self, path: &str) -> Result<(), GitError> {
        let mut index = self.repo.index()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        index.add_path(PathBuf::from(path).as_path())
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        index.write()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        Ok(())
    }
    
    /// 提交更改
    pub fn commit(
        &self,
        message: &str,
        author: &str,
        email: &str,
    ) -> Result<(), GitError> {
        let mut index = self.repo.index()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        index.write()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        let tree_id = index.write_tree()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        let tree = self.repo.find_tree(tree_id)
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        let signature = Signature::now(author, email)
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        let head = self.repo.head()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        let parent = head.peel_to_commit()
            .map_err(|e| GitError::CommitError(e.to_string()))?;
        
        self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        ).map_err(|e| GitError::CommitError(e.to_string()))?;
        
        Ok(())
    }
    
    /// 推送到远程
    pub fn push(&self) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote("origin")
            .map_err(|e| GitError::PushError(e.to_string()))?;
        
        let refs = &[format!("refs/heads/{}:refs/heads/{}", self.branch, self.branch)];
        
        remote.push(refs, None)
            .map_err(|e| GitError::PushError(e.to_string()))?;
        
        Ok(())
    }
    
    /// 从远程拉取
    pub fn pull(&self) -> Result<(), GitError> {
        let mut remote = self.repo.find_remote("origin")
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        remote.fetch(&[&self.branch], None, None)
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        let fetch_head = self.repo.find_reference("FETCH_HEAD")
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        let fetch_commit = self.repo.reference_to_annotated_commit(&fetch_head)
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        let head = self.repo.head()
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        let analysis = self.repo.merge_analysis(&[&fetch_commit])
            .map_err(|e| GitError::PullError(e.to_string()))?;
        
        if analysis.0.is_up_to_date() {
            return Ok(());
        }
        
        if analysis.0.is_fast_forward() {
            let mut refname = format!("refs/heads/{}", self.branch);
            match self.repo.find_branch(&self.branch, git2::BranchType::Local) {
                Ok(mut b) => {
                    b.get_mut()
                        .set_target(fetch_commit.id(), "Fast-Forward")
                        .map_err(|e| GitError::PullError(e.to_string()))?;
                }
                Err(_) => {
                    self.repo.branch(
                        &self.branch,
                        &self.repo.find_commit(fetch_commit.id())
                            .map_err(|e| GitError::PullError(e.to_string()))?,
                        false,
                    ).map_err(|e| GitError::PullError(e.to_string()))?;
                }
            }
        }
        
        Ok(())
    }
    
    /// 同步（拉取 + 推送）
    pub fn sync(&self) -> Result<(), GitError> {
        self.pull()?;
        self.push()?;
        Ok(())
    }
}
```

---

## 6. 三平台打包

### 6.1 macOS 打包

```bash
#!/bin/bash
# build-macos.sh

# 编译 Release 版本
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin

# 创建应用 Bundle
mkdir -p MistTerm.app/Contents/MacOS
mkdir -p MistTerm.app/Contents/Resources

# 复制二进制
cp target/x86_64-apple-darwin/release/mistterm MistTerm.app/Contents/MacOS/

# 创建 Info.plist
cat > MistTerm.app/Contents/Info.plist << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>MistTerm</string>
    <key>CFBundleExecutable</key>
    <string>mistterm</string>
    <key>CFBundleIdentifier</key>
    <string>com.mistterm.app</string>
    <key>CFBundleVersion</key>
    <string>1.0.0</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
</dict>
</plist>
EOF

# 代码签名
codesign --sign - MistTerm.app

# 创建 DMG
hdiutil create -volname MistTerm -srcfolder MistTerm.app -ov MistTerm.dmg
```

### 6.2 Windows 打包

```powershell
# build-windows.ps1

# 编译 Release 版本
cargo build --release --target x86_64-pc-windows-msvc

# 创建安装包目录
New-Item -ItemType Directory -Force -Path "dist\windows"
Copy-Item "target\x86_64-pc-windows-msvc\release\mistterm.exe" "dist\windows\"

# 使用 Inno Setup 创建安装包
# 编译 setup.iss
iscc.exe setup.iss
```

**setup.iss 示例**:
```ini
[Setup]
AppName=MistTerm
AppVersion=1.0.0
DefaultDirName={pf}\MistTerm
DefaultGroupName=MistTerm
OutputDir=dist
OutputBaseFilename=MistTerm-Setup

[Files]
Source: "target\x86_64-pc-windows-msvc\release\mistterm.exe"; DestDir: "{app}"

[Icons]
Name: "{group}\MistTerm"; Filename: "{app}\mistterm.exe"
```

### 6.3 Linux 打包

```bash
#!/bin/bash
# build-linux.sh

# 编译 Release 版本
cargo build --release

# 创建 AppImage
mkdir -p AppDir/usr/bin
cp target/release/mistterm AppDir/usr/bin/
cp icon.png AppDir/

# 创建 .desktop
cat > AppDir/mistterm.desktop << EOF
[Desktop Entry]
Name=MistTerm
Exec=mistterm
Icon=mistterm
Type=Application
Categories=System;TerminalEmulator;
EOF

# 构建 AppImage
appimagetool AppDir MistTerm-x86_64.AppImage

# 创建 deb 包
mkdir -p deb-package/DEBIAN
mkdir -p deb-package/usr/bin
mkdir -p deb-package/usr/share/icons/hicolor/256x256/apps

cat > deb-package/DEBIAN/control << EOF
Package: mistterm
Version: 1.0.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: MistTerm Team
Description: Modern SSH Terminal with lrzsz support
EOF

cp target/release/mistterm deb-package/usr/bin/
cp icon.png deb-package/usr/share/icons/hicolor/256x256/apps/mistterm.png

dpkg-deb --build deb-package mistterm_1.0.0_amd64.deb
```

---

## 7. 系统密钥链

### 7.1 密钥链管理 (security/keyring.rs)

```rust
use keyring::{Keyring, Error};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum KeyringError {
    #[error("保存密码失败：{0}")]
    SaveError(String),
    
    #[error("获取密码失败：{0}")]
    GetError(String),
    
    #[error("删除密码失败：{0}")]
    DeleteError(String),
}

/// 凭证管理器
pub struct CredentialManager {
    service: String,
}

impl CredentialManager {
    /// 创建新的凭证管理器
    pub fn new() -> Self {
        CredentialManager {
            service: "MistTerm".to_string(),
        }
    }
    
    /// 保存密码
    pub fn save_password(&self, username: &str, password: &str) -> Result<(), KeyringError> {
        let keyring = Keyring::new(&self.service, username);
        keyring.set_password(password)
            .map_err(|e| KeyringError::SaveError(e.to_string()))?;
        Ok(())
    }
    
    /// 获取密码
    pub fn get_password(&self, username: &str) -> Result<String, KeyringError> {
        let keyring = Keyring::new(&self.service, username);
        let password = keyring.get_password()
            .map_err(|e| KeyringError::GetError(e.to_string()))?;
        Ok(password)
    }
    
    /// 删除密码
    pub fn delete_password(&self, username: &str) -> Result<(), KeyringError> {
        let keyring = Keyring::new(&self.service, username);
        keyring.delete_password()
            .map_err(|e| KeyringError::DeleteError(e.to_string()))?;
        Ok(())
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## 8. 测试指南

### 8.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_manager() {
        let mut manager = SessionManager::new();
        
        let config = SessionConfig {
            id: "test-1".to_string(),
            name: "Test Server".to_string(),
            host: "192.168.1.1".to_string(),
            port: 22,
            username: "user".to_string(),
            password_id: None,
            created_at: 0,
            updated_at: 0,
            last_connected: None,
        };
        
        manager.add_session(config).unwrap();
        assert_eq!(manager.get_sessions().len(), 1);
    }
    
    #[test]
    fn test_terminal_feed() {
        let mut terminal = Terminal::new();
        terminal.feed(b"Hello, World!\n");
        
        let output = terminal.get_output();
        assert!(output.contains("Hello, World!"));
    }
}
```

### 8.2 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test -- ssh
cargo test -- terminal
cargo test -- lrzsz

# 显示测试输出
cargo test -- --nocapture

# 测试覆盖率
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

---

**文档结束**

> 💡 **备注**: 本文档提供完整的技术实现指导，包含代码示例和打包脚本。

---

## 9. UI 层详细实现

### 9.1 主应用 (ui/app.rs)

```rust
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender};
use crate::core::SessionManager;
use crate::ssh::{SshClient, SshMessage};
use crate::terminal::Terminal;
use crate::lrzsz::{LrzszDetector, TransferManager};

/// 主应用
pub struct MistTermApp {
    // 会话管理
    session_manager: SessionManager,
    selected_session: Option<usize>,
    
    // SSH 连接
    ssh_client: Option<SshClient>,
    message_rx: Option<Receiver<SshMessage>>,
    
    // 终端
    terminal: Terminal,
    input_buffer: String,
    
    // lrzsz
    lrzsz_detector: LrzszDetector,
    transfer_manager: TransferManager,
    
    // UI 状态
    showing_connect_dialog: bool,
    showing_settings: bool,
    new_config: SessionConfig,
    
    // 连接状态
    connection_state: ConnectionState,
}

/// 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl MistTermApp {
    /// 创建新应用
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 初始化字体
        let mut fonts = egui::FontDefinitions::default();
        // 添加等宽字体
        fonts.font_data.insert(
            "FiraCode".to_owned(),
            std::sync::Arc::new(
                egui::FontData::from_static(include_bytes!("../../resources/FiraCode-Regular.ttf"))
            ),
        );
        fonts.families.insert(
            egui::FontFamily::Monospace,
            vec!["FiraCode".to_owned()],
        );
        cc.egui_ctx.set_fonts(fonts);
        
        MistTermApp {
            session_manager: SessionManager::new(),
            selected_session: None,
            ssh_client: None,
            message_rx: None,
            terminal: Terminal::new(),
            input_buffer: String::new(),
            lrzsz_detector: LrzszDetector::new(),
            transfer_manager: TransferManager::new(),
            showing_connect_dialog: false,
            showing_settings: false,
            new_config: SessionConfig::default(),
            connection_state: ConnectionState::Disconnected,
        }
    }
    
    /// 主更新循环
    pub fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 处理 SSH 消息
        self.handle_ssh_messages();
        
        // 渲染界面
        self.render_ui(ctx);
    }
    
    /// 处理 SSH 消息
    fn handle_ssh_messages(&mut self) {
        if let Some(ref rx) = self.message_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SshMessage::Connected => {
                        self.connection_state = ConnectionState::Connected;
                    }
                    SshMessage::Disconnected => {
                        self.connection_state = ConnectionState::Disconnected;
                        self.ssh_client = None;
                    }
                    SshMessage::Output(data) => {
                        // 更新终端
                        self.terminal.feed(&data);
                        
                        // 检测 lrzsz
                        if let Some(event) = self.lrzsz_detector.detect(&String::from_utf8_lossy(&data)) {
                            // 处理 lrzsz 事件
                            tokio::spawn(async move {
                                // 处理传输
                            });
                        }
                    }
                    SshMessage::Error(err) => {
                        self.connection_state = ConnectionState::Error(err);
                    }
                    SshMessage::LrzszReady(_) => {
                        // lrzsz 就绪
                    }
                }
            }
        }
    }
    
    /// 渲染 UI
    fn render_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // 顶部栏
            self.render_header(ui);
            
            // 主内容区
            egui::SplitPanel::new(egui::Layout::left_to_right(egui::Align::TOP))
                .min_size(200.0)
                .show(ui, |ui| {
                    // 左侧边栏
                    ui.add_sized([240.0, f32::INFINITY], |ui: &mut egui::Ui| {
                        self.render_sidebar(ui)
                    });
                    
                    // 右侧终端
                    ui.add_sized([f32::INFINITY, f32::INFINITY], |ui: &mut egui::Ui| {
                        self.render_terminal(ui)
                    });
                });
            
            // 底部状态栏
            self.render_status_bar(ui);
        });
        
        // 对话框
        if self.showing_connect_dialog {
            self.render_connect_dialog(ctx);
        }
    }
    
    /// 渲染顶部栏
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("MistTerm");
            ui.separator();
            
            match &self.connection_state {
                ConnectionState::Connected => {
                    ui.label(egui::RichText::new("✓ 已连接").color(egui::Color32::GREEN));
                }
                ConnectionState::Connecting => {
                    ui.label(egui::RichText::new("连接中...").color(egui::Color32::YELLOW));
                }
                ConnectionState::Error(err) => {
                    ui.label(egui::RichText::new(format!("❌ {}", err)).color(egui::Color32::RED));
                }
                _ => {}
            }
        });
    }
    
    /// 渲染侧边栏
    fn render_sidebar(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            // 搜索框
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut String::new());
            });
            
            ui.add_space(10.0);
            
            // 会话列表
            ui.label("会话");
            
            let sessions = self.session_manager.get_sessions();
            for (idx, session) in sessions.iter().enumerate() {
                let is_selected = self.selected_session == Some(idx);
                
                if ui.selectable_label(is_selected, &session.name).clicked() {
                    self.selected_session = Some(idx);
                    self.connect_session(idx);
                }
            }
            
            ui.add_space(10.0);
            
            // 新建按钮
            if ui.button("+ 新建会话").clicked() {
                self.showing_connect_dialog = true;
            }
        });
        
        ui.response()
    }
    
    /// 渲染终端
    fn render_terminal(&mut self, ui: &mut egui::Ui) -> egui::Response {
        // 终端输出区域
        let output = self.terminal.get_output();
        
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(egui::RichText::new(output).family(egui::FontFamily::Monospace));
            });
        
        // 输入框
        ui.horizontal(|ui| {
            ui.label("➤");
            ui.text_edit_singleline(&mut self.input_buffer);
            
            if ui.button("发送").clicked() {
                self.send_command();
            }
        });
        
        ui.response()
    }
    
    /// 渲染状态栏
    fn render_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("SSH-2.0");
            ui.separator();
            ui.label("UTF-8");
            ui.separator();
            ui.label("14px");
        });
    }
    
    /// 渲染连接对话框
    fn render_connect_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("连接服务器")
            .collapsible(false)
            .resizable(true)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label("名称:");
                        ui.text_edit_singleline(&mut self.new_config.name);
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("主机:");
                        ui.text_edit_singleline(&mut self.new_config.host);
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("端口:");
                        ui.add(egui::DragValue::new(&mut self.new_config.port).range(1..=65535));
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("用户名:");
                        ui.text_edit_singleline(&mut self.new_config.username);
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("密码:");
                        ui.add(egui::TextEdit::singleline(&mut self.new_config.password)
                            .password(true));
                    });
                    
                    ui.add_space(20.0);
                    
                    ui.horizontal(|ui| {
                        if ui.button("连接").clicked() {
                            // 保存并连接
                            self.showing_connect_dialog = false;
                        }
                        
                        if ui.button("取消").clicked() {
                            self.showing_connect_dialog = false;
                        }
                    });
                });
            });
    }
    
    /// 连接会话
    fn connect_session(&mut self, idx: usize) {
        let sessions = self.session_manager.get_sessions();
        if let Some(session) = sessions.get(idx) {
            self.connection_state = ConnectionState::Connecting;
            
            // 创建 SSH 客户端
            let mut client = SshClient::new(
                session.host.clone(),
                session.port,
                session.username.clone(),
                session.password.clone(),
            );
            
            // 在后台线程连接
            let (tx, rx) = std::sync::mpsc::channel();
            self.message_rx = Some(rx);
            
            std::thread::spawn(move || {
                if let Err(e) = client.connect() {
                    tx.send(SshMessage::Error(e.to_string())).ok();
                    return;
                }
                
                tx.send(SshMessage::Connected).ok();
                
                if let Err(e) = client.open_shell() {
                    tx.send(SshMessage::Error(e.to_string())).ok();
                    return;
                }
                
                tx.send(SshMessage::Disconnected).ok();
            });
            
            self.ssh_client = Some(client);
        }
    }
    
    /// 发送命令
    fn send_command(&mut self) {
        if let Some(ref mut client) = self.ssh_client {
            let cmd = format!("{}\n", self.input_buffer);
            if let Err(e) = client.send(cmd.as_bytes()) {
                eprintln!("发送失败：{}", e);
            }
            self.input_buffer.clear();
        }
    }
}

impl Default for MistTermApp {
    fn default() -> Self {
        Self::new(&eframe::CreationContext::default())
    }
}
```

### 9.2 主入口 (main.rs)

```rust
use eframe::egui;
mod ui;
mod core;
mod ssh;
mod terminal;
mod lrzsz;
mod sync;
mod security;

use ui::MistTermApp;

fn main() -> eframe::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(load_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "MistTerm",
        native_options,
        Box::new(|cc| Ok(Box::new(MistTermApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    // 加载图标
    let icon = include_bytes!("../resources/icon.png");
    let image = image::load_from_memory(icon).unwrap();
    let rgba = image.into_rgba8();
    
    egui::IconData {
        rgba: rgba.into_raw(),
        width: image.width(),
        height: image.height(),
    }
}
```

---

## 10. 错误处理

### 10.1 统一错误类型

```rust
use thiserror::Error;

/// 应用级错误
#[derive(Error, Debug)]
pub enum AppError {
    #[error("SSH 错误：{0}")]
    Ssh(#[from] ssh::SshError),
    
    #[error("Git 错误：{0}")]
    Git(#[from] sync::GitError),
    
    #[error("密钥链错误：{0}")]
    Keyring(#[from] security::KeyringError),
    
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
    
    #[error("自定义错误：{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
```

### 10.2 错误处理策略

```rust
// SSH 连接错误处理
match client.connect() {
    Ok(_) => {
        // 连接成功
    }
    Err(SshError::ConnectionFailed(msg)) => {
        // 显示连接失败，建议检查网络
        show_error_dialog("连接失败", &format!("无法连接到服务器：{}", msg));
    }
    Err(SshError::AuthenticationFailed(msg)) => {
        // 显示认证失败，建议检查密码
        show_error_dialog("认证失败", &format!("用户名或密码错误：{}", msg));
    }
    Err(e) => {
        // 显示通用错误
        show_error_dialog("错误", &e.to_string());
    }
}
```

---

## 11. 调试指南

### 11.1 日志配置

```rust
// 在 main.rs 中
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::from_default_env()
            .add_directive("mistterm=debug".parse().unwrap())
            .add_directive("ssh2=info".parse().unwrap()),
    )
    .with_target(false)
    .with_thread_ids(true)
    .init();
```

### 11.2 运行调试

```bash
# 启用调试日志
RUST_LOG=mistterm=debug cargo run

# 只查看 SSH 相关日志
RUST_LOG=ssh2=debug cargo run

# 保存到文件
RUST_LOG=mistterm=debug cargo run 2>&1 | tee debug.log
```

### 11.3 性能分析

```bash
# 安装 flamegraph
cargo install flamegraph

# 生成火焰图
cargo flamegraph --bin mistterm

# 查看结果
open flamegraph.svg
```

---

## 12. CI/CD 配置

### 12.1 GitHub Actions

```yaml
# .github/workflows/build.yml
name: Build

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  build:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    
    steps:
    - uses: actions/checkout@v3
    
    - name: Install Rust
      uses: actions-rs/toolchain@v1
      with:
        toolchain: stable
    
    - name: Build
      run: cargo build --release
    
    - name: Test
      run: cargo test
    
    - name: Upload artifact
      uses: actions/upload-artifact@v3
      with:
        name: mistterm-${{ matrix.os }}
        path: target/release/mistterm*
```

---

**完整文档结束**
