//! SSH 自动重连调度（FUNCTIONAL_SPEC §1.4）
//!
//! 纯业务规则，不依赖 egui / SSH 句柄；由 UI 每帧传入时间与标签状态。

use std::time::{Duration, Instant};

/// 默认最多自动重连次数（与产品文案一致）
pub const DEFAULT_MAX_RECONNECT_ATTEMPTS: u8 = 5;

/// 单标签的重连计划
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabReconnectSchedule {
    pub next_fire: Option<Instant>,
    pub attempts: u8,
}

/// 写入状态栏的提示（本地化在 UI 层按语言格式化）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconnectStatus {
    /// 已达自动重连次数上限，不再重试
    GaveUp {
        max_attempts: u8,
    },
    /// 安排了下一次指数退避重连
    Scheduled {
        delay_secs: u64,
        attempt: u8,
        max_attempts: u8,
    },
}

/// 收集本帧应触发 `reconnect_tab` 的标签下标
pub fn collect_tabs_due_for_reconnect(
    schedules: &[TabReconnectSchedule],
    now: Instant,
    auto_reconnect_enabled: bool,
) -> Vec<usize> {
    if !auto_reconnect_enabled {
        return Vec::new();
    }
    schedules
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.next_fire
                .filter(|t| now >= *t)
                .map(|_| i)
        })
        .collect()
}

/// 意外断线后安排下一次重连；返回更新后的计划与可选状态文案
pub fn schedule_after_unexpected_disconnect(
    schedule: TabReconnectSchedule,
    max_attempts: u8,
    now: Instant,
) -> (TabReconnectSchedule, Option<ReconnectStatus>) {
    if schedule.attempts >= max_attempts {
        return (
            schedule,
            Some(ReconnectStatus::GaveUp {
                max_attempts,
            }),
        );
    }
    let exp = schedule.attempts.min(4);
    let delay = Duration::from_secs(1u64 << exp);
    let attempts = schedule.attempts + 1;
    (
        TabReconnectSchedule {
            next_fire: Some(now + delay),
            attempts,
        },
        Some(ReconnectStatus::Scheduled {
            delay_secs: delay.as_secs(),
            attempt: attempts,
            max_attempts,
        }),
    )
}

#[inline]
pub fn cleared_schedule() -> TabReconnectSchedule {
    TabReconnectSchedule::default()
}
