//! 监控告警推送偏好与上升沿判定（纯逻辑，无 GUI）。

use serde::{Deserialize, Serialize};

/// 告警推送三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlertNotifyMode {
    /// 系统通知（默认）。
    #[default]
    SystemNotify,
    /// 声音。
    Sound,
    /// 仅面板内红框。
    PanelOnly,
}

impl AlertNotifyMode {
    pub fn as_label_en(self) -> &'static str {
        match self {
            Self::SystemNotify => "System notification",
            Self::Sound => "Sound",
            Self::PanelOnly => "Panel only",
        }
    }

    pub fn as_label_zh(self) -> &'static str {
        match self {
            Self::SystemNotify => "系统通知",
            Self::Sound => "声音",
            Self::PanelOnly => "仅面板内",
        }
    }

    pub const ALL: [AlertNotifyMode; 3] = [
        AlertNotifyMode::SystemNotify,
        AlertNotifyMode::Sound,
        AlertNotifyMode::PanelOnly,
    ];
}

/// 监控告警推送设置（持久化到 AppSettings）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorAlertSettings {
    #[serde(default)]
    pub notify_mode: AlertNotifyMode,
    /// 首次系统权限说明已关掉。
    #[serde(default)]
    pub permission_hint_dismissed: bool,
    /// 上次系统通知被拒后，已向用户解释过降级原因。
    #[serde(default)]
    pub permission_denied_explained: bool,
}

impl Default for MonitorAlertSettings {
    fn default() -> Self {
        Self {
            notify_mode: AlertNotifyMode::SystemNotify,
            permission_hint_dismissed: false,
            permission_denied_explained: false,
        }
    }
}

/// 上升沿：相对上一批指纹，新出现的告警文案。
pub fn rising_alert_lines(previous: &[String], current: &[String]) -> Vec<String> {
    current
        .iter()
        .filter(|line| !previous.iter().any(|p| p == *line))
        .cloned()
        .collect()
}

/// 根据模式决定推送动作（纯决策，不含 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDispatch {
    /// 不推系统/声音（仅面板）。
    PanelOnly,
    /// 播放声音。
    PlaySound,
    /// 尝试系统通知。
    TrySystemNotify,
}

pub fn dispatch_for_mode(mode: AlertNotifyMode) -> AlertDispatch {
    match mode {
        AlertNotifyMode::PanelOnly => AlertDispatch::PanelOnly,
        AlertNotifyMode::Sound => AlertDispatch::PlaySound,
        AlertNotifyMode::SystemNotify => AlertDispatch::TrySystemNotify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_system_notify() {
        let s = MonitorAlertSettings::default();
        assert_eq!(s.notify_mode, AlertNotifyMode::SystemNotify);
        assert!(!s.permission_hint_dismissed);
    }

    #[test]
    fn rising_edge_detects_new_lines_only() {
        let prev = vec!["CPU 90% ≥ 阈值 80%".to_string()];
        let curr = vec![
            "CPU 90% ≥ 阈值 80%".to_string(),
            "磁盘 88.5% ≥ 阈值 85%".to_string(),
        ];
        let risen = rising_alert_lines(&prev, &curr);
        assert_eq!(risen, vec!["磁盘 88.5% ≥ 阈值 85%".to_string()]);
    }

    #[test]
    fn rising_edge_empty_when_unchanged() {
        let lines = vec!["磁盘 88.5% ≥ 阈值 85%".to_string()];
        assert!(rising_alert_lines(&lines, &lines).is_empty());
    }

    #[test]
    fn rising_edge_all_new_from_empty() {
        let curr = vec!["a".into(), "b".into()];
        assert_eq!(rising_alert_lines(&[], &curr), curr);
    }

    #[test]
    fn dispatch_matches_mode() {
        assert_eq!(
            dispatch_for_mode(AlertNotifyMode::PanelOnly),
            AlertDispatch::PanelOnly
        );
        assert_eq!(
            dispatch_for_mode(AlertNotifyMode::Sound),
            AlertDispatch::PlaySound
        );
        assert_eq!(
            dispatch_for_mode(AlertNotifyMode::SystemNotify),
            AlertDispatch::TrySystemNotify
        );
    }

    #[test]
    fn serde_roundtrip_defaults() {
        let raw = "{}";
        let s: MonitorAlertSettings = serde_json::from_str(raw).unwrap();
        assert_eq!(s, MonitorAlertSettings::default());
        let json = serde_json::to_string(&s).unwrap();
        let back: MonitorAlertSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }
}
