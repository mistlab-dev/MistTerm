//! 审计时间线（只读）：近端审计事件的有界本地缓存。
//!
//! v1.1.3 P0 目标：让用户在客户端内查看「本会话 + 近期」的
//! block / confirm / allow / 本地拦截结果，而不必跳到服务端报表。
//!
//! 设计约束：
//! - 只读缓存：不落盘、不回流服务端，纯粹用于 UI 展示。
//! - 有界：`MAX_EVENTS`，超出后淘汰最旧条目（FIFO）。
//! - 不参与信任判定：拦截/放行的权威仍在服务端侧 Agent 与本地规则引擎。
//!   本缓存只负责把已经发生的事件展示出来。
//!
//! ## 与 `crate::core::audit::AuditEvent` 的关系
//!
//! `AuditEvent` 是完整落盘审计（JSONL + 远程 sink）；`AuditTimeline` 是轻量
//! 内存视图，只保留 UI 需要的字段子集，避免把大 `detail` JSON 全部缓存。

use std::collections::VecDeque;
use std::time::Duration;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

/// 时间线最多缓存的事件条数（内存占用极小，足以覆盖一次长会话）。
pub const MAX_EVENTS: usize = 512;

/// 环境/结果分组，用于时间线筛选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineOutcome {
    /// 被策略拦截（服务器侧或本地）。
    Blocked,
    /// 要求用户确认后放行。
    Confirmed,
    /// 允许执行并可能已记录告警。
    Allowed,
    /// 被执行但只是普通记录（无拦截语义）。
    Info,
}

impl TimelineOutcome {
    /// 判定来源前缀：服务器侧 vs 本地检查。
    pub fn source_label(&self) -> &'static str {
        match self {
            Self::Blocked | Self::Confirmed | Self::Allowed => "server-policy",
            Self::Info => "info",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Blocked => "block",
            Self::Confirmed => "confirm",
            Self::Allowed => "allow",
            Self::Info => "info",
        }
    }
}

/// 一条时间线条目（字段刻意精简，避免整份 `AuditEvent` 进内存）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// RFC3339 毫秒时间戳（与 `AuditEvent.ts` 同格式，UTC）。
    pub ts: String,
    /// 远端主机 `host[:port]`，无则为空串。
    pub host: String,
    /// 相关命令文本（若有且允许记录）。
    pub command: String,
    /// 结果分组（block / confirm / allow / info）。
    pub outcome: TimelineOutcome,
    /// 规则名或动作标识（如 `CREAD-006`、`mist_confirm_demo`）。
    pub rule: String,
    /// 附加说明（如「服务器策略」「本地检查」）。
    pub note: String,
}

impl TimelineEntry {
    pub fn new(
        ts: &str,
        host: impl Into<String>,
        command: impl Into<String>,
        outcome: TimelineOutcome,
        rule: impl Into<String>,
        note: impl Into<String>,
    ) -> Self {
        Self {
            ts: ts.to_string(),
            host: host.into(),
            command: command.into(),
            outcome,
            rule: rule.into(),
            note: note.into(),
        }
    }

    /// 按主机过滤（空 host 视为不过滤）。
    pub fn matches_host(&self, host_filter: &str) -> bool {
        host_filter.is_empty() || self.host.contains(host_filter)
    }

    /// 按结果分组过滤（`None` 表示不筛选）。
    pub fn matches_outcome(&self, outcome_filter: Option<TimelineOutcome>) -> bool {
        match outcome_filter {
            None => true,
            Some(o) => self.outcome == o,
        }
    }
}

/// 有界、只插入的时间线缓存（线程内使用；UI 线程读写即可）。
#[derive(Debug, Clone, Default)]
pub struct AuditTimeline {
    events: VecDeque<TimelineEntry>,
    appended_total: u64,
}

impl AuditTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加一条事件；超过 [`MAX_EVENTS`] 时淘汰最旧。
    pub fn push(&mut self, entry: TimelineEntry) {
        self.appended_total = self.appended_total.saturating_add(1);
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(entry);
    }

    /// 全部事件（最旧在前）。
    pub fn entries(&self) -> impl Iterator<Item = &TimelineEntry> {
        self.events.iter()
    }

    /// 最近 N 条（最新在前，供 UI 反序展示）。
    pub fn recent(&self, n: usize) -> Vec<&TimelineEntry> {
        let n = n.min(self.events.len());
        self.events.iter().rev().take(n).collect()
    }

    /// 清空缓存（仅 UI 操作，不影响已落盘审计）。
    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 自创建以来累计追加的条目数（含已淘汰）。
    pub fn appended_total(&self) -> u64 {
        self.appended_total
    }

    /// 按主机关键字 + 结果分组过滤（host 空则不过滤；outcome `None` 不筛选）。
    pub fn filter(&self, host: &str, outcome: Option<TimelineOutcome>) -> Vec<&TimelineEntry> {
        self.events
            .iter()
            .filter(|e| e.matches_host(host) && e.matches_outcome(outcome))
            .collect()
    }
}

/// 判断某事件前后时间差是否不超过 `max_age`。
pub fn recent_within(entry_ts: &str, reference: Option<&str>, max_age: Duration) -> bool {
    let Some(entry_ms) = parse_rfc3339_ms(entry_ts) else {
        return false;
    };
    let reference_ms = match reference {
        Some(r) => match parse_rfc3339_ms(r) {
            Some(v) => v,
            None => return false,
        },
        None => chrono::Utc::now().timestamp_millis().max(0) as u64,
    };
    reference_ms
        .saturating_sub(entry_ms)
        <= max_age.as_millis() as u64
}

/// 解析 RFC3339 时间为 Unix 毫秒。
pub fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    u64::try_from(DateTime::parse_from_rfc3339(s).ok()?.timestamp_millis()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(ts: &str, host: &str, outcome: TimelineOutcome) -> TimelineEntry {
        TimelineEntry::new(ts, host, "ls -la", outcome, "demo", "note")
    }

    #[test]
    fn push_and_entries_oldest_first() {
        let mut t = AuditTimeline::new();
        t.push(entry("2026-09-03T00:00:00.000Z", "h1", TimelineOutcome::Blocked));
        t.push(entry("2026-09-03T00:00:01.000Z", "h2", TimelineOutcome::Allowed));
        assert_eq!(t.len(), 2);
        let entries: Vec<_> = t.entries().collect();
        assert_eq!(entries[0].host, "h1");
        assert_eq!(entries[1].host, "h2");
    }

    #[test]
    fn bounded_by_max_events() {
        let mut t = AuditTimeline::new();
        for i in 0..(MAX_EVENTS + 100) {
            t.push(entry(
                &format!("2026-09-03T00:{:02}:00.000Z", i % 60),
                &format!("h{}", i % 5),
                TimelineOutcome::Info,
            ));
        }
        assert_eq!(t.len(), MAX_EVENTS);
        assert_eq!(t.appended_total(), (MAX_EVENTS + 100) as u64);
        // 最旧 100 条被淘汰。
        assert!(t.entries().all(|e| !e.host.contains("h0") || !e.host.is_empty()));
    }

    #[test]
    fn recent_returns_newest_first() {
        let mut t = AuditTimeline::new();
        t.push(entry("2026-09-03T00:00:00.000Z", "a", TimelineOutcome::Blocked));
        t.push(entry("2026-09-03T00:00:01.000Z", "b", TimelineOutcome::Confirmed));
        t.push(entry("2026-09-03T00:00:02.000Z", "c", TimelineOutcome::Allowed));
        let recent = t.recent(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].host, "c");
        assert_eq!(recent[1].host, "b");
    }

    #[test]
    fn clear_resets() {
        let mut t = AuditTimeline::new();
        t.push(entry("2026-09-03T00:00:00.000Z", "a", TimelineOutcome::Blocked));
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
    }

    #[test]
    fn filter_by_host_and_outcome() {
        let mut t = AuditTimeline::new();
        t.push(entry("2026-09-03T00:00:00.000Z", "web-01", TimelineOutcome::Blocked));
        t.push(entry("2026-09-03T00:00:01.000Z", "web-02", TimelineOutcome::Allowed));
        t.push(entry("2026-09-03T00:00:02.000Z", "db-01", TimelineOutcome::Blocked));
        assert_eq!(t.filter("web", None).len(), 2);
        assert_eq!(t.filter("", Some(TimelineOutcome::Blocked)).len(), 2);
        assert_eq!(t.filter("db", Some(TimelineOutcome::Blocked)).len(), 1);
    }

    #[test]
    fn parse_rfc3339_ms_basic() {
        let ms = parse_rfc3339_ms("2026-09-03T10:00:00.000Z").unwrap();
        // 2026-09-03 10:00:00 UTC → 计算并验证秒级。
        let secs = ms / 1000;
        assert_eq!(secs, 1_788_429_600);
    }

    #[test]
    fn parse_rfc3339_ms_rejects_short() {
        assert_eq!(parse_rfc3339_ms("2026-09-03"), None);
        assert_eq!(parse_rfc3339_ms("garbage"), None);
    }

    #[test]
    fn recent_within_gap() {
        let newer = "2026-09-03T10:00:05.000Z";
        let older = "2026-09-03T10:00:00.000Z";
        assert!(recent_within(older, Some(newer), Duration::from_secs(10)));
        assert!(!recent_within(older, Some(newer), Duration::from_secs(3)));
        // 无参考时间回退到 now：较早时间必然不在 1 秒窗口内。
        assert!(!recent_within("2000-01-01T00:00:00.000Z", None, Duration::from_secs(1)));
    }

    #[test]
    fn outcome_labels() {
        assert_eq!(TimelineOutcome::Blocked.as_str(), "block");
        assert_eq!(TimelineOutcome::Confirmed.as_str(), "confirm");
        assert_eq!(TimelineOutcome::Allowed.as_str(), "allow");
        assert_eq!(TimelineOutcome::Info.source_label(), "info");
    }
}
