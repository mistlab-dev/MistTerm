//! 应用级设置（Vault、审计等），与 egui 窗口几何持久化分离。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::core::ai_settings::AiSettings;
use crate::core::audit::AuditSettings;
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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ui_language: UiLanguage::default(),
            vault: VaultSettings::default(),
            audit: AuditSettings::default(),
            ai: AiSettings::default(),
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
        if !path.exists() {
            return Self::default();
        }
        let mut settings: Self = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        if settings.ai.migrate_keyring_to_local() {
            let _ = settings.save();
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }
}
