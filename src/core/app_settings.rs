//! 应用级设置（Vault、审计等），与 egui 窗口几何持久化分离。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::ai_settings::AiSettings;
use crate::core::audit::AuditSettings;
use crate::core::team::TeamSettings;
use crate::core::vault::VaultSettings;
use crate::i18n::UiLanguage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// UI language (default English).
    #[serde(default)]
    pub ui_language: UiLanguage,
    #[serde(default)]
    pub vault: VaultSettings,
    #[serde(default)]
    pub audit: AuditSettings,
    #[serde(default)]
    pub ai: AiSettings,
    #[serde(default)]
    pub team: TeamSettings,
    /// 自动更新偏好（默认：自动检查开、后台下载开）。
    #[serde(default)]
    pub update: crate::core::updater::UpdateSettings,
    /// 首次启动已自动展示「新人上手」帮助页（只弹一次）。
    #[serde(default)]
    pub onboarding_help_shown: bool,
    /// 团队多机「确认并执行」走服务端「自动执行」（Plan/Lease/Run），默认开启。
    #[serde(default = "default_true")]
    pub control_plane_plans: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ui_language: UiLanguage::default(),
            vault: VaultSettings::default(),
            audit: AuditSettings::default(),
            ai: AiSettings::default(),
            team: TeamSettings::default(),
            update: crate::core::updater::UpdateSettings::default(),
            onboarding_help_shown: false,
            control_plane_plans: true,
        }
    }
}

impl AppSettings {
    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("settings.json");
        p
    }

    pub fn load() -> Self {
        let path = Self::default_path();
        let mut settings: Self = crate::security::encrypted_file::load_encrypted_json(&path);
        let mut changed = settings.ai.migrate_legacy_secrets();
        changed |= settings.update.migrate_auto_download_default_on();
        settings.team.lock_to_product_defaults();
        if changed {
            let _ = settings.save();
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::default_path(), self)
    }
}
