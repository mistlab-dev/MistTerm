//! 团队本地状态（用户、团队列表、同步 cursor）。

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::models::{TeamMembership, TeamServer, TeamSyncEntry, TeamUser};

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
    /// `GET /v1/team/sync` 缓存（team_id → 条目）
    #[serde(default)]
    pub sync_entries: HashMap<String, TeamSyncEntry>,
}

impl TeamState {
    pub fn config_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("team_state.json");
        p
    }

    pub fn load() -> Self {
        crate::security::encrypted_file::load_encrypted_json(&Self::config_path())
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::config_path(), self)
    }

    pub fn clear_session(&mut self) {
        self.user = None;
        self.teams.clear();
        self.current_team_id = None;
        self.sync_cursors.clear();
        self.sync_entries.clear();
        self.last_error.clear();
        let _ = self.save();
    }

    pub fn servers_for_team(&self, team_id: &str) -> Vec<TeamServer> {
        self.sync_entries
            .get(team_id)
            .map(|e| e.servers.clone())
            .unwrap_or_default()
    }

    pub fn sync_entry_for(&self, team_id: &str) -> Option<&TeamSyncEntry> {
        self.sync_entries.get(team_id)
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
