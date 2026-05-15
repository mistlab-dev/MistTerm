//! SSH 客户端 - 单个 SSH 连接的管理
#![allow(dead_code)]

use ssh2::Session;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use super::format_ssh_connect_error;

/// SSH 配置
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub private_key_path: String,
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

        let mut addrs = (&self.config.host as &str, self.config.port)
            .to_socket_addrs()
            .map_err(|e| {
                format_ssh_connect_error(&format!("无法解析主机地址: {}", e))
            })?;
        let sock = addrs.next().ok_or_else(|| {
            format_ssh_connect_error("无可解析的地址（请检查主机名与端口）")
        })?;
        let stream = TcpStream::connect_timeout(&sock, Duration::from_secs(30)).map_err(|e| {
            format_ssh_connect_error(&format!("TCP 连接失败（30 秒内未建立）: {}", e))
        })?;
        log::info!("TCP connected");

        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

        // 创建 SSH 会话
        let mut session = Session::new()
            .map_err(|e| format!("Failed to create SSH session: {}", e))?;
        log::info!("SSH session created");

        session.set_tcp_stream(stream);

        // SSH 握手
        session.handshake().map_err(|e| {
            format_ssh_connect_error(&format!("SSH handshake failed: {}", e))
        })?;
        log::info!("SSH handshake completed");

        // 认证策略：优先使用用户指定的私钥，然后尝试密码，最后尝试默认系统密钥
        let mut authenticated = false;

        // 1. 优先尝试用户指定的私钥路径
        if !self.config.private_key_path.is_empty() {
            let p = std::path::Path::new(&self.config.private_key_path);
            if p.is_file() {
                match session.userauth_pubkey_file(&self.config.username, None, p, None) {
                    Ok(_) => {
                        log::info!("Authenticated with user-specified SSH key: {}", self.config.private_key_path);
                        authenticated = true;
                    }
                    Err(e) => {
                        log::warn!("User-specified SSH key auth failed: {}", e);
                    }
                }
            } else {
                log::warn!("User-specified SSH key not found: {}", self.config.private_key_path);
            }
        }

        // 2. 尝试密码认证
        if !authenticated && !self.config.password.is_empty() {
            match session.userauth_password(&self.config.username, &self.config.password) {
                Ok(_) => {
                    log::info!("Authenticated with password");
                    authenticated = true;
                }
                Err(e) => {
                    log::info!("Password auth failed: {}", e);
                }
            }
        }

        // 3. 尝试默认系统密钥
        if !authenticated {
            log::info!("Trying default SSH keys under ~/.ssh ...");
            if let Some(home) = dirs::home_dir() {
                let ssh_dir = home.join(".ssh");
                for key_name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
                    let p = ssh_dir.join(key_name);
                    if p.is_file()
                        && session
                            .userauth_pubkey_file(&self.config.username, None, &p, None)
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
            return Err(format_ssh_connect_error(
                "Authentication failed (password and SSH keys failed)",
            ));
        }

        // 认证完成后再切到非阻塞，供 shell 读写线程轮询
        session.set_blocking(false);

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
        };
        
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_client_creation() {
        let config = SshConfig {
            host: "localhost".to_string(),
            port: 22,
            username: "test".to_string(),
            password: "pass".to_string(),
            private_key_path: String::new(),
        };
        
        let client = SshClient::new(config);
        assert!(!client.is_connected());
    }
}
