//! 崩溃报告（#150）：panic hook 把 panic 信息 + 栈落到本地日志。
//! #189：崩溃对用户可见——正常退出写 stamp，下次启动检测到新崩溃报告时弹提示。
//!
//! 背景：`ctx.set_fonts` 只排队，下一帧 `Fonts::new` 里 ab_glyph 解析失败会
//! epaint `panic!`——Rust panic 走 unwind/abort，macOS 往往不生成 `.ips`，
//! 会话日志也看不到栈。此模块在 `logs/crash-reports/`（与 hang-reports 同级）
//! 留一份可查的报告，命名/头部字段与 hang-reports 对齐。
//!
//! 边界：
//! - hook 内不做任何可能再 panic 的复杂操作；写文件失败则静默退回 stderr；
//! - 保留默认 hook 行为（stderr 输出），落盘是额外动作；
//! - 报告数上限（保留最近 10 份），与现有日志轮转口径一致；
//! - 捕获边界（#189 P1）：Rust `panic!` hook 可落盘；`panic_cannot_unwind` /
//!   ObjC 回调里 panic 进程 abort，hook 多数仍能写盘但无 UI（下次启动提示补上）；
//!   纯原生 SIGSEGV/SIGBUS 当前无本机报告（crashpad 不在本票范围）。
//! - #188：macOS `drawRect:` 外包 `catch_unwind` 后进程可继续；panic hook 仍会先写
//!   `crash-*.log`，另由 [`write_caught_ffi_note`] 标注 caught-in-FFI。
//!   这类「没有新的 crash-*.log」的异常退出靠 `launch_seen`：第一次安装不提示，
//!   之后若本次启动晚于上次 `last_run.ok`，仍弹一次。确认只发生在用户关掉窗口之后
//!   的正常退出（写 stamp），启动检测本身不写确认。

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const MAX_REPORTS: usize = 10;

/// 正常退出标记文件名（位于 logs/ 下）。
pub const CLEAN_EXIT_STAMP: &str = "last_run.ok";
/// 每次 GUI 启动写下的标记。比 `last_run.ok` 新，说明上次启动后没有正常退出。
const LAUNCH_SEEN_NAME: &str = "launch_seen";

static INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn crash_report_dir() -> PathBuf {
    crate::core::default_log_base_dir().join("crash-reports")
}

fn clean_exit_stamp_path() -> PathBuf {
    crate::core::default_log_base_dir().join(CLEAN_EXIT_STAMP)
}

/// 正常退出时写 stamp（#189）。尽力而为：写失败不影响退出流程。
pub fn mark_clean_exit() {
    let path = clean_exit_stamp_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    write_clean_exit_stamp_at(&path);
}

fn write_clean_exit_stamp_at(path: &std::path::Path) {
    let body = format!("clean exit: {}\n", chrono::Local::now().to_rfc3339());
    let _ = std::fs::write(path, body);
}

/// 启动时检测上次运行是否出现过崩溃：
/// 命中 = 存在 mtime 比 clean-exit stamp 更新的 crash-*.log（或 stamp 缺失且有报告）。
/// 无任何 crash 报告时返回 None。没有报告的异常退出由 [`take_startup_notice`] 另判。
pub fn detect_unreported_crash() -> Option<CrashNotice> {
    detect_unreported_crash_in(&crash_report_dir(), &clean_exit_stamp_path())
}

/// GUI 启动时调用一次。先沿用 #190 的报告判定，再补「没有新 crash-*.log 的异常退出」。
///
/// 不在这里写确认：用户还没看见对话框。关掉窗口只影响本进程；下次是否再提示
/// 取决于之后有没有 `mark_clean_exit`。
pub fn take_startup_notice() -> Option<CrashNotice> {
    let base = crate::core::default_log_base_dir();
    let _ = std::fs::create_dir_all(&base);
    take_startup_notice_in(&base)
}

fn take_startup_notice_in(log_base: &std::path::Path) -> Option<CrashNotice> {
    let _ = std::fs::create_dir_all(log_base);
    let seen_path = log_base.join(LAUNCH_SEEN_NAME);
    let stamp_path = log_base.join(CLEAN_EXIT_STAMP);
    let seen_mtime = file_mtime(&seen_path);
    let stamp_mtime = file_mtime(&stamp_path);
    let crash = detect_unreported_crash_in(&log_base.join("crash-reports"), &stamp_path);
    // 第一次安装没有 launch_seen，不能把「从未跑过」当成异常退出。
    let unclean_without_new_report = seen_mtime.is_some()
        && match (seen_mtime, stamp_mtime) {
            (Some(seen), Some(stamp)) => seen > stamp,
            (Some(_), None) => true,
            _ => false,
        };
    let notice = if crash.is_some() {
        crash
    } else if unclean_without_new_report {
        Some(CrashNotice::unclean_without_report())
    } else {
        None
    };
    if let Err(e) = std::fs::write(&seen_path, chrono::Local::now().to_rfc3339()) {
        log::warn!("failed to record launch marker: {e}");
    }
    notice
}

fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 可注入版本（测试用临时目录）。
fn detect_unreported_crash_in(
    report_dir: &std::path::Path,
    stamp_path: &std::path::Path,
) -> Option<CrashNotice> {
    let latest = latest_report_mtime(report_dir)?;
    let stamp = std::fs::metadata(stamp_path)
        .and_then(|m| m.modified())
        .ok();
    // stamp 缺失 = 上次未走到正常退出（或首次运行）；mtime 晚于 stamp = 崩溃发生在上次退出之后。
    let suspicious = match stamp {
        Some(s) => latest > s,
        None => true,
    };
    if !suspicious {
        return None;
    }
    CrashNotice::from_newest_report(report_dir)
}

fn latest_report_mtime(dir: &std::path::Path) -> Option<std::time::SystemTime> {
    crash_report_files(dir).into_iter().map(|(t, _)| t).max()
}

/// 列出 `crash-*` 报告。某一个目录项读 metadata 失败或不是文件时跳过，不影响其余文件。
fn crash_report_files(dir: &std::path::Path) -> Vec<(std::time::SystemTime, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("crash-") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        files.push((modified, entry.path()));
    }
    files
}

/// 启动提示所需的最小信息（#189）：版本 / 时间 / panic 首行 + 全文。
#[derive(Debug, Clone, PartialEq)]
pub struct CrashNotice {
    pub version: String,
    pub time: String,
    pub panic_summary: String,
    pub full_text: String,
    pub file_name: String,
    /// `false`：上次异常退出，但没有新的 crash-*.log（例如 kill -9 / SIGSEGV）。
    pub has_report: bool,
}

impl CrashNotice {
    /// 没有新报告时的提示。不带可复制的全文。
    pub fn unclean_without_report() -> Self {
        Self {
            version: crate::platform::APP_VERSION.to_string(),
            time: String::new(),
            panic_summary: String::new(),
            full_text: String::new(),
            file_name: String::new(),
            has_report: false,
        }
    }

    /// 解析最新的 crash-*.log；解析失败时退化为只展示文件名。
    pub fn from_newest_report(dir: &std::path::Path) -> Option<Self> {
        let mut files = crash_report_files(dir);
        if files.is_empty() {
            return None;
        }
        files.sort_by(|a, b| b.0.cmp(&a.0)); // 新→旧
        let (ts, path) = files.first()?;
        let full_text = std::fs::read_to_string(path).unwrap_or_default();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let file_time = chrono::DateTime::<chrono::Local>::from(*ts).to_rfc3339();
        let mut time = file_time;
        let mut panic_summary = String::new();
        let mut version = String::new();
        for line in full_text.lines() {
            if let Some(v) = line.strip_prefix("version: ") {
                version = v.to_string();
            } else if let Some(t) = line.strip_prefix("time: ") {
                time = t.to_string();
            } else if let Some(p) = line.strip_prefix("panic: ") {
                panic_summary = p.chars().take(200).collect();
                break;
            }
        }
        if panic_summary.is_empty() {
            panic_summary = format!("({file_name})");
        }
        Some(Self {
            version,
            time,
            panic_summary,
            full_text,
            file_name,
            has_report: true,
        })
    }
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

/// #188：FFI 边界 `catch_unwind` 之后补一条 caught-in-FFI 标注（进程已继续）。
///
/// panic hook 通常已写过带 backtrace 的 `crash-*.log`；本函数再写一份短注，
/// 方便和「未捕获 abort」区分。失败静默。
pub fn write_caught_ffi_note(context: &str) {
    let body = format!(
        "MistTerm crash report (caught-in-FFI, process continued)\n=====================\n\
         version: {}\nos: {} {}\n\
         time: {}\n\
         thread: {}\n\
         context: {}\n\
         note: panic was caught at ObjC FFI boundary; sibling crash-*.log from the panic hook holds the backtrace.\n",
        crate::platform::APP_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
        chrono::Local::now().to_rfc3339(),
        std::thread::current().name().unwrap_or("<unnamed>"),
        context,
    );
    if let Ok(dir) = ensure_crash_report_dir() {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
        let path = dir.join(format!("crash-{ts}-caught-ffi.log"));
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
    fn write_caught_ffi_note_does_not_panic() {
        // Writes under the real logs/crash-reports dir (best-effort); must not panic.
        write_caught_ffi_note("drawRect:#188-test");
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

    // ---- #189：clean-exit stamp / 启动检测（纯文件逻辑，用临时目录验证） ----

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mistterm-crash189-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_report(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    fn sample_report() -> String {
        format!(
            "MistTerm crash report\n=====================\nversion: {}\nos: test\n\
             time: 2026-10-29T00:00:00+08:00\nthread: main\npanic: test panic boom\n\
             location: src/x.rs:1:1\n\nbacktrace:\n(none)\n",
            crate::platform::APP_VERSION
        )
    }

    #[test]
    fn no_reports_means_no_notice() {
        let d = temp_dir("empty");
        assert!(detect_unreported_crash_in(&d, &d.join("stamp")).is_none());
    }

    #[test]
    fn crash_newer_than_stamp_is_detected() {
        let d = temp_dir("newer");
        let stamp = d.join("stamp");
        std::fs::write(&stamp, b"ok").unwrap();
        // stamp 先写，报告后写 → 报告 mtime 更新
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_report(&d, "crash-1.log", &sample_report());
        let n = detect_unreported_crash_in(&d, &stamp).expect("should detect");
        assert_eq!(n.panic_summary, "test panic boom");
        assert!(!n.version.is_empty());
        assert!(n.full_text.contains("backtrace"));
        assert!(n.has_report);
    }

    #[test]
    fn crash_older_than_stamp_is_silent() {
        let d = temp_dir("older");
        write_report(&d, "crash-1.log", &sample_report());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let stamp = d.join("stamp");
        std::fs::write(&stamp, b"ok").unwrap(); // stamp 更新 → 已提示过
        assert!(detect_unreported_crash_in(&d, &stamp).is_none());
    }

    #[test]
    fn missing_stamp_with_crash_is_detected() {
        let d = temp_dir("nostamp");
        write_report(&d, "crash-1.log", &sample_report());
        let n = detect_unreported_crash_in(&d, &d.join("missing.stamp")).expect("should detect");
        assert_eq!(n.panic_summary, "test panic boom");
    }

    #[test]
    fn non_crash_files_are_ignored() {
        let d = temp_dir("other");
        write_report(&d, "hang-1.log", "unrelated");
        assert!(detect_unreported_crash_in(&d, &d.join("missing")).is_none());
    }

    #[test]
    fn notice_parses_version_time_summary() {
        let d = temp_dir("parse");
        write_report(&d, "crash-x.log", &sample_report());
        let n = CrashNotice::from_newest_report(&d).unwrap();
        assert_eq!(n.version, crate::platform::APP_VERSION);
        assert_eq!(n.time, "2026-10-29T00:00:00+08:00");
        assert_eq!(n.panic_summary, "test panic boom");
        assert_eq!(n.file_name, "crash-x.log");
    }

    #[test]
    fn notice_falls_back_to_file_name_when_malformed() {
        let d = temp_dir("malformed");
        write_report(&d, "crash-bad.log", "not a report");
        let n = CrashNotice::from_newest_report(&d).unwrap();
        assert!(n.panic_summary.contains("crash-bad.log"));
    }

    #[test]
    fn clean_exit_stamp_roundtrip() {
        let d = temp_dir("stamp");
        let stamp = d.join("last_run.ok");
        write_clean_exit_stamp_at(&stamp);
        assert!(stamp.exists());
        assert!(std::fs::read_to_string(&stamp)
            .unwrap()
            .starts_with("clean exit:"));
    }

    #[test]
    fn directory_named_like_a_report_does_not_hide_a_real_log() {
        let d = temp_dir("badent");
        std::fs::create_dir(d.join("crash-not-a-file")).unwrap();
        write_report(&d, "crash-ok.log", &sample_report());
        let n = CrashNotice::from_newest_report(&d).unwrap();
        assert_eq!(n.file_name, "crash-ok.log");
        assert!(n.has_report);
    }

    #[test]
    fn first_launch_without_report_does_not_prompt() {
        let d = temp_dir("first");
        assert!(take_startup_notice_in(&d).is_none());
        assert!(d.join("launch_seen").is_file());
        assert!(!d.join(CLEAN_EXIT_STAMP).exists());
    }

    #[test]
    fn second_start_without_clean_exit_prompts_even_without_a_report() {
        let d = temp_dir("kill");
        assert!(take_startup_notice_in(&d).is_none());
        let n = take_startup_notice_in(&d).expect("unclean without report");
        assert!(!n.has_report);
        assert!(n.full_text.is_empty());
    }

    fn set_mtime(path: &std::path::Path, secs: u64) {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(t).unwrap();
    }

    #[test]
    fn clean_exit_suppresses_the_next_prompt_until_another_unclean_start() {
        let d = temp_dir("clean");
        assert!(take_startup_notice_in(&d).is_none());
        set_mtime(&d.join("launch_seen"), 1_000);
        write_clean_exit_stamp_at(&d.join(CLEAN_EXIT_STAMP));
        set_mtime(&d.join(CLEAN_EXIT_STAMP), 2_000);
        assert!(
            take_startup_notice_in(&d).is_none(),
            "stamp newer than the previous launch"
        );
        let again = take_startup_notice_in(&d).expect("seen newer than stamp");
        assert!(!again.has_report);
    }

    #[test]
    fn crash_report_wins_over_the_no_report_message() {
        let d = temp_dir("prefer");
        assert!(take_startup_notice_in(&d).is_none());
        let reports = d.join("crash-reports");
        std::fs::create_dir_all(&reports).unwrap();
        write_report(&reports, "crash-1.log", &sample_report());
        let n = take_startup_notice_in(&d).expect("report");
        assert!(n.has_report);
        assert_eq!(n.panic_summary, "test panic boom");
    }
}
