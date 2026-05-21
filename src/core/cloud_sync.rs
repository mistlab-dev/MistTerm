//! 云端同步配置（MVP：本地导出/导入包，个人备份用）。
//! 团队片段正式能力走团队 API，见 `docs/tech/TEAM-PLATFORM-DEV-PLAN.md`。

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
