//! 泵线程写路径（`write_pty_with_drain` + `pump_channel_reads`）对真实 sshd 的回归测试：
//! 大量写入期间远端持续输出时，通道不能被关闭，入站数据也不能被丢弃。

use super::manager::SshManager;
use super::SshMessage;
use crate::test_support::ssh_local::skip_without_sshd;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const READY: &[u8] = b"MIST-READY";

struct StressOutcome {
    sent: u64,
    received: u64,
    failure: Option<String>,
}

/// 远端命令须在 `stty raw -echo` 之后打印 `READY`，之后的字节才计入 `received`。
fn stress_pty_writes(remote_cmd: &str, total: u64, settle_until_received: bool) -> Option<StressOutcome> {
    let session = skip_without_sshd()?;
    let mut channel = session.channel_session().expect("channel_session");
    channel
        .request_pty("xterm-256color", None, Some((200, 50, 0, 0)))
        .expect("request_pty");
    channel.exec(remote_cmd).expect("exec");

    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while !seen.ends_with(READY) {
        assert!(channel.read(&mut byte).expect("read READY") == 1, "EOF before READY");
        seen.push(byte[0]);
    }
    session.set_blocking(false);

    let (tx, rx) = mpsc::channel::<SshMessage>();
    let received = Arc::new(AtomicU64::new(0));
    let failure = Arc::new(Mutex::new(None::<String>));
    let drain = {
        let received = received.clone();
        let failure = failure.clone();
        std::thread::spawn(move || {
            for msg in rx {
                match msg {
                    SshMessage::Output { data, .. } => {
                        received.fetch_add(data.len() as u64, Ordering::Relaxed);
                    }
                    SshMessage::Disconnected { .. } => {
                        *failure.lock().unwrap() = Some("channel EOF".into());
                    }
                    SshMessage::Error { error, .. } => {
                        *failure.lock().unwrap() = Some(error);
                    }
                    _ => {}
                }
            }
        })
    };

    let bypass = Arc::new(Mutex::new(None));
    let mut read_buffer = [0u8; 16384];
    let chunk: Vec<u8> = (0..32 * 1024).map(|i| b' ' + (i % 95) as u8).collect();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut sent = 0u64;
    let mut write_error = None;

    while sent < total && Instant::now() < deadline && failure.lock().unwrap().is_none() {
        if let Err(e) = SshManager::write_pty_with_drain(&mut channel, &chunk, &mut read_buffer, &tx, 0, &bypass)
        {
            write_error = Some(e.to_string());
            break;
        }
        sent += chunk.len() as u64;
        if SshManager::pump_channel_reads(&mut channel, &mut read_buffer, &tx, 0, &bypass, None).is_err() {
            break;
        }
    }

    if settle_until_received && write_error.is_none() {
        let settle_deadline = Instant::now() + Duration::from_secs(15);
        while received.load(Ordering::Relaxed) < sent && Instant::now() < settle_deadline {
            if SshManager::pump_channel_reads(&mut channel, &mut read_buffer, &tx, 0, &bypass, None).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    let _ = channel.close();
    drop(tx);
    drain.join().unwrap();
    let failure = write_error.or_else(|| failure.lock().unwrap().clone());
    Some(StressOutcome {
        sent,
        received: received.load(Ordering::Relaxed),
        failure,
    })
}

#[test]
fn tiny_inbound_packets_do_not_close_channel_during_writes() {
    let cmd = "stty raw -echo; printf MIST-READY; (while :; do printf ok; sleep 0.002; done) & exec cat > /dev/null";
    let Some(out) = stress_pty_writes(cmd, 16 * 1024 * 1024, false) else {
        return;
    };
    assert!(
        out.failure.is_none() && out.sent == 16 * 1024 * 1024,
        "channel died after {} bytes: {:?}",
        out.sent,
        out.failure
    );
}

#[test]
fn inbound_output_is_not_discarded_during_writes() {
    let Some(out) = stress_pty_writes("stty raw -echo; printf MIST-READY; exec cat", 8 * 1024 * 1024, true) else {
        return;
    };
    assert!(out.failure.is_none(), "channel failed: {:?}", out.failure);
    assert_eq!(out.received, out.sent, "echoed bytes lost while writing");
}
