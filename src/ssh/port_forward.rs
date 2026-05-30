//! SSH 端口转发：本地 `-L`、远程 `-R`。

use ssh2::Session;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocalPortForward {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(default = "default_bind")]
    pub bind_address: String,
}

fn default_bind() -> String {
    "127.0.0.1".into()
}

/// 远程端口转发（SSH `-R`）：远端监听，连入流量转发到本机 `target_host:target_port`。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemotePortForward {
    pub remote_port: u16,
    pub target_host: String,
    pub target_port: u16,
    /// 远端绑定地址（`None` 表示由服务端默认，通常为 loopback）
    #[serde(default)]
    pub remote_bind_address: Option<String>,
}

/// 在克隆的 `Session` 上启动本地转发（每规则一线程）。
pub fn spawn_local_forwards(session: &Session, forwards: &[LocalPortForward]) {
    for fwd in forwards {
        if fwd.local_port == 0 || fwd.remote_host.is_empty() {
            continue;
        }
        let sess = session.clone();
        let fwd = fwd.clone();
        thread::spawn(move || {
            if let Err(e) = run_local_forward(sess, &fwd) {
                log::warn!(
                    "local forward {}:{} -> {}:{} stopped: {}",
                    fwd.bind_address,
                    fwd.local_port,
                    fwd.remote_host,
                    fwd.remote_port,
                    e
                );
            }
        });
    }
}

fn run_local_forward(session: Session, fwd: &LocalPortForward) -> Result<(), String> {
    let bind = if fwd.bind_address.is_empty() {
        "127.0.0.1"
    } else {
        &fwd.bind_address
    };
    let addr = format!("{bind}:{}", fwd.local_port);
    let listener =
        TcpListener::bind(&addr).map_err(|e| format!("bind {addr}: {}", e))?;
    log::info!(
        "local forward listening {} -> {}:{}",
        addr,
        fwd.remote_host,
        fwd.remote_port
    );
    let session = Arc::new(Mutex::new(session));
    loop {
        let (tcp, _) = listener
            .accept()
            .map_err(|e| format!("local accept: {}", e))?;
        let session = Arc::clone(&session);
        let host = fwd.remote_host.clone();
        let port = fwd.remote_port;
        thread::spawn(move || {
            let channel = match session.lock() {
                Ok(sess) => {
                    sess.set_blocking(true);
                    sess.channel_direct_tcpip(&host, port, None)
                }
                Err(_) => return,
            };
            match channel {
                Ok(ch) => {
                    let _ = bridge_tcp_channel(tcp, ch);
                }
                Err(e) => log::warn!("direct-tcpip {}:{} failed: {}", host, port, e),
            }
        });
    }
}

/// 在克隆的 `Session` 上启动远程转发（每规则一线程）。
pub fn spawn_remote_forwards(session: &Session, forwards: &[RemotePortForward]) {
    for fwd in forwards {
        if fwd.remote_port == 0 || fwd.target_host.is_empty() {
            continue;
        }
        let sess = session.clone();
        let fwd = fwd.clone();
        thread::spawn(move || {
            if let Err(e) = run_remote_forward(sess, &fwd) {
                log::warn!(
                    "remote forward :{} -> {}:{} stopped: {}",
                    fwd.remote_port,
                    fwd.target_host,
                    fwd.target_port,
                    e
                );
            }
        });
    }
}

fn run_remote_forward(session: Session, fwd: &RemotePortForward) -> Result<(), String> {
    session.set_blocking(true);
    let bind = fwd.remote_bind_address.as_deref();
    let (mut listener, bound_port) = session
        .channel_forward_listen(fwd.remote_port, bind, Some(32))
        .map_err(|e| format!("remote forward listen :{}: {}", fwd.remote_port, e))?;
    log::info!(
        "remote forward listening on remote port {} (bound {}) -> {}:{}",
        fwd.remote_port,
        bound_port,
        fwd.target_host,
        fwd.target_port
    );
    loop {
        let channel = listener
            .accept()
            .map_err(|e| format!("remote forward accept: {}", e))?;
        let host = fwd.target_host.clone();
        let port = fwd.target_port;
        thread::spawn(move || {
            let addr = format!("{host}:{port}");
            let tcp = match TcpStream::connect(&addr) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("remote forward connect to {addr}: {e}");
                    return;
                }
            };
            let _ = bridge_tcp_channel(tcp, channel);
        });
    }
}

pub(crate) fn bridge_tcp_channel(mut tcp: TcpStream, channel: ssh2::Channel) -> Result<(), String> {
    let mut tcp_peer = tcp
        .try_clone()
        .map_err(|e| format!("tcp clone: {}", e))?;
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
