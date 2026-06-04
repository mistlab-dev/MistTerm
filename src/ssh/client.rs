//! SSH 客户端 - 单个 SSH 连接的管理
#![allow(dead_code)]

use ssh2::Session;
use std::io::Write;

use super::jump;

/// SSH 配置
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub private_key_path: String,
    /// 在指定密钥之后尝试 ssh-agent / Pageant。
    pub use_ssh_agent: bool,
    /// `ServerAliveInterval`（秒）；0 表示不启用
    pub keepalive_interval_secs: u32,
    pub keepalive_count_max: u8,
    /// OpenSSH `ProxyJump`（逗号分隔多跳）
    pub proxy_jump: String,
    /// OpenSSH `ProxyCommand`（经子进程 stdio 桥接 TCP）
    pub proxy_command: String,
    /// 与 `proxy_jump` 各跳对应的凭据（由 UI 从已保存会话解析）
    pub jump_hops: Vec<jump::JumpHop>,
    /// 本地端口转发（`-L`）
    pub local_forwards: Vec<super::port_forward::LocalPortForward>,
    /// 远程端口转发（`-R`）
    pub remote_forwards: Vec<super::port_forward::RemotePortForward>,
    /// 动态 SOCKS 转发（`-D`）
    pub dynamic_forwards: Vec<super::socks_proxy::DynamicPortForward>,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 22,
            username: String::new(),
            password: String::new(),
            private_key_path: String::new(),
            use_ssh_agent: true,
            keepalive_interval_secs: 0,
            keepalive_count_max: 3,
            proxy_jump: String::new(),
            proxy_command: String::new(),
            jump_hops: Vec::new(),
            local_forwards: Vec::new(),
            remote_forwards: Vec::new(),
            dynamic_forwards: Vec::new(),
        }
    }
}

/// 认证 SSH 会话（密码 / 指定密钥 / 默认 `~/.ssh` 密钥）。
pub fn authenticate_session(session: &mut Session, config: &SshConfig) -> Result<(), String> {
    let mut authenticated = false;

    if !config.private_key_path.is_empty() {
        let p = std::path::Path::new(&config.private_key_path);
        if p.is_file() {
            match session.userauth_pubkey_file(&config.username, None, p, None) {
                Ok(_) => {
                    log::info!(
                        "Authenticated with user-specified SSH key: {}",
                        config.private_key_path
                    );
                    authenticated = true;
                }
                Err(e) => {
                    log::warn!("User-specified SSH key auth failed: {}", e);
                }
            }
        } else {
            log::warn!(
                "User-specified SSH key not found: {}",
                config.private_key_path
            );
        }
    }

    if !authenticated && config.use_ssh_agent {
        match session.userauth_agent(&config.username) {
            Ok(_) => {
                log::info!("Authenticated with SSH agent");
                authenticated = true;
            }
            Err(e) => {
                log::debug!("SSH agent auth failed: {}", e);
            }
        }
    }

    if !authenticated && !config.password.is_empty() {
        match session.userauth_password(&config.username, &config.password) {
            Ok(_) => {
                log::info!("Authenticated with password");
                authenticated = true;
            }
            Err(e) => {
                log::info!("Password auth failed: {}", e);
            }
        }
    }

    if !authenticated {
        log::info!("Trying default SSH keys under ~/.ssh ...");
        if let Some(home) = dirs::home_dir() {
            let ssh_dir = home.join(".ssh");
            for key_name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
                let p = ssh_dir.join(key_name);
                if p.is_file()
                    && session
                        .userauth_pubkey_file(&config.username, None, &p, None)
                        .is_ok()
                {
                    log::info!("Authenticated with SSH key {}", p.display());
                    authenticated = true;
                    break;
                }
            }
        }
    }

    if !authenticated {
        return Err(
            "Authentication failed (SSH keys, agent, password, and default keys failed)".to_string(),
        );
    }
    Ok(())
}

pub fn apply_keepalive(session: &mut Session, config: &SshConfig) {
    if config.keepalive_interval_secs > 0 {
        let want = config.keepalive_interval_secs;
        let _ = session.set_keepalive(true, want);
        log::info!(
            "SSH keepalive enabled: interval={}s count_max={}",
            want,
            config.keepalive_count_max
        );
    }
}

/// SSH 客户端
pub struct SshClient {
    session: Option<Session>,
    config: SshConfig,
}

impl SshClient {
    /// 创建新的 SSH 客户端
    pub fn new(config: SshConfig) -> Self {
        Self {
            session: None,
            config,
        }
    }

    /// 连接到 SSH 服务器
    pub fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        log::info!("Connecting to {}...", addr);

        let session = jump::connect_ssh_session(&self.config)?;
        if !self.config.local_forwards.is_empty() {
            super::port_forward::spawn_local_forwards(&session, &self.config.local_forwards);
        }
        if !self.config.remote_forwards.is_empty() {
            super::port_forward::spawn_remote_forwards(&session, &self.config.remote_forwards);
        }
        if !self.config.dynamic_forwards.is_empty() {
            super::socks_proxy::spawn_dynamic_forwards(&session, &self.config.dynamic_forwards);
        }
        self.session = Some(session);
        log::info!("SSH connected successfully");
        Ok(())
    }

    /// 打开交互式 shell 通道（`cols`/`rows` 为字符网格，需与本地终端模拟器一致）
    pub fn open_shell(&mut self, cols: u32, rows: u32) -> Result<ssh2::Channel, String> {
        let session = self.session.as_mut()
            .ok_or("Not connected")?;

        // 打开 channel/shell 时使用阻塞模式，避免 Session(-37) Would block
        session.set_blocking(true);

        let mut channel = match session.channel_session() {
            Ok(channel) => channel,
            Err(e) => {
                session.set_blocking(false);
                return Err(format!("Failed to open channel: {}", e));
            }
        };

        let cols = cols.clamp(20, 512);
        let rows = rows.clamp(5, 256);
        let px_w = cols.saturating_mul(9);
        let px_h = rows.saturating_mul(16);

        // 请求 PTY（尺寸错误会导致远端按 80 列换行、vim 只开一行等）
        if let Err(e) = channel.request_pty(
            "xterm-256color",
            None,
            Some((cols, rows, px_w, px_h)),
        ) {
            session.set_blocking(false);
            return Err(format!("Failed to request PTY: {}", e));
        }

        // 启动 shell
        if let Err(e) = channel.shell() {
            session.set_blocking(false);
            return Err(format!("Failed to start shell: {}", e));
        }

        // shell 已建立，切回非阻塞给后台轮询线程
        session.set_blocking(false);

        log::info!("Shell channel opened");
        Ok(channel)
    }

    /// 发送数据到 SSH 通道
    pub fn send(&mut self, data: &[u8]) -> Result<usize, String> {
        let mut channel = self.session.as_mut()
            .ok_or("Not connected")?
            .channel_session()
            .map_err(|e| format!("Failed to get channel: {}", e))?;

        channel.write_all(data)
            .map_err(|e| format!("Write failed: {}", e))?;
        
        Ok(data.len())
    }

    /// 与 libssh2 非阻塞 I/O 配合：写卡住时可短暂切阻塞再切回。
    pub fn set_blocking(&mut self, blocking: bool) -> Result<(), String> {
        let s = self.session.as_mut().ok_or("Not connected")?;
        s.set_blocking(blocking);
        Ok(())
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.session = None;
        log::info!("SSH disconnected");
    }

    /// 非交互 `exec`（独立 channel，不占用已打开的 shell）。
    pub fn exec_command(&mut self, command: &str) -> Result<(String, i32), String> {
        use std::io::Read;
        let session = self.session.as_mut().ok_or("Not connected")?;
        session.set_blocking(true);
        let result = (|| {
            let mut channel = session
                .channel_session()
                .map_err(|e| format!("打开 exec 通道失败: {e}"))?;
            channel
                .exec(command)
                .map_err(|e| format!("exec 失败: {e}"))?;
            let mut output = Vec::new();
            channel
                .read_to_end(&mut output)
                .map_err(|e| format!("读取输出失败: {e}"))?;
            let code = channel.exit_status().unwrap_or(-1);
            let _ = channel.wait_close();
            let stdout = String::from_utf8_lossy(&output).into_owned();
            Ok((stdout, code))
        })();
        session.set_blocking(false);
        result
    }

    /// 获取 SSH 会话（用于文件传输等高级操作）
    pub fn get_session(&self) -> &Session {
        &self.session
            .as_ref()
            .expect("No active SSH session")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = SshConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "test".to_string(),
            password: "pass".to_string(),
            private_key_path: String::new(),
            use_ssh_agent: true,
            ..SshConfig::default()
        };
        
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 22);
        assert!(config.use_ssh_agent);
    }

    #[test]
    fn test_client_creation() {
        let config = SshConfig::default();
        
        let client = SshClient::new(config);
        assert!(!client.is_connected());
    }
}
