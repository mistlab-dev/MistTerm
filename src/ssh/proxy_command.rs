//! OpenSSH `ProxyCommand`：子进程 stdio 桥接为 TCP 流。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub fn expand_proxy_command(template: &str, host: &str, port: u16, user: &str) -> String {
    template
        .replace("%h", host)
        .replace("%p", &port.to_string())
        .replace("%r", user)
        .replace("%u", user)
}

/// 启动 ProxyCommand 并在本机暴露为 `127.0.0.1:随机端口` 的 `TcpStream`。
pub fn tcp_stream_via_proxy_command(command: &str) -> Result<TcpStream, String> {
    let command = command.to_string();
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("ProxyCommand bridge bind: {}", e))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| format!("ProxyCommand bridge: {}", e))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("ProxyCommand bridge: {}", e))?
        .port();

    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);

    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut child = spawn_proxy_shell(&command)?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or("ProxyCommand: no stdin")?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or("ProxyCommand: no stdout")?;

            let (mut tcp, _) = listener
                .accept()
                .map_err(|e| format!("ProxyCommand bridge accept: {}", e))?;
            ready_tx
                .send(Ok(()))
                .map_err(|_| "ProxyCommand: caller dropped".to_string())?;

            let mut tcp_peer = tcp
                .try_clone()
                .map_err(|e| format!("ProxyCommand tcp clone: {}", e))?;

            let t1 = thread::spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match stdout.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tcp.write_all(&buf[..n]).is_err() {
                                break;
                            }
                        }
                    }
                }
            });

            let mut buf = [0u8; 16 * 1024];
            loop {
                match tcp_peer.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if stdin.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = t1.join();
            let _ = child.kill();
            Ok(())
        })();
        if let Err(e) = result {
            let _ = ready_tx.send(Err(e));
        }
    });

    match ready_rx.recv_timeout(CONNECT_TIMEOUT) {
        Ok(Ok(())) => TcpStream::connect_timeout(
            &format!("127.0.0.1:{local_port}")
                .parse()
                .map_err(|e| format!("ProxyCommand addr: {}", e))?,
            CONNECT_TIMEOUT,
        )
        .map_err(|e| format!("ProxyCommand local connect: {}", e)),
        Ok(Err(e)) => Err(e),
        Err(mpsc::RecvTimeoutError::Timeout) => Err("ProxyCommand timed out".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("ProxyCommand thread exited early".into())
        }
    }
}

fn spawn_proxy_shell(command: &str) -> Result<std::process::Child, String> {
    #[cfg(unix)]
    {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("ProxyCommand spawn: {}", e))
    }
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("ProxyCommand spawn: {}", e))
    }
}
