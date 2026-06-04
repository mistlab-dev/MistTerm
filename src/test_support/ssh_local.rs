//! 本地 OpenSSH 探测与连接（集成测试用，无 sshd 时自动 skip）。

use std::io::Read;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use ssh2::Session;

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 22;
const DEFAULT_USER: &str = "root";
const DEFAULT_PASSWORD: &str = "mistterm123";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

pub fn ssh_host() -> String {
    env_or("MISTTERM_TEST_SSH_HOST", DEFAULT_HOST)
}

pub fn ssh_port() -> u16 {
    std::env::var("MISTTERM_TEST_SSH_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn ssh_user() -> String {
    env_or("MISTTERM_TEST_SSH_USER", DEFAULT_USER)
}

pub fn ssh_password() -> String {
    env_or("MISTTERM_TEST_SSH_PASSWORD", DEFAULT_PASSWORD)
}

/// TCP 可达且能完成 SSH 握手即视为可用。
pub fn local_sshd_available() -> bool {
    let addr: SocketAddr = match format!("{}:{}", ssh_host(), ssh_port()).parse() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let Ok(stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return false;
    };
    let mut sess = match Session::new() {
        Ok(s) => s,
        Err(_) => return false,
    };
    sess.set_tcp_stream(stream);
    sess.handshake().is_ok()
}

/// 连接并密码认证；失败返回 `None`（调用方应 skip 测试）。
pub fn connect_local_sshd() -> Option<Session> {
    if !local_sshd_available() {
        return None;
    }
    let addr: SocketAddr = format!("{}:{}", ssh_host(), ssh_port())
        .parse()
        .ok()?;
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    let mut sess = Session::new().ok()?;
    sess.set_tcp_stream(stream);
    sess.handshake().ok()?;
    let user = ssh_user();
    let pass = ssh_password();
    if sess.userauth_password(&user, &pass).is_err()
        && sess.userauth_agent(&user).is_err()
    {
        return None;
    }
    Some(sess)
}

pub fn skip_without_sshd() -> Option<Session> {
    connect_local_sshd().or_else(|| {
        eprintln!(
            "skip: local sshd unavailable at {}:{} (set MISTTERM_TEST_SSH_* or start sshd)",
            ssh_host(),
            ssh_port()
        );
        None
    })
}

pub fn exec_remote(session: &Session, cmd: &str) -> Option<String> {
    let mut channel = session.channel_session().ok()?;
    channel.exec(cmd).ok()?;
    let mut out = String::new();
    channel.read_to_string(&mut out).ok()?;
    channel.wait_close().ok();
    Some(out)
}

pub fn open_sftp(session: &Session) -> Option<ssh2::Sftp> {
    session.sftp().ok()
}
