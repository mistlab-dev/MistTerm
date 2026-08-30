//! 云端同步配置（MVP：本地导出/导入包，个人备份用）。
//! 团队片段正式能力走团队 API，见 `docs/tech/TEAM.md`。

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

/// 用户勾选的同步项（设计文档 §5.2）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncSettings {
    /// 展示用账号提示（未对接服务端时可为空）
    #[serde(default)]
    pub account_hint: String,
    #[serde(default = "default_true")]
    pub sync_sessions: bool,
    #[serde(default = "default_true")]
    pub sync_fragments: bool,
    #[serde(default = "default_true")]
    pub sync_themes: bool,
    #[serde(default = "default_true")]
    pub sync_shortcuts: bool,
    #[serde(default)]
    pub sync_team_config: bool,
    #[serde(default)]
    pub sync_credentials: bool,
    /// 0 = 仅手动同步；>0 为自动间隔（分钟），用于后续定时器
    #[serde(default = "default_frequency")]
    pub frequency_minutes: u32,
    #[serde(default)]
    pub last_sync_unix: Option<i64>,
    #[serde(default)]
    pub last_error: String,
}

fn default_true() -> bool {
    true
}

fn default_frequency() -> u32 {
    5
}

impl Default for CloudSyncSettings {
    fn default() -> Self {
        Self {
            account_hint: String::new(),
            sync_sessions: true,
            sync_fragments: true,
            sync_themes: true,
            sync_shortcuts: true,
            sync_team_config: false,
            sync_credentials: false,
            frequency_minutes: 5,
            last_sync_unix: None,
            last_error: String::new(),
        }
    }
}

impl CloudSyncSettings {
    pub fn config_path() -> PathBuf {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm");
        let _ = fs::create_dir_all(&dir);
        dir.join("cloud_sync.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::config_path();
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        let data = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(&path, data)
    }

    pub fn mark_sync_ok(&mut self) {
        self.last_sync_unix = Some(chrono::Utc::now().timestamp());
        self.last_error.clear();
        let _ = self.save();
    }

    pub fn mark_sync_err(&mut self, msg: impl Into<String>) {
        self.last_error = msg.into();
        let _ = self.save();
    }

    pub fn record_manual_import_ok(&mut self) {
        self.last_sync_unix = Some(chrono::Utc::now().timestamp());
        self.last_error.clear();
        let _ = self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_expected_boolean_mask_and_frequency() {
        let d = CloudSyncSettings::default();
        assert_eq!(d.account_hint, "");
        assert!(d.sync_sessions);
        assert!(d.sync_fragments);
        assert!(d.sync_themes);
        assert!(d.sync_shortcuts);
        assert!(!d.sync_team_config); // conservative: team NOT synced by default
        assert!(!d.sync_credentials); // conservative: credentials NOT synced by default
        assert_eq!(d.frequency_minutes, 5);
        assert_eq!(d.last_sync_unix, None);
        assert_eq!(d.last_error, "");
    }

    #[test]
    fn default_true_fn_always_true() {
        for _ in 0..3 {
            assert!(default_true());
        }
    }

    #[test]
    fn default_frequency_fn_is_5_minutes() {
        assert_eq!(default_frequency(), 5);
    }

    #[test]
    fn serde_roundtrip_preserves_all_fields() {
        let s = CloudSyncSettings {
            account_hint: "u@corp".into(),
            sync_sessions: true,
            sync_fragments: false,
            sync_themes: true,
            sync_shortcuts: false,
            sync_team_config: true,
            sync_credentials: true,
            frequency_minutes: 15,
            last_sync_unix: Some(1_700_000_000),
            last_error: "network timeout".into(),
        };
        let json = serde_json::to_string(&s).expect("serialize");
        let restored: CloudSyncSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.account_hint, "u@corp");
        assert!(restored.sync_sessions);
        assert!(!restored.sync_fragments);
        assert!(restored.sync_themes);
        assert!(!restored.sync_shortcuts);
        assert!(restored.sync_team_config);
        assert!(restored.sync_credentials);
        assert_eq!(restored.frequency_minutes, 15);
        assert_eq!(restored.last_sync_unix, Some(1_700_000_000));
        assert_eq!(restored.last_error, "network timeout");
    }

    #[test]
    fn serde_defaults_apply_when_fields_missing() {
        // Partial JSON; missing fields should fall back to `#[serde(default)]` and
        // `#[serde(default = "...")]` annotations, matching `Default::default()`
        // for those fields.
        let json = r#"{"account_hint": "only"}"#;
        let s: CloudSyncSettings = serde_json::from_str(json).expect("deserialize");
        assert_eq!(s.account_hint, "only");
        assert!(s.sync_sessions);   // default_true
        assert!(s.sync_fragments);  // default_true
        assert!(s.sync_themes);     // default_true
        assert!(s.sync_shortcuts);  // default_true
        assert!(!s.sync_team_config); // #[serde(default)] -> false
        assert!(!s.sync_credentials); // #[serde(default)] -> false
        assert_eq!(s.frequency_minutes, 5); // default_frequency
        assert_eq!(s.last_sync_unix, None);
        assert_eq!(s.last_error, "");
    }

    #[test]
    fn serde_empty_object_fills_all_defaults() {
        let s: CloudSyncSettings = serde_json::from_str("{}").expect("deserialize");
        let d = CloudSyncSettings::default();
        assert_eq!(s.account_hint, d.account_hint);
        assert_eq!(s.sync_sessions, d.sync_sessions);
        assert_eq!(s.sync_fragments, d.sync_fragments);
        assert_eq!(s.sync_themes, d.sync_themes);
        assert_eq!(s.sync_shortcuts, d.sync_shortcuts);
        assert_eq!(s.sync_team_config, d.sync_team_config);
        assert_eq!(s.sync_credentials, d.sync_credentials);
        assert_eq!(s.frequency_minutes, d.frequency_minutes);
        assert_eq!(s.last_sync_unix, d.last_sync_unix);
        assert_eq!(s.last_error, d.last_error);
    }

    #[test]
    fn mark_sync_err_sets_message_and_leaves_timestamp_intact() {
        let mut s = CloudSyncSettings::default();
        let prev_ts = s.last_sync_unix;
        s.last_error = "old".into();
        s.mark_sync_err("new error message");
        assert_eq!(s.last_error, "new error message");
        // Mark_sync_err does NOT update last_sync_unix.
        assert_eq!(s.last_sync_unix, prev_ts);
    }
}
