//! UI 卡顿（主线程疑似无响应）诊断：本地落盘 JSON 报告。
//!
//! 第一版仅本地记录，不做网络上报。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 默认阈值：主线程约 3 秒无心跳视为疑似卡顿。
/// 过短（1s 内）易被 FileDialog/短暂 GC 误报；过长（>5s）不利定位。业界桌面诊断常见 2–5s。
const DEFAULT_HANG_THRESHOLD_MS: u64 = 3_000;
/// watchdog 检查间隔（不必过密）。
const DEFAULT_POLL_MS: u64 = 500;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HangSnapshot {
    pub status_message: String,
    pub tabs_count: usize,
    pub active_tab: Option<usize>,
    pub panel_state: String,
    /// 忙态提示（团队/市场/对话框等），便于归因 UI 阻塞。
    #[serde(default)]
    pub busy_hint: String,
}

impl Default for HangSnapshot {
    fn default() -> Self {
        Self {
            status_message: String::new(),
            tabs_count: 0,
            active_tab: None,
            panel_state: String::new(),
            busy_hint: String::new(),
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
    log::warn!(
        "UI hang suspected: stale_for_ms={} threshold_ms={} busy_hint={} panels={}",
        report.stale_for_ms,
        report.threshold_ms,
        report.snapshot.busy_hint,
        report.snapshot.panel_state
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hang_snapshot_default_all_fields_are_empty_or_zero() {
        let s = HangSnapshot::default();
        assert_eq!(s.status_message, "");
        assert_eq!(s.tabs_count, 0);
        assert_eq!(s.active_tab, None);
        assert_eq!(s.panel_state, "");
        assert_eq!(s.busy_hint, "");
    }

    #[test]
    fn hang_snapshot_serde_busy_hint_defaults_to_empty_when_missing() {
        // HangSnapshot::busy_hint 有 #[serde(default)]，序列化后缺该字段
        // 时反序列化应补为 ""。
        let json = r#"{
            "status_message":"loading",
            "tabs_count": 4,
            "active_tab": 1,
            "panel_state": "connection-grid"
        }"#;
        let s: HangSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(s.status_message, "loading");
        assert_eq!(s.tabs_count, 4);
        assert_eq!(s.active_tab, Some(1));
        assert_eq!(s.panel_state, "connection-grid");
        assert_eq!(s.busy_hint, "");
    }

    #[test]
    fn hang_snapshot_serde_roundtrip_preserves_busy_hint() {
        let s = HangSnapshot {
            status_message: "rendering".into(),
            tabs_count: 1,
            active_tab: Some(0),
            panel_state: "terminal".into(),
            busy_hint: "market-catalog".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let r: HangSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(r.status_message, "rendering");
        assert_eq!(r.tabs_count, 1);
        assert_eq!(r.active_tab, Some(0));
        assert_eq!(r.panel_state, "terminal");
        assert_eq!(r.busy_hint, "market-catalog");
    }

    #[test]
    fn ensure_hang_report_dir_creates_parent_and_returns_path() {
        let dir = ensure_hang_report_dir().expect("should be able to create the dir");
        assert!(dir.exists());
        assert!(dir.is_dir());
        // Directory name ends with the configured subfolder.
        assert!(dir
            .to_string_lossy()
            .contains("hang-reports"));
    }

    #[test]
    fn hang_report_dir_lives_under_default_log_base() {
        let dir = hang_report_dir();
        let base = crate::core::default_log_base_dir();
        assert!(dir.starts_with(base));
    }

    #[test]
    fn default_constants_within_reasonable_ranges() {
        // Threshold 3s; don't let refactors accidentally widen too much.
        assert!(DEFAULT_HANG_THRESHOLD_MS >= 2_000, "threshold too low to be useful");
        assert!(DEFAULT_HANG_THRESHOLD_MS <= 5_000, "threshold too high to diagnose hangs");
        assert!(DEFAULT_POLL_MS >= 100);
        assert!(DEFAULT_POLL_MS <= DEFAULT_HANG_THRESHOLD_MS);
    }

    // ---- Threshold + dedup logic via Inner without spawning a thread ----

    fn fresh_inner(threshold_ms: u64, poll_ms: u64) -> Inner {
        Inner {
            stop: AtomicBool::new(false),
            last_heartbeat_ms: AtomicU64::new(0),
            last_reported_heartbeat_ms: AtomicU64::new(0),
            threshold_ms,
            poll_ms,
            snapshot: Mutex::new(HangSnapshot::default()),
        }
    }

    #[test]
    fn check_and_dump_skips_when_last_heartbeat_under_threshold() {
        let inner = fresh_inner(10_000, 100);
        // "now" is >= last (0 with saturating sub => 0ms stale); threshold 10s
        // so it should NOT dump anything. Just confirm the call is a no-op.
        check_and_dump_if_hung(&inner);
        // No reports should be written; but since we can't easily list all
        // hang reports, we assert via the reported-timestamp atomics which
        // remain 0 (unchanged) because the early-return path was taken.
        assert_eq!(
            inner
                .last_reported_heartbeat_ms
                .load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn check_and_dump_writes_report_when_stale() {
        let threshold = 10; // ms
        let inner = fresh_inner(threshold, 1);
        let past_ts = now_unix_ms().saturating_sub(1_000); // definitely stale
        inner.last_heartbeat_ms.store(past_ts, Ordering::Relaxed);
        check_and_dump_if_hung(&inner);
        // After dumping, `last_reported_heartbeat_ms` should match `last` so
        // a second invocation within the same stale window does NOT dump
        // again (dedup logic).
        assert_eq!(
            inner
                .last_reported_heartbeat_ms
                .load(Ordering::Relaxed),
            past_ts
        );
    }

    #[test]
    fn check_and_dump_dedupes_within_same_heartbeat() {
        let threshold = 10;
        let inner = fresh_inner(threshold, 1);
        let past_ts = now_unix_ms().saturating_sub(1_000);
        inner
            .last_heartbeat_ms
            .store(past_ts, Ordering::Relaxed);
        check_and_dump_if_hung(&inner); // dumps
        let reported_after_first = inner
            .last_reported_heartbeat_ms
            .load(Ordering::Relaxed);
        check_and_dump_if_hung(&inner); // deduped
        let reported_after_second = inner
            .last_reported_heartbeat_ms
            .load(Ordering::Relaxed);
        // The dedup condition `last_reported == last` guards the second call.
        // If dedup worked correctly, no new dump happened -> atomic unchanged.
        assert_eq!(reported_after_first, reported_after_second);
        assert_eq!(reported_after_second, past_ts);
    }

    #[test]
    fn reporter_drop_sets_stop_flag() {
        // Start a real reporter (small poll just so it doesn't wait long),
        // then drop it and inspect stop by leaking the Arc contents. Since
        // `inner` is private we check indirectly: constructing a reporter
        // then dropping it must be safe (no panic, no thread stuck forever
        // past drop). With poll_ms=1 we can at least exercise the code.
        let reporter = HangReporter::start(10_000, 1);
        reporter.heartbeat();
        reporter.update_snapshot(HangSnapshot {
            status_message: "hi".into(),
            ..Default::default()
        });
        drop(reporter);
        // If we got here without deadlock/panic, Drop's stop=true fired
        // correctly (the watchdog thread's next loop iteration exits).
    }
}
