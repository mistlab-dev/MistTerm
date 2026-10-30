//! SSH 自动重连调度(FUNCTIONAL_SPEC §1.4)
//!
//! 纯业务规则，不依赖 egui / SSH 句柄；由 UI 每帧传入时间与标签状态。

use std::time::{Duration, Instant};

/// 默认最多自动重连次数(与产品文案一致)
pub const DEFAULT_MAX_RECONNECT_ATTEMPTS: u8 = 5;

/// 单标签的重连计划
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabReconnectSchedule {
    pub next_fire: Option<Instant>,
    pub attempts: u8,
}

/// 写入状态栏的提示(本地化在 UI 层按语言格式化)
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
            TabReconnectSchedule {
                next_fire: None,
                attempts: schedule.attempts,
            },
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

/// 终端区常驻断开条要展示的重连阶段。纯状态，文案在 UI 层本地化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisconnectBannerKind {
    /// 未开启自动重连，或本轮还没排上下一次重试。
    Held,
    /// 自动重连退避中。`delay_secs` 是距离 `next_fire` 的剩余整秒。
    Scheduled {
        attempt: u8,
        max_attempts: u8,
        delay_secs: u64,
    },
    /// 已达次数上限，不再自动重试。
    GaveUp {
        max_attempts: u8,
    },
}

/// 由标签上已有的重连字段派生横幅阶段。达到上限后不再显示倒计时。
pub fn disconnect_banner_kind(
    auto_reconnect_enabled: bool,
    attempts: u8,
    next_fire: Option<Instant>,
    now: Instant,
    max_attempts: u8,
) -> DisconnectBannerKind {
    if auto_reconnect_enabled && attempts > 0 && attempts >= max_attempts {
        return DisconnectBannerKind::GaveUp { max_attempts };
    }
    if auto_reconnect_enabled {
        if let Some(fire) = next_fire {
            if attempts > 0 {
                return DisconnectBannerKind::Scheduled {
                    attempt: attempts,
                    max_attempts,
                    delay_secs: fire.saturating_duration_since(now).as_secs(),
                };
            }
        }
    }
    DisconnectBannerKind::Held
}

// --- #201：窗口/标签回到前台时可选自动重连 ---

/// 前台/标签激活时是否应自动重连（方案 §3）。
///
/// 纯判定，不依赖 egui；由 UI 在两个触发点（viewport focused 边沿、标签切换）收集
/// 候选标签后统一调用。
///
/// - `unexpected_disconnect`：以 `unexpected_disconnect_notified` 信号为准；
///   用户主动断开（`local_disconnect_intent`）路径不会置位该信号，直接排除。
/// - `banner_shown`：断线条可见（离线缓冲 + 未连接 + 非连接中 + 无错误），
///   避免对连接中/报错态误触发。
/// - `attempts` / `max_attempts`：与现有退避共用预算，防止前台反复切换刷重连。
/// - `backoff_pending`：已有退避排程在等时跳过，等退避到点即可。
pub fn should_reconnect_on_activation(
    preference_enabled: bool,
    unexpected_disconnect: bool,
    banner_shown: bool,
    attempts: u8,
    max_attempts: u8,
) -> bool {
    if !preference_enabled {
        return false;
    }
    if !unexpected_disconnect {
        return false;
    }
    if !banner_shown {
        return false;
    }
    // #201 复审：预算未耗尽即可触发；有退避排程时命中则清排程立刻连
    // （「回前台跳过剩余退避」），不再以 backoff_pending 拦截。
    attempts < max_attempts
}

#[cfg(test)]
mod foreground_tests {
    use super::should_reconnect_on_activation;

    const ON: bool = true;
    const OFF: bool = false;

    #[test]
    fn requires_preference_on() {
        // 验收点 1：默认关（偏好关）→ 行为与现状一致，永不触发。
        assert!(!should_reconnect_on_activation(OFF, true, true, 0, 5));
    }

    #[test]
    fn excludes_user_initiated_disconnect() {
        // 验收点 3：用户主动断开（粘性标记未置位）→ 不自动重连。
        assert!(!should_reconnect_on_activation(ON, false, true, 0, 5));
    }

    #[test]
    fn requires_banner_state() {
        // 非断线条展示态（连接中/报错）不触发。
        assert!(!should_reconnect_on_activation(ON, true, false, 0, 5));
    }

    #[test]
    fn respects_attempt_budget() {
        // 预算耗尽（GaveUp）→ 前台切换不刷新重连。
        assert!(!should_reconnect_on_activation(ON, true, true, 5, 5));
        // 预算未耗尽 → 可触发。
        assert!(should_reconnect_on_activation(ON, true, true, 3, 5));
    }

    #[test]
    fn fires_even_when_backoff_pending() {
        // #201 复审修正：有退避排程也触发（命中时由调用方清排程立刻连，
        // 「回前台跳过剩余退避」），不再以 backoff_pending 拦截。
        assert!(should_reconnect_on_activation(ON, true, true, 1, 5));
    }

    #[test]
    fn happy_path() {
        // 验收点 2：偏好开 + 意外断开 + 断线条可见 + 有预算 → 触发。
        assert!(should_reconnect_on_activation(ON, true, true, 0, 5));
    }
}
