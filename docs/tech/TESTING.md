# MistTerm 测试方案

## 📋 目录

1. [测试概述](#1-测试概述)
2. [单元测试](#2-单元测试)
3. [集成测试](#3-集成测试)
4. [端到端测试](#4-端到端测试)
5. [性能测试](#5-性能测试)
6. [安全测试](#6-安全测试)
7. [测试覆盖率](#7-测试覆盖率)

---

## 1. 测试概述

### 1.1 测试策略

```
┌─────────────────────────────────────────────────────────────┐
│                    测试金字塔                                │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│                      ┌─────┐                               │
│                     │  E2E │  (10%)                         │
│                    │ 测试  │                               │
│                   └───────┘                                │
│                 ┌───────────────┐                          │
│                │   集成测试      │  (20%)                   │
│               │   Integration   │                          │
│              └───────────────────┘                         │
│           ┌─────────────────────────────┐                  │
│          │       单元测试                │  (70%)           │
│         │       Unit Tests              │                  │
│        └─────────────────────────────────┘                 │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 测试类型

| 类型 | 工具 | 目标 | 频率 |
|-----|------|-----|------|
| 单元测试 | cargo test | 模块功能验证 | 每次提交 |
| 集成测试 | cargo test | 模块间交互 | 每日构建 |
| 端到端测试 | 手动/自动化 | 完整流程 | 发布前 |
| 性能测试 | cargo flamegraph | 性能指标 | 定期 |
| 安全测试 | cargo audit | 安全漏洞 | 定期 |

### 1.3 测试环境

```
┌─────────────────────────────────────────────────────────────┐
│                     测试环境                                 │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │  开发环境   │  │  测试环境   │  │  生产环境   │        │
│  │  (本地)     │  │  (远程)     │  │  (线上)     │        │
│  ├─────────────┤  ├─────────────┤  ├─────────────┤        │
│  │ 单元测试    │  │ 集成测试    │  │ 监控        │        │
│  │ 快速反馈    │  │ 持续集成    │  │ 告警        │        │
│  └─────────────┘  └─────────────┘  └─────────────┘        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

---

## 2. 单元测试

### 2.1 测试结构

```
src/
├── main.rs
├── ui/
│   └── app.rs          # 包含单元测试
├── core/
│   ├── session.rs      # 包含单元测试
│   └── connection.rs   # 包含单元测试
├── ssh/
│   ├── client.rs       # 包含单元测试
│   └── manager.rs      # 包含单元测试
└── terminal/
    └── emulator.rs     # 包含单元测试
```

### 2.2 SSH 层测试

#### 2.2.1 SshConfig 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_config_default() {
        let config = SshConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "test".to_string(),
            password: "pass".to_string(),
        };
        
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "test");
    }

    #[test]
    fn test_ssh_config_invalid_port() {
        // 端口范围验证
        let invalid_ports = vec![0, 65536, -1];
        
        for port in invalid_ports {
            // 这里应该验证端口范围
            // 实际实现中可以在构造函数中验证
        }
    }
}
```

#### 2.2.2 SshClient 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let config = SshConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "test".to_string(),
            password: "pass".to_string(),
        };
        
        let client = SshClient::new(config);
        assert!(!client.is_connected());
    }

    #[test]
    fn test_connection_refused() {
        // 测试连接被拒绝的情况
        let config = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 1,  // 通常没有服务监听
            username: "test".to_string(),
            password: "pass".to_string(),
        };
        
        let mut client = SshClient::new(config);
        let result = client.connect();
        
        // 预期失败
        assert!(result.is_err());
    }
}
```

### 2.3 核心层测试

#### 2.3.1 SessionConfig 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_session_config_serialization() {
        let config = SessionConfig {
            name: "Test Server".to_string(),
            host: "192.168.1.1".to_string(),
            port: 22,
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        
        // 序列化
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("Test Server"));
        
        // 反序列化
        let restored: SessionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.name, config.name);
        assert_eq!(restored.host, config.host);
    }

    #[test]
    fn test_session_config_clone() {
        let config = SessionConfig {
            name: "Test".to_string(),
            host: "host".to_string(),
            port: 22,
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        
        let cloned = config.clone();
        assert_eq!(cloned.name, config.name);
    }
}
```

#### 2.3.2 SessionManager 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_session_manager_new() {
        let manager = SessionManager::new();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_add_session() {
        let mut manager = SessionManager::new();
        
        let config = SessionConfig {
            name: "Test".to_string(),
            host: "host".to_string(),
            port: 22,
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        
        let idx = manager.add_session(config);
        assert_eq!(idx, 0);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_remove_session() {
        let mut manager = SessionManager::new();
        
        let config = SessionConfig {
            name: "Test".to_string(),
            host: "host".to_string(),
            port: 22,
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        
        manager.add_session(config);
        manager.remove_session(0);
        
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();
        let sessions_file = temp_dir.path().join("sessions.json");
        
        // 创建并保存
        {
            let mut manager = SessionManager::new();
            manager.add_session(SessionConfig {
                name: "Test".to_string(),
                host: "host".to_string(),
                port: 22,
                username: "user".to_string(),
                password: "pass".to_string(),
            });
            manager.save(&sessions_file).unwrap();
        }
        
        // 加载并验证
        {
            let manager = SessionManager::load(&sessions_file).unwrap();
            assert_eq!(manager.count(), 1);
        }
    }
}
```

#### 2.3.3 ConnectionState 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_equality() {
        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_ne!(ConnectionState::Connected, ConnectionState::Disconnected);
    }

    #[test]
    fn test_connection_state_error() {
        let error1 = ConnectionState::Error("test".to_string());
        let error2 = ConnectionState::Error("test".to_string());
        let error3 = ConnectionState::Error("other".to_string());
        
        assert_eq!(error1, error2);
        assert_ne!(error1, error3);
    }
}
```

### 2.4 终端层测试

#### 2.4.1 Terminal 测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_new() {
        let terminal = Terminal::new();
        assert!(terminal.get_formatted_output().is_empty());
    }

    #[test]
    fn test_terminal_plain_output() {
        let mut terminal = Terminal::new();
        terminal.feed(b"Hello, World!");
        
        let output = terminal.get_formatted_output();
        assert!(output.contains("Hello, World!"));
    }

    #[test]
    fn test_terminal_ansi_color() {
        let mut terminal = Terminal::new();
        
        // ANSI 颜色代码：红色
        terminal.feed(b"\x1b[31mRed Text\x1b[0m");
        
        let output = terminal.get_formatted_output();
        assert!(output.contains("Red Text"));
    }

    #[test]
    fn test_terminal_newline() {
        let mut terminal = Terminal::new();
        terminal.feed(b"Line 1\nLine 2\n");
        
        let output = terminal.get_formatted_output();
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
    }

    #[test]
    fn test_terminal_buffer_limit() {
        let mut terminal = Terminal::new();
        
        // 填充大量数据
        for _ in 0..1000 {
            terminal.feed(b"Line of text\n");
        }
        
        // 验证缓冲区有上限
        let output = terminal.get_formatted_output();
        assert!(output.len() < 100000);  // 合理上限
    }
}
```

### 2.5 运行单元测试

```bash
# 运行所有测试
cargo test

# 运行特定模块测试
cargo test -- ssh
cargo test -- core
cargo test -- terminal

# 显示测试输出
cargo test -- --nocapture

# 运行特定测试
cargo test test_session_config_serialization

# 测试覆盖率
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

---

## 3. 集成测试

### 3.1 测试结构

```
tests/
├── integration/
│   ├── mod.rs
│   ├── ssh_connection.rs    # SSH 连接测试
│   ├── session_management.rs # 会话管理测试
│   └── terminal_output.rs   # 终端输出测试
└── fixtures/
    └── test_server.sh       # 测试服务器脚本
```

### 3.2 SSH 连接集成测试

```rust
// tests/integration/ssh_connection.rs

#[cfg(test)]
mod tests {
    use mistterm::ssh::*;

    #[test]
    fn test_full_connection_flow() {
        // 创建配置（使用测试服务器）
        let config = SshConfig {
            host: get_test_server_host(),
            port: 22,
            username: get_test_user(),
            password: get_test_password(),
        };
        
        // 创建客户端
        let mut client = SshClient::new(config);
        
        // 连接
        let result = client.connect();
        assert!(result.is_ok(), "连接应该成功");
        
        // 打开 Shell
        let channel_result = client.open_shell();
        assert!(channel_result.is_ok(), "Shell 应该打开成功");
        
        // 发送命令
        let cmd = b"echo 'Hello from test'\n";
        let sent = client.send(cmd).unwrap();
        assert_eq!(sent, cmd.len());
        
        // 断开连接
        client.disconnect();
    }

    #[test]
    fn test_connection_timeout() {
        let config = SshConfig {
            host: "10.255.255.1".to_string(),  // 不可达地址
            port: 22,
            username: "test".to_string(),
            password: "test".to_string(),
        };
        
        let mut client = SshClient::new(config);
        
        // 应该超时
        let result = client.connect();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timeout") || 
                result.unwrap_err().contains("failed"));
    }
}
```

### 3.3 会话管理集成测试

```rust
// tests/integration/session_management.rs

#[cfg(test)]
mod tests {
    use mistterm::core::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_session_lifecycle() {
        let temp_dir = TempDir::new().unwrap();
        let sessions_file = temp_dir.path().join("sessions.json");
        
        // 创建多个会话
        let mut manager = SessionManager::new();
        
        let config1 = SessionConfig {
            name: "Server 1".to_string(),
            host: "192.168.1.1".to_string(),
            port: 22,
            username: "user1".to_string(),
            password: "pass1".to_string(),
        };
        
        let config2 = SessionConfig {
            name: "Server 2".to_string(),
            host: "192.168.1.2".to_string(),
            port: 2222,
            username: "user2".to_string(),
            password: "pass2".to_string(),
        };
        
        manager.add_session(config1);
        manager.add_session(config2);
        
        assert_eq!(manager.count(), 2);
        
        // 保存
        manager.save(&sessions_file).unwrap();
        
        // 验证文件存在
        assert!(sessions_file.exists());
        
        // 重新加载
        let loaded = SessionManager::load(&sessions_file).unwrap();
        assert_eq!(loaded.count(), 2);
    }
}
```

### 3.4 运行集成测试

```bash
# 运行所有集成测试
cargo test --test '*'

# 运行特定集成测试
cargo test --test ssh_connection

# 使用测试服务器
export TEST_SERVER_HOST=localhost
export TEST_SERVER_USER=testuser
export TEST_SERVER_PASS=testpass
cargo test --test integration
```

---

## 4. 端到端测试

### 4.1 测试场景

| 场景 | 步骤 | 预期结果 |
|-----|------|---------|
| 新连接 | 1. 点击 Connect<br>2. 填写信息<br>3. 点击 Connect | 连接成功，显示终端 |
| 保存会话 | 1. 连接服务器<br>2. 会话自动保存 | 会话列表显示新会话 |
| 加载会话 | 1. 选择已保存会话<br>2. 点击 Connect | 重新连接成功 |
| 命令执行 | 1. 输入命令<br>2. 按 Enter | 显示命令输出 |
| 断开连接 | 1. 点击 X 按钮 | 会话被删除 |

### 4.2 手动测试清单

```markdown
## 功能测试

- [ ] **连接功能**
  - [ ] 新连接成功
  - [ ] 已保存会话连接成功
  - [ ] 错误连接显示错误信息
  - [ ] 连接超时处理

- [ ] **会话管理**
  - [ ] 会话自动保存
  - [ ] 会话列表显示正确
  - [ ] 删除会话正常
  - [ ] 会话配置正确加载

- [ ] **终端功能**
  - [ ] 命令输入正常
  - [ ] 命令输出显示
  - [ ] ANSI 颜色正确
  - [ ] 滚动正常

- [ ] **UI 交互**
  - [ ] 窗口缩放正常
  - [ ] 对话框打开/关闭
  - [ ] 焦点管理正常
```

---

## 5. 性能测试

### 5.1 性能指标

| 指标 | 目标 | 测量方法 |
|-----|------|---------|
| 启动时间 | < 2 秒 | 计时启动过程 |
| 连接时间 | < 5 秒 | 计时连接过程 |
| 命令响应 | < 100ms | 计时输入到输出 |
| 内存占用 | < 100MB | 运行时监控 |
| CPU 占用 | < 5% (空闲) | 运行时监控 |

### 5.2 性能测试脚本

```rust
// tests/performance/mod.rs

#[cfg(test)]
mod tests {
    use std::time::Instant;

    #[test]
    fn test_startup_time() {
        let start = Instant::now();
        
        // 模拟启动过程
        let _app = crate::MistTermApp::default();
        
        let duration = start.elapsed();
        assert!(duration.as_secs() < 2, "启动时间应该小于 2 秒");
    }

    #[test]
    fn test_command_response_time() {
        let mut terminal = Terminal::new();
        
        let start = Instant::now();
        
        // 模拟命令输出
        for _ in 0..100 {
            terminal.feed(b"ls -la\n");
        }
        
        let duration = start.elapsed();
        assert!(duration.as_millis() < 1000, "100 次命令响应应该小于 1 秒");
    }
}
```

### 5.3 火焰图分析

```bash
# 安装 flamegraph
cargo install flamegraph

# 生成火焰图
cargo flamegraph --bin mistterm -- --test

# 查看结果
open flamegraph.svg
```

---

## 6. 安全测试

### 6.1 安全审计

```bash
# 安装 cargo-audit
cargo install cargo-audit

# 运行审计
cargo audit

# 检查特定漏洞
cargo audit --id CVE-2023-xxxx
```

### 6.2 安全测试项

| 测试项 | 方法 | 预期 |
|-------|------|-----|
| 密码存储 | 检查 sessions.json | 待加密 |
| SSH 协议 | 检查协议版本 | SSH-2 |
| 主机验证 | 检查主机密钥 | 待实现 |
| 输入验证 | 测试特殊字符 | 正常处理 |
| 内存安全 | 运行 Miri | 无错误 |

### 6.3 使用 Miri 检测未定义行为

```bash
# 安装 Miri
rustup component add miri

# 初始化
cargo miri setup

# 运行测试
cargo miri test
```

---

## 7. 测试覆盖率

### 7.1 覆盖率目标

| 模块 | 目标覆盖率 | 当前覆盖率 |
|-----|----------|----------|
| ssh::client | 80% | TBD |
| ssh::manager | 70% | TBD |
| core::session | 85% | TBD |
| core::connection | 75% | TBD |
| terminal::emulator | 60% | TBD |
| ui::app | 50% | TBD |

### 7.2 生成覆盖率报告

```bash
# 使用 tarpaulin
cargo install cargo-tarpaulin
cargo tarpaulin --out Html --out Lcov

# 查看报告
open tarpaulin-report.html

# 查看覆盖率
cat lcov.info
```

### 7.3 覆盖率不足的处理

```rust
// 对于难以测试的代码，使用覆盖属性
#[cfg(test)]
#[allow(dead_code)]
mod tests {
    // 测试代码
}

// 或者使用条件编译
#[cfg(not(test))]
pub fn hard_to_test_function() {
    // 实际实现
}

#[cfg(test)]
pub fn hard_to_test_function() {
    // 测试桩
}
```

---

## 📚 相关文档

- [架构文档](./ARCHITECTURE.md)
- [部署指南](./DEPLOYMENT.md)
- [API 文档](./API.md)
