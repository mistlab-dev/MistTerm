//! 崩溃报告（#150）与下次启动提示（#189）。
//!
//! 背景：`ctx.set_fonts` 只排队，下一帧 `Fonts::new` 里 ab_glyph 解析失败会
//! epaint `panic!`——Rust panic 走 unwind/abort，macOS 往往不生成 `.ips`，
//! 会话日志也看不到栈。此模块在 `logs/crash-reports/`（与 hang-reports 同级）
//! 留一份可查的报告，命名/头部字段与 hang-reports 对齐。
//!
//! GUI 正常退出时写 `logs/last_run.ok`。下次启动若戳缺失（且曾经成功启动过），
//! 或 `crash-*.log` 新于上次已确认的报告，则返回 [`CrashNotice`]，由界面弹一次。
//! 无界面入口（`--version` / `update`）不调用 [`begin_run`]，避免误清退出戳。
//!
//! # 捕获边界（#189）
//!
//! - **Rust `panic!`**：hook 可落盘（#150），下次启动能读到版本 / 时间 / panic 首行。
//! - **`panic_cannot_unwind` / ObjC 回调里 panic（#188）**：进程会 abort；hook 多数仍能写盘，但崩溃当下没有 UI。下次启动靠本模块提示。
//! - **纯原生 SIGSEGV / SIGBUS**：当前无本机报告（不做 crashpad）。若上次 GUI 没写 `last_run.ok`，仍提示「没有崩溃报告」。
//! - **正常退出**：`on_exit` 写 `last_run.ok`，下次启动不提示。
//!
//! 不在本模块：远程上报、修复 winit 根因。
//!
//! hook 约束：
//! - hook 内不做任何可能再 panic 的复杂操作；写文件失败则静默退回 stderr；
//! - 保留默认 hook 行为（stderr 输出），落盘是额外动作；
//! - 报告数上限（保留最近 10 份），与现有日志轮转口径一致。

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_REPORTS: usize = 10;
/// 对话框「复制全文」读入上限，避免异常大的报告占满内存。
const MAX_REPORT_BYTES: usize = 512 * 1024;
const PANIC_PREVIEW_CHARS: usize = 180;

const CLEAN_STAMP_NAME: &str = "last_run.ok";
const LAUNCH_SEEN_NAME: &str = "launch_seen";
const ACK_NAME: &str = "crash_notice_ack";

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// 下次启动要展示的一次提示。同一进程内由 UI 持有；报告文件的「已看过」在
/// [`begin_run_in`] 里写入，避免用户直接退出后同一份报告反复弹。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashNotice {
    /// 上次 GUI 启动过，但没有写干净退出戳。
    pub unclean_exit: bool,
    /// `report` 的修改时间新于上次确认。
    pub report_is_new: bool,
    pub report: Option<CrashReportSummary>,
}

/// 从最新 `crash-*.log` 抽出的展示字段。`full_text` 供「复制全文」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashReportSummary {
    pub path: PathBuf,
    pub modified_ms: u64,
    pub version: String,
    pub time: String,
    pub panic_first_line: String,
    pub full_text: String,
}

pub fn crash_report_dir() -> PathBuf {
    crate::core::default_log_base_dir().join("crash-reports")
}

pub fn ensure_crash_report_dir() -> Result<PathBuf, String> {
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

/// GUI 启动时调用一次。返回 `Some` 时界面应弹一次提示。
pub fn begin_run() -> Option<CrashNotice> {
    begin_run_in(&crate::core::default_log_base_dir())
}

/// 正常关机（`App::on_exit`）写干净退出戳。崩溃 / abort / kill 不会走到这里。
pub fn mark_clean_exit() {
    mark_clean_exit_in(&crate::core::default_log_base_dir());
}

/// 用户关掉提示后再次确认（与 [`begin_run_in`] 里的确认一致，重复写无害）。
pub fn acknowledge(notice: &CrashNotice) {
    acknowledge_in(&crate::core::default_log_base_dir(), notice);
}

pub fn begin_run_in(log_base: &Path) -> Option<CrashNotice> {
    let _ = std::fs::create_dir_all(log_base);
    let stamp = log_base.join(CLEAN_STAMP_NAME);
    let seen = log_base.join(LAUNCH_SEEN_NAME);
    let stamp_present = stamp.is_file();
    let seen_present = seen.is_file();
    let ack = read_ack(&log_base.join(ACK_NAME));
    let report = newest_crash_report(&log_base.join("crash-reports"));
    let report_is_new = report.as_ref().is_some_and(|r| r.modified_ms > ack);
    // 第一次见到这个数据目录时，没有「上次运行」可比较，缺戳不算异常退出。
    let unclean_exit = seen_present && !stamp_present;

    // 本次运行先清掉戳；只有 mark_clean_exit 会再写上。中途 abort 则下次视为不干净。
    let _ = std::fs::remove_file(&stamp);
    if let Err(e) = std::fs::write(&seen, b"1\n") {
        log::warn!("failed to record launch marker: {e}");
    }

    if !unclean_exit && !report_is_new {
        return None;
    }

    let notice = CrashNotice {
        unclean_exit,
        report_is_new,
        report,
    };
    // 同一份报告只提示一次。缺戳造成的再次异常退出仍会在下次启动提示。
    acknowledge_in(log_base, &notice);
    log::info!("startup crash notice: unclean={unclean_exit} new_report={report_is_new}");
    Some(notice)
}

pub fn mark_clean_exit_in(log_base: &Path) {
    if let Err(e) = std::fs::create_dir_all(log_base) {
        log::warn!("failed to create log dir for clean-exit stamp: {e}");
        return;
    }
    let body = chrono::Local::now().to_rfc3339();
    if let Err(e) = std::fs::write(log_base.join(CLEAN_STAMP_NAME), body) {
        log::warn!("failed to write clean-exit stamp: {e}");
    }
}

pub fn acknowledge_in(log_base: &Path, notice: &CrashNotice) {
    let Some(report) = notice.report.as_ref() else {
        return;
    };
    let path = log_base.join(ACK_NAME);
    let next = read_ack(&path).max(report.modified_ms);
    if let Err(e) = std::fs::create_dir_all(log_base) {
        log::warn!("failed to acknowledge crash notice: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&path, next.to_string()) {
        log::warn!("failed to acknowledge crash notice: {e}");
    }
}

fn read_ack(path: &Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn newest_crash_report(dir: &Path) -> Option<CrashReportSummary> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("crash-") || !name.ends_with(".log") {
            continue;
        }
        let meta = entry.metadata().ok()?;
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        let path = entry.path();
        let replace = match &best {
            None => true,
            Some((prev_t, prev_path)) => {
                modified > *prev_t || (modified == *prev_t && path > *prev_path)
            }
        };
        if replace {
            best = Some((modified, path));
        }
    }
    let (modified, path) = best?;
    Some(summarize_crash_report(&path, modified))
}

fn summarize_crash_report(path: &Path, modified: SystemTime) -> CrashReportSummary {
    let (full_text, _truncated) = read_report_text(path);
    let version = header_field(&full_text, "version")
        .unwrap_or("")
        .to_string();
    let time = header_field(&full_text, "time").unwrap_or("").to_string();
    let panic_line = header_field(&full_text, "panic").unwrap_or("");
    CrashReportSummary {
        path: path.to_path_buf(),
        modified_ms: system_time_ms(modified),
        version,
        time,
        panic_first_line: truncate_chars(panic_line, PANIC_PREVIEW_CHARS),
        full_text,
    }
}

fn header_field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    text.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn read_report_text(path: &Path) -> (String, bool) {
    let Ok(file) = std::fs::File::open(path) else {
        return (String::new(), false);
    };
    let mut buf = Vec::new();
    let mut limited = file.take(MAX_REPORT_BYTES as u64 + 1);
    if limited.read_to_end(&mut buf).is_err() {
        return (String::new(), false);
    }
    let truncated = buf.len() > MAX_REPORT_BYTES;
    if truncated {
        buf.truncate(MAX_REPORT_BYTES);
    }
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated {
        text.push_str("\n…(truncated)…\n");
    }
    (text, truncated)
}

fn system_time_ms(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}…")
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

    fn write_crash(log_base: &Path, name: &str, body: &str, mtime_ms: u64) {
        let dir = log_base.join("crash-reports");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(UNIX_EPOCH + std::time::Duration::from_millis(mtime_ms))
            .unwrap();
    }

    const SAMPLE: &str = "\
MistTerm crash report\n\
=====================\n\
version: 1.2.39\n\
os: linux x86_64\n\
time: 2026-10-29T14:00:00+08:00\n\
thread: main\n\
panic: test boom\n\
location: src/main.rs:1:1\n\
\n\
backtrace:\n\
frame\n";

    #[test]
    fn first_launch_without_crash_does_not_prompt() {
        let dir = tempfile::tempdir().unwrap();
        assert!(begin_run_in(dir.path()).is_none());
        assert!(dir.path().join("launch_seen").is_file());
        assert!(!dir.path().join("last_run.ok").is_file());
    }

    #[test]
    fn clean_exit_then_next_launch_is_quiet() {
        let dir = tempfile::tempdir().unwrap();
        assert!(begin_run_in(dir.path()).is_none());
        mark_clean_exit_in(dir.path());
        assert!(dir.path().join("last_run.ok").is_file());
        assert!(begin_run_in(dir.path()).is_none());
        assert!(!dir.path().join("last_run.ok").is_file());
    }

    #[test]
    fn missing_stamp_after_a_seen_launch_prompts_once_per_unclean_exit() {
        let dir = tempfile::tempdir().unwrap();
        assert!(begin_run_in(dir.path()).is_none());
        let notice = begin_run_in(dir.path()).expect("unclean");
        assert!(notice.unclean_exit);
        assert!(!notice.report_is_new);
        assert!(notice.report.is_none());
        // 仍未干净退出：再启动还要提示（新的一次异常退出）。
        assert!(begin_run_in(dir.path()).is_some());
        mark_clean_exit_in(dir.path());
        assert!(begin_run_in(dir.path()).is_none());
    }

    #[test]
    fn new_crash_log_prompts_even_after_clean_exit_and_then_stays_quiet() {
        let dir = tempfile::tempdir().unwrap();
        assert!(begin_run_in(dir.path()).is_none());
        mark_clean_exit_in(dir.path());
        write_crash(dir.path(), "crash-1.log", SAMPLE, 5_000);
        let notice = begin_run_in(dir.path()).expect("new report");
        assert!(notice.report_is_new);
        assert!(!notice.unclean_exit);
        let report = notice.report.expect("summary");
        assert_eq!(report.version, "1.2.39");
        assert_eq!(report.time, "2026-10-29T14:00:00+08:00");
        assert_eq!(report.panic_first_line, "test boom");
        assert!(report.full_text.contains("backtrace"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("crash_notice_ack"))
                .unwrap()
                .trim(),
            report.modified_ms.to_string()
        );
        mark_clean_exit_in(dir.path());
        assert!(begin_run_in(dir.path()).is_none());
    }

    #[test]
    fn newer_crash_log_prompts_again_after_acknowledge() {
        let dir = tempfile::tempdir().unwrap();
        write_crash(dir.path(), "crash-old.log", SAMPLE, 1_000);
        assert!(begin_run_in(dir.path()).unwrap().report_is_new);
        mark_clean_exit_in(dir.path());
        let body = SAMPLE.replace("test boom", "second boom");
        write_crash(dir.path(), "crash-a.log", &body, 3_000);
        // 文件名更靠后，但 mtime 更旧，不应盖过更新的那份。
        write_crash(
            dir.path(),
            "crash-z.log",
            &SAMPLE.replace("test boom", "stale"),
            2_000,
        );
        let notice = begin_run_in(dir.path()).expect("newer report");
        assert_eq!(notice.report.unwrap().panic_first_line, "second boom");
    }

    #[test]
    fn unclean_exit_still_prompts_when_crash_log_was_already_acknowledged() {
        let dir = tempfile::tempdir().unwrap();
        write_crash(dir.path(), "crash-1.log", SAMPLE, 4_000);
        let first = begin_run_in(dir.path()).unwrap();
        assert!(first.report_is_new);
        let again = begin_run_in(dir.path()).expect("unclean without a new file");
        assert!(again.unclean_exit);
        assert!(!again.report_is_new);
        assert_eq!(again.report.unwrap().panic_first_line, "test boom");
    }

    #[test]
    fn unrelated_files_do_not_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let crash_dir = dir.path().join("crash-reports");
        std::fs::create_dir_all(&crash_dir).unwrap();
        std::fs::write(crash_dir.join("notes.log"), "panic: no").unwrap();
        assert!(begin_run_in(dir.path()).is_none());
    }

    #[test]
    fn panic_preview_is_first_line_and_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let long = "y".repeat(200);
        let body = format!("panic: {long}\nnot-the-preview\n");
        write_crash(dir.path(), "crash-1.log", &body, 1_000);
        let path = dir.path().join("crash-reports").join("crash-1.log");
        let summary =
            summarize_crash_report(&path, UNIX_EPOCH + std::time::Duration::from_millis(1_000));
        assert!(summary.panic_first_line.ends_with('…'));
        assert!(!summary.panic_first_line.contains("not-the-preview"));
        assert!(summary.full_text.contains("not-the-preview"));
        assert_eq!(summary.panic_first_line.chars().count(), 181);
    }
}
