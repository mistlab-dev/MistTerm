//! 使用与 MistTerm 相同的 `zmodem2::Sender` 路径，经 **stdin/stdout** 与 `rz -bye` 对跑冒烟测试。
//!
//! ```text
//! # 用你的服务器（须已配置 ssh 免密或 ssh-agent；**`-tt` 强制远端 PTY**）
//! export RZ_SMOKE_SSH=ubuntu@你的主机
//! # 可选：RZ_SMOKE_SSH_PORT=22  RZ_SMOKE_SSH_IDENTITY=~/.ssh/id_ed25519
//! # 可选：远端命令（默认在 /tmp 收文件）
//! # export RZ_SMOKE_REMOTE_CMD='cd /tmp && rz -bye'
//! # 若 exit 3 / 128 等可疑退出码，可与 BatchMode 对照（默认开启 BatchMode=yes）
//! # RZ_SMOKE_SSH_BATCH_MODE=0 cargo run -p mistterm --bin rz_upload_smoke --release
//! cargo run -p mistterm --bin rz_upload_smoke --release
//!
//! # 仅本机 rz（管道、无 PTY，易超时；对照用）
//! RZ_SMOKE_LOCAL=1 cargo run -p mistterm --bin rz_upload_smoke --release
//! ```
//!
//! 超时：本机默认 10s，SSH 默认 90s（`RZ_SMOKE_DEADLINE_SECS`）；文件个数默认 3（`RZ_SMOKE_FILE_COUNT=1..20`）。
//! `RZ_SMOKE_NO_STRIP=1`：握手期不调 `strip_handshake_incoming`（仅调试用；与主程序 `MISTTERM_ZMODEM_NO_HANDSHAKE_STRIP` 对照）。
//! `RZ_SMOKE_DEBUG=1`：首段 ssh stdout 打 hex；exit 3 时总会多打一行收/发字节与缓冲残留（无需 DEBUG）。
//! `RZ_SMOKE_DUMP_TX=1`：把**写入 ssh/rz 侧 stdin 的全部字节**打 hex（便于与外部 `sz` 抓包对照）。
//! 退出码：`0` 成功；`1` 写失败/内容不一致/ssh 非零；`2` 超时；`3` 连接已断但 ZMODEM 未完成。
//! **非 LOCAL 时必须设置 `RZ_SMOKE_SSH`**，不再内置默认主机。
//!
//! 说明：本机 `RZ_SMOKE_LOCAL=1` 使用**管道**连 `rz`；不少 lrzsz 版本在无 TTY 时与 SSH PTY 上行为不一致，
//! 冒烟可能超时。**`cargo test --lib`** 与 **`cargo test --manifest-path vendor/zmodem2/Cargo.toml --test protocol`**
//! 可稳定覆盖协议与 PTY 前导剥离逻辑。
//!
//! 服务器上粗测「stdin 立刻 EOF」时：`rz -bye` 仍可能往 **stdout 写 ZMODEM 邀请帧**，与同一行的 `echo $?` 粘连，
//! 看起来像乱码。要看干净退出码请重定向：`cd /tmp && /usr/bin/rz -bye </dev/null >/dev/null 2>&1; echo EXIT:$?`
//! （此时多为 `128`，表示无有效发送端，**不说明** SSH 冒烟失败原因）。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use mistterm::ssh::zmodem_pty_prefix::strip_handshake_incoming;

const DEADLINE_LOCAL: Duration = Duration::from_secs(10);
const DEADLINE_SSH: Duration = Duration::from_secs(90);
const FILE_COUNT: usize = 3;
const FILE_SIZE: usize = 64;

/// 默认 `yes`（CI/无人值守）；设 `0`/`false` 则不加 `-oBatchMode=yes`，便于与交互式 `ssh` 行为对照。
fn ssh_wants_batch_mode() -> bool {
    std::env::var("RZ_SMOKE_SSH_BATCH_MODE")
        .map(|v| {
            let v = v.trim();
            v == "1"
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(true)
}

fn smoke_debug() -> bool {
    std::env::var("RZ_SMOKE_DEBUG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn smoke_dump_tx() -> bool {
    std::env::var("RZ_SMOKE_DUMP_TX")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn hex_prefix(data: &[u8], max: usize) -> String {
    data.iter()
        .take(max)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 多行 hex（每行 32 字节 + 十六进制偏移），用于 TX dump。
fn hex_dump_block(data: &[u8]) {
    let mut off = 0usize;
    for row in data.chunks(32) {
        eprintln!(
            "    {:04x}  {}",
            off,
            row.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ")
        );
        off += row.len();
    }
}

fn flush_zmodem_out(
    sender: &mut zmodem2::Sender,
    stdin: &mut std::process::ChildStdin,
    temp_root: &Path,
    to_child_stdin: &mut u64,
    dump_tx: bool,
    tx_dump: &mut Vec<u8>,
) -> bool {
    let mut any = false;
    while !sender.drain_outgoing().is_empty() {
        any = true;
        let out = sender.drain_outgoing().to_vec();
        let n = out.len();
        if dump_tx {
            tx_dump.extend_from_slice(&out);
        }
        if let Err(e) = stdin.write_all(&out) {
            eprintln!(
                "rz_upload_smoke: 写入 ssh/rz stdin 失败（多为连接已断）: {e}"
            );
            let _ = std::fs::remove_dir_all(temp_root);
            std::process::exit(1);
        }
        let _ = stdin.flush();
        *to_child_stdin += n as u64;
        sender.advance_outgoing(n);
    }
    any
}

fn main() {
    let local = std::env::var("RZ_SMOKE_LOCAL").map(|v| v == "1" || v.eq_ignore_ascii_case("true")) == Ok(true);
    let ssh_target = if local {
        None
    } else {
        let t = std::env::var("RZ_SMOKE_SSH").unwrap_or_else(|_| String::new());
        let t = t.trim().to_string();
        if t.is_empty() {
            eprintln!(
                "rz_upload_smoke: 未设置 RZ_SMOKE_SSH。示例:\n  export RZ_SMOKE_SSH=ubuntu@你的主机\n  cargo run -p mistterm --bin rz_upload_smoke --release"
            );
            std::process::exit(1);
        }
        Some(t)
    };
    let deadline_dur = std::env::var("RZ_SMOKE_DEADLINE_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(if local {
            DEADLINE_LOCAL
        } else {
            DEADLINE_SSH
        });
    let file_count = std::env::var("RZ_SMOKE_FILE_COUNT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| (1..=20).contains(&n))
        .unwrap_or(FILE_COUNT);
    let no_strip = std::env::var("RZ_SMOKE_NO_STRIP").map(|v| v == "1" || v.eq_ignore_ascii_case("true")) == Ok(true);
    let dump_tx = smoke_dump_tx();
    let mut tx_dump_buf = Vec::<u8>::new();

    let root = std::env::temp_dir().join(format!("mistterm_rz_smoke_{}", std::process::id()));
    let outbound = root.join("outbound");
    let inbound = root.join("inbound");
    std::fs::create_dir_all(&outbound).expect("mkdir outbound");
    std::fs::create_dir_all(&inbound).expect("mkdir inbound");

    let paths: Vec<PathBuf> = (0..file_count)
        .map(|i| {
            let p = outbound.join(format!("rz_smoke_{i}.dat"));
            let body: Vec<u8> = (0..FILE_SIZE)
                .map(|b| ((b as u16 + i as u16) % 256) as u8)
                .collect();
            std::fs::write(&p, &body).expect("write smoke file");
            p
        })
        .collect();

    let remote_cmd = std::env::var("RZ_SMOKE_REMOTE_CMD")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "cd /tmp && rz -bye".to_string());

    let mut child = if local {
        Command::new("rz")
            .args(["-bye"])
            .current_dir(&inbound)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
    } else {
        let target = ssh_target.as_ref().expect("ssh target");
        let batch = ssh_wants_batch_mode();
        eprintln!(
            "rz_upload_smoke: SSH {} remote: {} (BatchMode={})",
            target,
            remote_cmd,
            if batch { "yes" } else { "off" }
        );
        let mut cmd = Command::new("ssh");
        cmd.arg("-tt");
        if batch {
            cmd.arg("-oBatchMode=yes");
        }
        cmd.args([
            "-oConnectTimeout=15",
            "-oServerAliveInterval=5",
            "-oServerAliveCountMax=3",
        ]);
        if let Ok(port) = std::env::var("RZ_SMOKE_SSH_PORT") {
            let p = port.trim();
            if !p.is_empty() {
                cmd.arg("-p").arg(p);
            }
        }
        if let Ok(id) = std::env::var("RZ_SMOKE_SSH_IDENTITY") {
            let id = id.trim();
            if !id.is_empty() {
                cmd.arg("-i").arg(id);
            }
        }
        cmd.arg(target)
            .arg(&remote_cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn()
    }
    .unwrap_or_else(|e| {
        eprintln!("spawn ssh/rz failed: {e}");
        let _ = std::fs::remove_dir_all(&root);
        std::process::exit(1);
    });

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let stderr = child.stderr.take().expect("stderr");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut stdout = stdout;
        let mut buf = [0u8; 8192];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    // 必须持续读 stderr，否则 ssh 写满管道后会阻塞，远端 `rz` 表现为秒退或僵死。
    thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = [0u8; 8192];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = std::io::stderr().write_all(&buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    let mut open_files: HashMap<String, File> = HashMap::new();
    for path in &paths {
        let filename = path.file_name().unwrap().to_str().unwrap().to_string();
        let file = File::open(path).expect("open");
        open_files.insert(filename, file);
    }

    let mut file_iter = paths.iter();
    let first_path = file_iter.next().expect("files");
    let first_filename = first_path.file_name().unwrap().to_str().unwrap();
    let first_size = first_path.metadata().unwrap().len() as u32;

    let mut sender = zmodem2::Sender::new().expect("Sender::new");
    {
        let pre = sender.drain_outgoing();
        let n = pre.len();
        sender.advance_outgoing(n);
    }
    sender
        .start_file(first_filename.as_bytes(), first_size)
        .expect("start_file");

    let mut current_filename = first_filename.to_string();
    let mut input_buf: Vec<u8> = Vec::new();
    let mut file_buf = [0u8; 1024];
    let mut session_done = false;
    let mut file_data_started = false;
    let deadline = Instant::now() + deadline_dur;
    let mut total_from_ssh: u64 = 0;
    let mut to_child_stdin: u64 = 0;
    let mut first_rx_hex_done = false;

    while !session_done || !sender.drain_outgoing().is_empty() {
        if Instant::now() > deadline {
            eprintln!("rz_upload_smoke: 超过 {deadline_dur:?} 仍未完成，中止");
            if dump_tx && !tx_dump_buf.is_empty() {
                eprintln!(
                    "rz_upload_smoke: [dump-tx] 已累计写入 stdin {} B：",
                    tx_dump_buf.len()
                );
                hex_dump_block(&tx_dump_buf);
            }
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&root);
            std::process::exit(2);
        }

        if !session_done {
            if let Ok(Some(st)) = child.try_wait() {
                eprintln!(
                    "rz_upload_smoke: SSH/rz 子进程已退出（{st}），但 ZMODEM 会话尚未完成"
                );
                eprintln!(
                    "rz_upload_smoke: 诊断：ssh stdout 累计 {} B，已向 ssh stdin 写 {} B，input_buf 残留 {} B，session_done={} file_data_started={}",
                    total_from_ssh,
                    to_child_stdin,
                    input_buf.len(),
                    session_done,
                    file_data_started
                );
                if smoke_debug() && !input_buf.is_empty() {
                    eprintln!(
                        "rz_upload_smoke: [debug] input_buf 前 64B: {}",
                        hex_prefix(&input_buf, 64)
                    );
                }
                if dump_tx && !tx_dump_buf.is_empty() {
                    eprintln!(
                        "rz_upload_smoke: [dump-tx] 已累计写入 ssh stdin {} B：",
                        tx_dump_buf.len()
                    );
                    hex_dump_block(&tx_dump_buf);
                }
                eprintln!(
                    "rz_upload_smoke: 若上面刚出现「Connection closed」，多为远端 rz 秒退：服务器是否已安装 lrzsz（which rz）、/tmp 是否可写、登录 shell 是否正常。"
                );
                let _ = std::fs::remove_dir_all(&root);
                std::process::exit(3);
            }
        }

        let mut progressed = false;

        while let Ok(chunk) = rx.try_recv() {
            if smoke_debug() && !first_rx_hex_done && !chunk.is_empty() {
                eprintln!(
                    "rz_upload_smoke: [debug] 首包 stdout 前 64B: {}",
                    hex_prefix(&chunk, 64)
                );
                first_rx_hex_done = true;
            }
            total_from_ssh += chunk.len() as u64;
            input_buf.extend_from_slice(&chunk);
            progressed = true;
        }
        if !file_data_started && !no_strip {
            // 与 `zmodem_pty_pipeline` 握手期一致：CSI/内嵌终端序列/对齐 ZPAD/纯提示符块，避免只「对齐 *」时仍把半帧当噪声。
            let n = strip_handshake_incoming(&mut input_buf);
            if n > 0 {
                eprintln!("rz_upload_smoke: 握手段剥除 {n} 字节（strip_handshake_incoming，与主程序一致）");
            }
        }

        if flush_zmodem_out(
            &mut sender,
            &mut stdin,
            &root,
            &mut to_child_stdin,
            dump_tx,
            &mut tx_dump_buf,
        ) {
            progressed = true;
        }

        while !input_buf.is_empty() {
            if flush_zmodem_out(
                &mut sender,
                &mut stdin,
                &root,
                &mut to_child_stdin,
                dump_tx,
                &mut tx_dump_buf,
            ) {
                progressed = true;
            }
            let consumed = sender
                .feed_incoming(&input_buf)
                .expect("feed_incoming");
            if consumed == 0 {
                break;
            }
            input_buf.drain(..consumed);
            progressed = true;
        }

        while let Some(request) = sender.poll_file() {
            file_data_started = true;
            let file = open_files.get_mut(&current_filename).expect("file map");
            file.seek(std::io::SeekFrom::Start(u64::from(request.offset)))
                .unwrap();
            let n = file.read(&mut file_buf[..request.len]).unwrap();
            sender.feed_file(&file_buf[..n]).expect("feed_file");
            progressed = true;
            if flush_zmodem_out(
                &mut sender,
                &mut stdin,
                &root,
                &mut to_child_stdin,
                dump_tx,
                &mut tx_dump_buf,
            ) {
                progressed = true;
            }
        }

        if flush_zmodem_out(
            &mut sender,
            &mut stdin,
            &root,
            &mut to_child_stdin,
            dump_tx,
            &mut tx_dump_buf,
        ) {
            progressed = true;
        }

        while let Some(event) = sender.poll_event() {
            match event {
                zmodem2::SenderEvent::FileComplete => {
                    if let Some(next_path) = file_iter.next() {
                        let next_filename = next_path.file_name().unwrap().to_str().unwrap();
                        let next_size = next_path.metadata().unwrap().len() as u32;
                        sender
                            .start_file(next_filename.as_bytes(), next_size)
                            .expect("start_file next");
                        current_filename = next_filename.to_string();
                        file_data_started = false;
                    } else {
                        sender.finish_session().expect("finish_session");
                    }
                    progressed = true;
                }
                zmodem2::SenderEvent::SessionComplete => {
                    session_done = true;
                    progressed = true;
                }
            }
            if flush_zmodem_out(
                &mut sender,
                &mut stdin,
                &root,
                &mut to_child_stdin,
                dump_tx,
                &mut tx_dump_buf,
            ) {
                progressed = true;
            }
        }

        if flush_zmodem_out(
            &mut sender,
            &mut stdin,
            &root,
            &mut to_child_stdin,
            dump_tx,
            &mut tx_dump_buf,
        ) {
            progressed = true;
        }

        // 与 MistTerm 泵类似：同一调度周期内 stdout 可能连读多段，再扫一次避免空转一轮才喂协议。
        while let Ok(chunk) = rx.try_recv() {
            if smoke_debug() && !first_rx_hex_done && !chunk.is_empty() {
                eprintln!(
                    "rz_upload_smoke: [debug] 首包 stdout 前 64B: {}",
                    hex_prefix(&chunk, 64)
                );
                first_rx_hex_done = true;
            }
            total_from_ssh += chunk.len() as u64;
            input_buf.extend_from_slice(&chunk);
            progressed = true;
        }

        if !progressed {
            thread::sleep(Duration::from_millis(5));
        }
    }

    let status = child.wait().expect("wait child");

    if local {
        for path in &paths {
            let name = path.file_name().unwrap();
            let recv = inbound.join(name);
            if !recv.exists() {
                eprintln!("missing received file {}", recv.display());
                let _ = std::fs::remove_dir_all(&root);
                std::process::exit(1);
            }
            let a = std::fs::read(path).unwrap();
            let b = std::fs::read(&recv).unwrap();
            if a != b {
                eprintln!("content mismatch {}", name.to_string_lossy());
                let _ = std::fs::remove_dir_all(&root);
                std::process::exit(1);
            }
        }
    } else if !status.success() {
        eprintln!("rz_upload_smoke: ssh/rz 异常退出: {status}");
        eprintln!("rz_upload_smoke: 请在服务器执行: which rz && rz -V && ls -la /tmp");
        let _ = std::fs::remove_dir_all(&root);
        std::process::exit(1);
    } else {
        eprintln!("rz_upload_smoke: 远程会话正常结束（请到服务器 /tmp 核对 rz_smoke_*.dat 内容与字节数）");
    }

    if dump_tx && !tx_dump_buf.is_empty() {
        eprintln!(
            "rz_upload_smoke: [dump-tx] 已累计写入子进程 stdin {} B：",
            tx_dump_buf.len()
        );
        hex_dump_block(&tx_dump_buf);
    }

    let _ = std::fs::remove_dir_all(&root);
    eprintln!(
        "rz_upload_smoke: 成功 —— {} 个文件 × {} B（退出码 0）",
        file_count,
        FILE_SIZE
    );
}
