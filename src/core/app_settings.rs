//! 应用级设置（Vault、审计等），与 egui 窗口几何持久化分离。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::core::audit::AuditSettings;
use crate::core::vault::VaultSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub vault: VaultSettings,
    #[serde(default)]
    pub audit: AuditSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            vault: VaultSettings::default(),
            audit: AuditSettings::default(),
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
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
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
