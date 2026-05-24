//! 团队本地状态（用户、团队列表、同步 cursor）。

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::models::{TeamMembership, TeamUser};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamState {
    #[serde(default)]
    pub user: Option<TeamUser>,
    #[serde(default)]
    pub teams: Vec<TeamMembership>,
    #[serde(default)]
    pub current_team_id: Option<String>,
    /// team_id → fragments:sync cursor
    #[serde(default)]
    pub sync_cursors: HashMap<String, String>,
    #[serde(default)]
    pub last_sync_unix: Option<i64>,
    #[serde(default)]
    pub last_error: String,
}

impl TeamState {
    pub fn config_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("team_state.json");
        p
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
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }

    pub fn clear_session(&mut self) {
        self.user = None;
        self.teams.clear();
        self.current_team_id = None;
        self.sync_cursors.clear();
        self.last_error.clear();
        let _ = self.save();
    }

    pub fn current_membership(&self) -> Option<&TeamMembership> {
        let tid = self.current_team_id.as_deref()?;
        self.teams.iter().find(|m| m.team.id == tid)
    }

    pub fn current_role(&self) -> super::models::TeamRole {
        self.current_membership()
            .map(|m| m.role_enum())
            .unwrap_or(super::models::TeamRole::Viewer)
    }

    pub fn cursor_for(&self, team_id: &str) -> String {
        self.sync_cursors
            .get(team_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_cursor(&mut self, team_id: &str, cursor: String) {
        self.sync_cursors.insert(team_id.to_string(), cursor);
    }

    pub fn ensure_default_team(&mut self) {
        if self.current_team_id.is_some() {
            return;
        }
        if let Some(first) = self.teams.first() {
            self.current_team_id = Some(first.team.id.clone());
        }
    }
}
