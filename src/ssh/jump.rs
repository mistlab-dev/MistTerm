//! ProxyJump / 多跳 SSH：经跳板 `direct-tcpip` 隧道连接目标。

use ssh2::Session;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::client::{authenticate_session, apply_keepalive, SshConfig};
use super::known_hosts;
use super::proxy_command;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// 单跳跳板连接参数（由 UI 从已保存会话或 `user@host:port` 解析）。
#[derive(Debug, Clone)]
pub struct JumpHop {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub private_key_path: String,
    pub use_ssh_agent: bool,
}

/// 解析后的跳板链端点（尚未解析凭据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpEndpoint {
    pub host: String,
    pub port: u16,
    pub username: String,
}

/// 将 `ProxyJump` 字符串拆成多跳 token（OpenSSH 逗号分隔）。
pub fn parse_jump_chain(proxy_jump: &str) -> Vec<String> {
    proxy_jump
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// 解析单跳描述：`host` | `user@host` | `user@host:port`。
pub fn parse_jump_endpoint(token: &str, default_username: &str) -> Result<JumpEndpoint, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("Empty ProxyJump hop".into());
    }

    let (user_host, port) = if let Some((left, port_str)) = token.rsplit_once(':') {
        if !port_str.is_empty() && port_str.chars().all(|c| c.is_ascii_digit()) {
            let port: u16 = port_str
                .parse()
                .map_err(|_| format!("Invalid port in ProxyJump hop: {}", token))?;
            (left, port)
        } else {
            (token, 22u16)
        }
    } else {
        (token, 22)
    };

    let (username, host) = if let Some((u, h)) = user_host.split_once('@') {
        if u.is_empty() || h.is_empty() {
            return Err(format!("Invalid ProxyJump hop: {}", token));
        }
        (u.to_string(), h.to_string())
    } else if user_host.is_empty() {
        return Err(format!("Invalid ProxyJump hop: {}", token));
    } else {
        (
            default_username.to_string(),
            user_host.to_string(),
        )
    };

    Ok(JumpEndpoint {
        host,
        port,
        username,
    })
}

/// 建立 SSH 会话（含 ProxyJump 链）；`config.jump_hops` 与 `proxy_jump` 一一对应且已解析凭据。
pub fn connect_ssh_session(config: &SshConfig) -> Result<Session, String> {
    if !config.proxy_command.trim().is_empty() && !parse_jump_chain(&config.proxy_jump).is_empty()
    {
        return Err("ProxyCommand and ProxyJump cannot be used together".into());
    }

    if !config.proxy_command.trim().is_empty() {
        let cmd = proxy_command::expand_proxy_command(
            config.proxy_command.trim(),
            &config.host,
            config.port,
            &config.username,
        );
        let stream = proxy_command::tcp_stream_via_proxy_command(&cmd)?;
        return handshake_on_tcp_stream(stream, config);
    }

    let chain = parse_jump_chain(&config.proxy_jump);
    if chain.is_empty() {
        return connect_direct_tcp(config);
    }

    if chain.len() != config.jump_hops.len() {
        return Err(format!(
            "ProxyJump hop count ({}) does not match resolved credentials ({})",
            chain.len(),
            config.jump_hops.len()
        ));
    }

    let mut session = connect_direct_tcp_session(&config.jump_hops[0])?;

    for i in 0..chain.len() {
        let (next_host, next_port, next_cfg) = if i + 1 < config.jump_hops.len() {
            let hop = &config.jump_hops[i + 1];
            (
                hop.host.clone(),
                hop.port,
                hop_to_config(hop),
            )
        } else {
            (
                config.host.clone(),
                config.port,
                SshConfig {
                    host: config.host.clone(),
                    port: config.port,
                    username: config.username.clone(),
                    password: config.password.clone(),
                    private_key_path: config.private_key_path.clone(),
                    use_ssh_agent: config.use_ssh_agent,
                    keepalive_interval_secs: config.keepalive_interval_secs,
                    keepalive_count_max: config.keepalive_count_max,
                    proxy_jump: String::new(),
                    proxy_command: String::new(),
                    jump_hops: Vec::new(),
                    local_forwards: config.local_forwards.clone(),
                    remote_forwards: config.remote_forwards.clone(),
                    dynamic_forwards: config.dynamic_forwards.clone(),
                },
            )
        };

        log::info!(
            "ProxyJump hop {}/{}: tunnel {}:{}",
            i + 1,
            chain.len(),
            next_host,
            next_port
        );

        session.set_blocking(true);
        let channel = session
            .channel_direct_tcpip(&next_host, next_port, None)
            .map_err(|e| format!("direct-tcpip to {}:{} failed: {}", next_host, next_port, e))?;
        session.set_blocking(false);

        session = handshake_on_channel(channel, &next_cfg)?;
    }

    Ok(session)
}

fn connect_direct_tcp(config: &SshConfig) -> Result<Session, String> {
    let stream = tcp_connect(&config.host, config.port)?;
    handshake_on_tcp_stream(stream, config)
}

fn connect_direct_tcp_session(hop: &JumpHop) -> Result<Session, String> {
    let stream = tcp_connect(&hop.host, hop.port)?;
    let cfg = hop_to_config(hop);
    handshake_on_tcp_stream(stream, &cfg)
}

fn hop_to_config(hop: &JumpHop) -> SshConfig {
    SshConfig {
        host: hop.host.clone(),
        port: hop.port,
        username: hop.username.clone(),
        password: hop.password.clone(),
        private_key_path: hop.private_key_path.clone(),
        use_ssh_agent: hop.use_ssh_agent,
        ..SshConfig::default()
    }
}

fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, String> {
    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("Failed to resolve {}:{} — {}", host, port, e))?;
    let sock = addrs
        .next()
        .ok_or_else(|| format!("No resolvable address for {}:{}", host, port))?;
    TcpStream::connect_timeout(&sock, CONNECT_TIMEOUT)
        .map_err(|e| format!("TCP connect failed (30s timeout) to {}:{} — {}", host, port, e))
}

fn handshake_on_tcp_stream(stream: TcpStream, config: &SshConfig) -> Result<Session, String> {
    stream
        .set_read_timeout(Some(CONNECT_TIMEOUT))
        .ok();
    let mut session = Session::new().map_err(|e| format!("Failed to create SSH session: {}", e))?;
    session.set_tcp_stream(stream);
    finish_handshake(session, config)
}

/// 将 `direct-tcpip` 通道桥接到本机 `TcpStream`，供 libssh2 在 Windows 上握手（`Stream` 无 `AsRawSocket`）。
fn handshake_on_channel(channel: ssh2::Channel, config: &SshConfig) -> Result<Session, String> {
    let stream = channel_as_local_tcp(channel)?;
    handshake_on_tcp_stream(stream, config)
}

fn channel_as_local_tcp(channel: ssh2::Channel) -> Result<TcpStream, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("ProxyJump local bridge bind failed: {}", e))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("ProxyJump local bridge: {}", e))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("ProxyJump local bridge: {}", e))?
        .port();

    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    thread::spawn(move || {
        let bridge_result = (|| -> Result<(), String> {
            let (tcp, _) = listener
                .accept()
                .map_err(|e| format!("ProxyJump bridge accept: {}", e))?;
            ready_tx
                .send(Ok(()))
                .map_err(|_| "ProxyJump bridge: caller dropped".to_string())?;
            run_channel_tcp_bridge(tcp, channel)
        })();
        if let Err(e) = bridge_result {
            let _ = ready_tx.send(Err(e));
        }
    });

    let stream = tcp_connect("127.0.0.1", local_port)?;
    match ready_rx.recv_timeout(CONNECT_TIMEOUT) {
        Ok(Ok(())) => Ok(stream),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("ProxyJump local bridge timed out".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("ProxyJump local bridge thread exited early".into())
        }
    }
}

fn run_channel_tcp_bridge(mut tcp: TcpStream, channel: ssh2::Channel) -> Result<(), String> {
    let mut tcp_peer = tcp
        .try_clone()
        .map_err(|e| format!("ProxyJump bridge tcp clone: {}", e))?;
    let channel = Arc::new(Mutex::new(channel));

    let ch_to_tcp = Arc::clone(&channel);
    let t1 = thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = match ch_to_tcp.lock() {
                Ok(mut ch) => match ch.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                },
                Err(_) => break,
            };
            if tcp.write_all(&buf[..n]).is_err() {
                break;
            }
        }
    });

    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = match tcp_peer.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        match channel.lock() {
            Ok(mut ch) => {
                if ch.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = t1.join();
    Ok(())
}

fn finish_handshake(mut session: Session, config: &SshConfig) -> Result<Session, String> {
    session.set_timeout(CONNECT_TIMEOUT.as_millis() as u32);
    session.set_blocking(true);
    session
        .handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;
    known_hosts::verify_or_record_host_key(&session, &config.host, config.port)?;
    apply_keepalive(&mut session, config);
    authenticate_session(&mut session, config)?;
    session.set_blocking(false);
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_chain() {
        assert!(parse_jump_chain("").is_empty());
        assert!(parse_jump_chain("  , , ").is_empty());
    }

    #[test]
    fn parse_multi_hop_chain() {
        assert_eq!(
            parse_jump_chain("bastion, inner"),
            vec!["bastion".to_string(), "inner".to_string()]
        );
    }

    #[test]
    fn parse_endpoint_user_host_port() {
        let e = parse_jump_endpoint("admin@bastion.example:2222", "root").unwrap();
        assert_eq!(e.username, "admin");
        assert_eq!(e.host, "bastion.example");
        assert_eq!(e.port, 2222);
    }

    #[test]
    fn parse_endpoint_host_only() {
        let e = parse_jump_endpoint("bastion", "deploy").unwrap();
        assert_eq!(e.username, "deploy");
        assert_eq!(e.host, "bastion");
        assert_eq!(e.port, 22);
    }

    #[test]
    fn parse_endpoint_user_host() {
        let e = parse_jump_endpoint("ops@jump", "root").unwrap();
        assert_eq!(e.username, "ops");
        assert_eq!(e.host, "jump");
        assert_eq!(e.port, 22);
    }

    #[test]
    fn hop_to_config_propagates_use_ssh_agent() {
        let hop = JumpHop {
            host: "hop".into(),
            port: 22,
            username: "u".into(),
            password: String::new(),
            private_key_path: String::new(),
            use_ssh_agent: false,
        };
        let cfg = hop_to_config(&hop);
        assert!(!cfg.use_ssh_agent);
        assert_eq!(cfg.host, "hop");
    }
}
