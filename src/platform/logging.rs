//! 运行时日志：GUI 独立启动时不向控制台刷屏（macOS `open Mist.app` 等）。

use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 可选的文件日志（`MIST_LOG_FILE=path`）。
static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

struct TeeWriter;

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(buf);
        if let Some(file) = LOG_FILE.get() {
            if let Ok(mut g) = file.lock() {
                let _ = g.write_all(buf);
                let _ = g.flush();
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Some(file) = LOG_FILE.get() {
            if let Ok(mut g) = file.lock() {
                let _ = g.flush();
            }
        }
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TeeWriter {
    type Writer = TeeWriter;

    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter
    }
}

/// 初始化 tracing / log 输出。
///
/// - 终端已连接（`cargo run`、shell 里直接运行）：按 `RUST_LOG` 或默认级别写 stderr。
/// - GUI 独立启动（stderr 非 TTY）：默认静默；设 `MIST_LOG=1` 或 `RUST_LOG=…` 可强制开启。
/// - 设 `MIST_LOG_FILE=path` 时同时写入该文件（方便 GUI 手测后把日志贴回来）。
pub fn init_runtime_logging() {
    use tracing_subscriber::{EnvFilter, fmt};

    let force = std::env::var("MIST_LOG").is_ok() || std::env::var("RUST_LOG").is_ok();
    let stderr_tty = std::io::stderr().is_terminal();
    let log_file_path = std::env::var("MIST_LOG_FILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);

    if !stderr_tty && !force && log_file_path.is_none() {
        return;
    }

    if let Some(path) = &log_file_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(f) => {
                let _ = LOG_FILE.set(Mutex::new(f));
            }
            Err(e) => {
                eprintln!("MIST_LOG_FILE open failed ({}): {}", path.display(), e);
            }
        }
    }

    let default_level = if cfg!(debug_assertions) {
        "info"
    } else {
        "info"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_writer(TeeWriter);

    subscriber.init();
    // 桥接 `log` crate：本项目绝大多数日志用 `log::info!` 等宏记录，
    // 不装 LogTracer 的话这些记录会被 log crate 静默丢弃（GUI 模式下等于零日志）。
    let _ = tracing_log::LogTracer::init();
    log::info!(
        "runtime logging initialized (log bridge installed); MIST_LOG_FILE={}",
        log_file_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".into())
    );
}
