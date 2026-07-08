//! UI 卡顿（主线程疑似无响应）诊断：本地落盘 JSON 报告。
//!
//! 第一版仅本地记录，不做网络上报。

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 默认阈值：主线程 5 秒无心跳，判定为一次疑似卡顿。
const DEFAULT_HANG_THRESHOLD_MS: u64 = 5_000;
/// watchdog 检查间隔。
const DEFAULT_POLL_MS: u64 = 1_000;

#[derive(Clone, Debug, Serialize)]
pub struct HangSnapshot {
    pub status_message: String,
    pub tabs_count: usize,
    pub active_tab: Option<usize>,
    pub panel_state: String,
}

impl Default for HangSnapshot {
    fn default() -> Self {
        Self {
            status_message: String::new(),
            tabs_count: 0,
            active_tab: None,
            panel_state: String::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct HangReport {
    event: &'static str,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    timestamp_unix_ms: u64,
    stale_for_ms: u64,
    threshold_ms: u64,
    snapshot: HangSnapshot,
}

/// UI 卡顿本地报告器（watchdog 线程 + 心跳 + 快照）。
pub struct HangReporter {
    inner: Arc<Inner>,
}

struct Inner {
    stop: AtomicBool,
    last_heartbeat_ms: AtomicU64,
    last_reported_heartbeat_ms: AtomicU64,
    threshold_ms: u64,
    poll_ms: u64,
    snapshot: Mutex<HangSnapshot>,
}

impl HangReporter {
    pub fn start_default() -> Self {
        Self::start(DEFAULT_HANG_THRESHOLD_MS, DEFAULT_POLL_MS)
    }

    pub fn start(threshold_ms: u64, poll_ms: u64) -> Self {
        let now = now_unix_ms();
        let inner = Arc::new(Inner {
            stop: AtomicBool::new(false),
            last_heartbeat_ms: AtomicU64::new(now),
            last_reported_heartbeat_ms: AtomicU64::new(0),
            threshold_ms,
            poll_ms,
            snapshot: Mutex::new(HangSnapshot::default()),
        });
        spawn_watchdog(inner.clone());
        Self { inner }
    }

    /// 每帧调用：更新当前 UI 快照（轻量文本）。
    pub fn update_snapshot(&self, snapshot: HangSnapshot) {
        if let Ok(mut guard) = self.inner.snapshot.lock() {
            *guard = snapshot;
        }
    }

    /// 每帧调用：刷新主线程心跳时间戳。
    pub fn heartbeat(&self) {
        self.inner
            .last_heartbeat_ms
            .store(now_unix_ms(), Ordering::Relaxed);
    }
}

impl Drop for HangReporter {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::Relaxed);
    }
}

pub fn hang_report_dir() -> PathBuf {
    crate::core::default_log_base_dir().join("hang-reports")
}

pub fn ensure_hang_report_dir() -> Result<PathBuf, String> {
    let dir = hang_report_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| format!("create hang report dir failed ({}): {e}", dir.display()))?;
    Ok(dir)
}

fn spawn_watchdog(inner: Arc<Inner>) {
    let _ = thread::Builder::new()
        .name("mistterm-hang-watchdog".to_string())
        .spawn(move || {
            loop {
                if inner.stop.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(Duration::from_millis(inner.poll_ms));
                check_and_dump_if_hung(&inner);
            }
        });
}

fn check_and_dump_if_hung(inner: &Inner) {
    let now = now_unix_ms();
    let last = inner.last_heartbeat_ms.load(Ordering::Relaxed);
    let stale_for_ms = now.saturating_sub(last);
    if stale_for_ms < inner.threshold_ms {
        return;
    }
    // 同一轮卡顿仅写一次：心跳恢复并再次停滞时才会产生下一条。
    let last_reported = inner.last_reported_heartbeat_ms.load(Ordering::Relaxed);
    if last_reported == last {
        return;
    }
    let snapshot = inner
        .snapshot
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| HangSnapshot::default());
    let report = HangReport {
        event: "ui_hang_suspected",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        timestamp_unix_ms: now,
        stale_for_ms,
        threshold_ms: inner.threshold_ms,
        snapshot,
    };
    let _ = write_report(&report);
    inner
        .last_reported_heartbeat_ms
        .store(last, Ordering::Relaxed);
}

fn write_report(report: &HangReport) -> Result<PathBuf, String> {
    let dir = ensure_hang_report_dir()?;
    let filename = format!("hang-{}.json", report.timestamp_unix_ms);
    let path = dir.join(filename);
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| format!("serialize hang report failed: {e}"))?;
    fs::write(&path, bytes)
        .map_err(|e| format!("write hang report failed ({}): {e}", path.display()))?;
    Ok(path)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
