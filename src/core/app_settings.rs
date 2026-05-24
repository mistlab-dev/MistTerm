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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ui_language: UiLanguage::default(),
            vault: VaultSettings::default(),
            audit: AuditSettings::default(),
            ai: AiSettings::default(),
            team: TeamSettings::default(),
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
