//! Unit tests for reconnect
//!
//! Tests SSH auto-reconnect scheduling logic.

use mistterm::core::reconnect::*;
use std::time::{Duration, Instant};

fn make_schedule(next_fire: Option<Instant>, attempts: u8) -> TabReconnectSchedule {
    TabReconnectSchedule { next_fire, attempts }
}

fn future_from_now(secs: u64) -> Instant {
    Instant::now() + Duration::from_secs(secs)
}

#[test]
fn cleared_schedule_has_no_next_fire() {
    let schedule = cleared_schedule();
    assert!(schedule.next_fire.is_none());
    assert_eq!(schedule.attempts, 0);
}

#[test]
fn collect_tabs_due_disabled_returns_empty() {
    let schedules = vec![
        make_schedule(Some(future_from_now(10)), 1),
        make_schedule(Some(future_from_now(20)), 2),
    ];
    let now = Instant::now();
    let result = collect_tabs_due_for_reconnect(&schedules, now, false);
    assert!(result.is_empty());
}

#[test]
fn collect_tabs_due_filters_future_fires() {
    let now = Instant::now();
    let schedules = vec![
        make_schedule(Some(now + Duration::from_secs(10)), 1),
        make_schedule(Some(now + Duration::from_secs(20)), 2),
    ];
    let result = collect_tabs_due_for_reconnect(&schedules, now, true);
    assert!(result.is_empty());
}

#[test]
fn collect_tabs_due_includes_past_fires() {
    let schedules = vec![
        make_schedule(Some(future_from_now(10)), 1),
        make_schedule(Some(future_from_now(20)), 2),
    ];
    let now = Instant::now() + Duration::from_secs(15);
    let result = collect_tabs_due_for_reconnect(&schedules, now, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], 0);
}

#[test]
fn collect_tabs_due_multiple_ready() {
    let schedules = vec![
        make_schedule(Some(future_from_now(10)), 1),
        make_schedule(Some(Instant::now() - Duration::from_secs(1)), 1),
        make_schedule(Some(future_from_now(20)), 2),
    ];
    let now = Instant::now();
    let result = collect_tabs_due_for_reconnect(&schedules, now, true);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], 1);
}

#[test]
fn schedule_after_unexpected_disconnect_gives_up() {
    let schedule = make_schedule(None, 5);
    let now = Instant::now();
    let max_attempts: u8 = 5;

    let (updated, status) = schedule_after_unexpected_disconnect(schedule, max_attempts, now);

    assert_eq!(updated.attempts, 5);
    assert!(updated.next_fire.is_none());
    assert!(matches!(status, Some(ReconnectStatus::GaveUp { max_attempts: 5 })));
}

#[test]
fn schedule_after_unexpected_disconnect_gave_up_clears_pending_fire() {
    let schedule = make_schedule(Some(future_from_now(30)), 5);
    let (updated, status) = schedule_after_unexpected_disconnect(schedule, 5, Instant::now());
    assert!(updated.next_fire.is_none());
    assert_eq!(updated.attempts, 5);
    assert!(matches!(status, Some(ReconnectStatus::GaveUp { max_attempts: 5 })));
}

#[test]
fn disconnect_banner_kind_tracks_schedule_and_gave_up() {
    let now = Instant::now();
    assert_eq!(
        disconnect_banner_kind(false, 2, Some(future_from_now(4)), now, 5),
        DisconnectBannerKind::Held
    );
    assert_eq!(
        disconnect_banner_kind(true, 0, None, now, 5),
        DisconnectBannerKind::Held
    );
    match disconnect_banner_kind(true, 2, Some(now + Duration::from_secs(4)), now, 5) {
        DisconnectBannerKind::Scheduled {
            attempt,
            max_attempts,
            delay_secs,
        } => {
            assert_eq!(attempt, 2);
            assert_eq!(max_attempts, 5);
            assert_eq!(delay_secs, 4);
        }
        other => panic!("expected Scheduled, got {other:?}"),
    }
    assert_eq!(
        disconnect_banner_kind(true, 5, Some(future_from_now(9)), now, 5),
        DisconnectBannerKind::GaveUp { max_attempts: 5 }
    );
}

#[test]
fn schedule_after_unexpected_disconnect_schedules_next() {
    let schedule = make_schedule(None, 0);
    let now = Instant::now();
    let max_attempts: u8 = 5;

    let (updated, status) = schedule_after_unexpected_disconnect(schedule, max_attempts, now);

    assert_eq!(updated.attempts, 1);
    assert!(updated.next_fire.is_some());
    match status {
        Some(ReconnectStatus::Scheduled { delay_secs, attempt, max_attempts: ma }) => {
            assert_eq!(delay_secs, 1);
            assert_eq!(attempt, 1);
            assert_eq!(ma, 5);
        }
        _ => panic!("Expected Scheduled status"),
    }
}

#[test]
fn schedule_after_unexpected_disconnect_exponential_backoff() {
    let schedule0 = make_schedule(None, 0);
    let schedule1 = make_schedule(None, 1);
    let schedule2 = make_schedule(None, 2);
    let schedule3 = make_schedule(None, 3);
    let _schedule4 = make_schedule(None, 4);
    let now = Instant::now();
    let max_attempts: u8 = 5;

    let (_, s0) = schedule_after_unexpected_disconnect(schedule0, max_attempts, now);
    let (_, s1) = schedule_after_unexpected_disconnect(schedule1, max_attempts, now);
    let (_, s2) = schedule_after_unexpected_disconnect(schedule2, max_attempts, now);
    let (_, s3) = schedule_after_unexpected_disconnect(schedule3, max_attempts, now);

    if let Some(ReconnectStatus::Scheduled { delay_secs: d0, .. }) = s0 {
        assert_eq!(d0, 1);
    }
    if let Some(ReconnectStatus::Scheduled { delay_secs: d1, .. }) = s1 {
        assert_eq!(d1, 2);
    }
    if let Some(ReconnectStatus::Scheduled { delay_secs: d2, .. }) = s2 {
        assert_eq!(d2, 4);
    }
    if let Some(ReconnectStatus::Scheduled { delay_secs: d3, .. }) = s3 {
        assert_eq!(d3, 8);
    }
}

#[test]
fn tab_reconnect_schedule_debug() {
    let schedule = make_schedule(None, 0);
    let debug_str = format!("{:?}", schedule);
    assert!(debug_str.contains("TabReconnectSchedule"));
}

#[test]
fn reconnect_status_debug() {
    let status = ReconnectStatus::GaveUp { max_attempts: 5 };
    let debug_str = format!("{:?}", status);
    assert!(debug_str.contains("GaveUp"));
}