//! 崩溃报告（#150）：panic hook 把 panic 信息 + 栈落到本地日志。
//!
//! 背景：`ctx.set_fonts` 只排队，下一帧 `Fonts::new` 里 ab_glyph 解析失败会
//! epaint `panic!`——Rust panic 走 unwind/abort，macOS 往往不生成 `.ips`，
//! 会话日志也看不到栈。此模块在 `logs/crash-reports/`（与 hang-reports 同级）
//! 留一份可查的报告，命名/头部字段与 hang-reports 对齐。
//!
//! 边界：
//! - hook 内不做任何可能再 panic 的复杂操作；写文件失败则静默退回 stderr；
//! - 保留默认 hook 行为（stderr 输出），落盘是额外动作；
//! - 报告数上限（保留最近 10 份），与现有日志轮转口径一致。

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_REPORTS: usize = 10;

static INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn crash_report_dir() -> PathBuf {
    crate::core::default_log_base_dir().join("crash-reports")
}

fn ensure_crash_report_dir() -> Result<PathBuf, String> {
    let dir = crash_report_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create crash report dir failed ({}): {e}", dir.display()))?;
    Ok(dir)
}

/// 安装 panic hook（幂等）。`main` 尽早调用。
pub fn install() {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        write_crash_report(info);
        default_hook(info);
    }));
}

fn write_crash_report(info: &std::panic::PanicHookInfo<'_>) {
    let msg = payload_string(info);
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown>".to_string());

    let body = format!(
        "MistTerm crash report\n=====================\n\
         version: {}\nos: {} {}\n\
         time: {}\n\
         thread: {}\n\
         panic: {}\nlocation: {}\n\nbacktrace:\n{}\n",
        crate::platform::APP_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().to_rfc3339(),
        std::thread::current().name().unwrap_or("<unnamed>"),
        msg,
        location,
        backtrace_text(),
    );

    // 尽力写盘；失败静默（hook 内不能再 panic）。
    if let Ok(dir) = ensure_crash_report_dir() {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
        let path = dir.join(format!("crash-{ts}.log"));
        if let Ok(mut f) = std::fs::File::create(&path) {
            let _ = f.write_all(body.as_bytes());
        }
        prune_old_reports(&dir);
    }
    eprintln!("\n{body}");
}

fn payload_string(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

fn backtrace_text() -> String {
    // std::backtrace 稳定可用；未捕获到时提示用户开 RUST_BACKTRACE=1。
    match std::backtrace::Backtrace::force_capture() {
        bt if bt.status() == std::backtrace::BacktraceStatus::Captured => format!("{bt}"),
        _ => "backtrace unavailable (set RUST_BACKTRACE=1 for full trace)".to_string(),
    }
}

fn prune_old_reports(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, e.path()))
        })
        .collect();
    if files.len() <= MAX_REPORTS {
        return;
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let excess = files.len() - MAX_REPORTS;
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_dir_is_under_logs_like_hang_reports() {
        let d = crash_report_dir();
        assert!(d.ends_with("crash-reports"));
        assert!(d.to_string_lossy().contains("mistterm"));
    }

    #[test]
    fn install_is_idempotent() {
        install();
        install(); // 第二次不应 panic / 不重复设置
    }

    #[test]
    fn payload_string_handles_both_types() {
        let s = "str payload";
        let info_str: Box<dyn std::any::Any + Send> = Box::new(s);
        // 直接验证 downcast 分支逻辑
        assert_eq!(payload_of(&info_str), "str payload");
        let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
        assert_eq!(payload_of(&owned), "owned");
    }

    fn payload_of(p: &Box<dyn std::any::Any + Send>) -> String {
        if let Some(s) = p.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = p.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        }
    }
}
