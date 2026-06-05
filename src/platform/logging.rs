//! 运行时日志：GUI 独立启动时不向控制台刷屏（macOS `open Mist.app` 等）。

use std::io::IsTerminal;

/// 初始化 tracing / log 输出。
///
/// - 终端已连接（`cargo run`、shell 里直接运行）：按 `RUST_LOG` 或默认级别写 stderr。
/// - GUI 独立启动（stderr 非 TTY）：默认静默；设 `MIST_LOG=1` 或 `RUST_LOG=…` 可强制开启。
pub fn init_runtime_logging() {
    use tracing_subscriber::{EnvFilter, fmt};

    let force = std::env::var("MIST_LOG").is_ok() || std::env::var("RUST_LOG").is_ok();
    let stderr_tty = std::io::stderr().is_terminal();

    if !stderr_tty && !force {
        return;
    }

    let default_level = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));

    let subscriber = fmt()
        .with_env_filter(filter)
        .with_target(cfg!(debug_assertions))
        .with_thread_ids(cfg!(debug_assertions));

    subscriber.init();
}
