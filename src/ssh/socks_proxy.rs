//! 本地 SOCKS5 代理（SSH `-D`）：CONNECT 经 `direct-tcpip` 走 SSH 隧道。

use ssh2::Session;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use super::port_forward::{self, bridge_tcp_channel, ForwardControl};

const SOCKS_VER: u8 = 0x05;
const CMD_CONNECT: u8 = 0x01;
const ATYP_IPV4: u8 = 0x01;
const ATYP_DOMAIN: u8 = 0x03;
const ATYP_IPV6: u8 = 0x04;
const REP_SUCCESS: u8 = 0x00;
const REP_CMD_UNSUPPORTED: u8 = 0x07;
const REP_HOST_UNREACHABLE: u8 = 0x04;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DynamicPortForward {
    pub local_port: u16,
    #[serde(default = "default_bind")]
    pub bind_address: String,
}

fn default_bind() -> String {
    "127.0.0.1".into()
}

pub fn spawn_dynamic_forwards(session: &Session, forwards: &[DynamicPortForward]) {
    for fwd in forwards {
        if fwd.local_port == 0 {
            continue;
        }
        let _ = spawn_dynamic_forward_controllable(session.clone(), fwd.clone());
    }
}

pub fn spawn_dynamic_forward_controllable(
    session: Session,
    fwd: DynamicPortForward,
) -> Result<ForwardControl, String> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let control = ForwardControl {
        shutdown: shutdown.clone(),
    };
    thread::spawn(move || {
        if let Err(e) = run_dynamic_forward(session, &fwd, shutdown) {
            log::warn!(
                "dynamic forward {}:{} stopped: {}",
                fwd.bind_address,
                fwd.local_port,
                e
            );
        }
    });
    Ok(control)
}

fn run_dynamic_forward(
    session: Session,
    fwd: &DynamicPortForward,
    shutdown: Arc<AtomicBool>,
) -> Result<(), String> {
    let bind = if fwd.bind_address.is_empty() {
        "127.0.0.1"
    } else {
        &fwd.bind_address
    };
    let addr = format!("{bind}:{}", fwd.local_port);
    let listener = TcpListener::bind(&addr).map_err(|e| format!("bind {addr}: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("set_nonblocking {addr}: {e}"))?;
    log::info!("SOCKS5 dynamic forward listening on {addr}");
    let session = Arc::new(Mutex::new(session));
    port_forward::accept_loop(&listener, &shutdown, |client| {
        let session = Arc::clone(&session);
        thread::spawn(move || {
            if let Err(e) = serve_socks5_client(client, session) {
                log::debug!("SOCKS client ended: {e}");
            }
        });
    })
}

fn serve_socks5_client(mut client: TcpStream, session: Arc<Mutex<Session>>) -> Result<(), String> {
    client
        .set_read_timeout(Some(std::time::Duration::from_secs(60)))
        .ok();
    client
        .set_write_timeout(Some(std::time::Duration::from_secs(60)))
        .ok();

    let mut buf = [0u8; 512];
    read_exact(&mut client, &mut buf[..2])?;
    if buf[0] != SOCKS_VER {
        return Err("invalid SOCKS version".into());
    }
    let nmethods = buf[1] as usize;
    if nmethods > buf.len() {
        return Err("too many auth methods".into());
    }
    read_exact(&mut client, &mut buf[..nmethods])?;
    client
        .write_all(&[SOCKS_VER, 0x00])
        .map_err(|e| format!("SOCKS auth reply: {e}"))?;

    read_exact(&mut client, &mut buf[..4])?;
    if buf[0] != SOCKS_VER {
        return Err("invalid SOCKS request version".into());
    }
    let cmd = buf[1];
    let atyp = buf[3];
    if cmd != CMD_CONNECT {
        socks_reply(&mut client, REP_CMD_UNSUPPORTED)?;
        return Err("only CONNECT supported".into());
    }

    let (host, port) = match read_socks_addr(&mut client, atyp)? {
        Some(t) => t,
        None => {
            socks_reply(&mut client, REP_CMD_UNSUPPORTED)?;
            return Err("unsupported address type".into());
        }
    };

    let channel = {
        let sess = session
            .lock()
            .map_err(|_| "SSH session lock poisoned".to_string())?;
        sess.set_blocking(true);
        sess.channel_direct_tcpip(&host, port, None)
            .map_err(|e| format!("direct-tcpip {host}:{port}: {e}"))
    };

    match channel {
        Ok(ch) => {
            socks_reply(&mut client, REP_SUCCESS)?;
            bridge_tcp_channel(client, ch)?;
            Ok(())
        }
        Err(e) => {
            socks_reply(&mut client, REP_HOST_UNREACHABLE)?;
            Err(e)
        }
    }
}

fn read_socks_addr(client: &mut TcpStream, atyp: u8) -> Result<Option<(String, u16)>, String> {
    let mut buf = [0u8; 260];
    match atyp {
        ATYP_IPV4 => {
            read_exact(client, &mut buf[..6])?;
            let host = format!(
                "{}.{}.{}.{}",
                buf[0], buf[1], buf[2], buf[3]
            );
            let port = u16::from_be_bytes([buf[4], buf[5]]);
            Ok(Some((host, port)))
        }
        ATYP_DOMAIN => {
            read_exact(client, &mut buf[..1])?;
            let len = buf[0] as usize;
            if len == 0 || len + 2 > buf.len() {
                return Err("invalid domain length".into());
            }
            read_exact(client, &mut buf[..len + 2])?;
            let host = String::from_utf8(buf[..len].to_vec())
                .map_err(|_| "invalid domain encoding".to_string())?;
            let port = u16::from_be_bytes([buf[len], buf[len + 1]]);
            Ok(Some((host, port)))
        }
        ATYP_IPV6 => {
            read_exact(client, &mut buf[..18])?;
            let host = format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                u16::from_be_bytes([buf[0], buf[1]]),
                u16::from_be_bytes([buf[2], buf[3]]),
                u16::from_be_bytes([buf[4], buf[5]]),
                u16::from_be_bytes([buf[6], buf[7]]),
                u16::from_be_bytes([buf[8], buf[9]]),
                u16::from_be_bytes([buf[10], buf[11]]),
                u16::from_be_bytes([buf[12], buf[13]]),
                u16::from_be_bytes([buf[14], buf[15]]),
            );
            let port = u16::from_be_bytes([buf[16], buf[17]]);
            Ok(Some((host, port)))
        }
        _ => Ok(None),
    }
}

fn socks_reply(client: &mut TcpStream, rep: u8) -> Result<(), String> {
    let reply = [
        SOCKS_VER,
        rep,
        0x00,
        ATYP_IPV4,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    client
        .write_all(&reply)
        .map_err(|e| format!("SOCKS reply: {e}"))
}

fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<(), String> {
    let mut off = 0;
    while off < buf.len() {
        let n = stream
            .read(&mut buf[off..])
            .map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("unexpected EOF".into());
        }
        off += n;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_forward_default_bind() {
        let f = DynamicPortForward {
            local_port: 1080,
            bind_address: default_bind(),
        };
        assert_eq!(f.bind_address, "127.0.0.1");
    }
}
