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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::team::models::{TeamInfo, TeamRole, TeamServer, TeamSyncEntry, TeamUser};

    fn make_user(id: &str) -> TeamUser {
        TeamUser {
            id: id.to_string(),
            email: format!("{id}@example.com"),
            username: id.to_string(),
            display_name: String::new(),
            email_verified: false,
            created_at: None,
            updated_at: None,
        }
    }

    fn make_membership(id: &str, name: &str, role: &str) -> TeamMembership {
        TeamMembership {
            team: TeamInfo {
                id: id.to_string(),
                name: name.to_string(),
                description: String::new(),
                created_at: None,
                updated_at: None,
            },
            role: role.to_string(),
        }
    }

    // ------------------------------------------------------ default/serde
    #[test]
    fn default_is_empty() {
        let s = TeamState::default();
        assert!(s.user.is_none());
        assert!(s.teams.is_empty());
        assert!(s.current_team_id.is_none());
        assert!(s.sync_cursors.is_empty());
        assert!(s.sync_entries.is_empty());
        assert!(s.last_error.is_empty());
        assert!(s.last_sync_unix.is_none());
    }

    #[test]
    fn serde_empty_object_behaves_like_default() {
        let s: TeamState = serde_json::from_str("{}").unwrap();
        assert!(s.user.is_none());
        assert!(s.teams.is_empty());
        let rt = serde_json::to_string(&TeamState::default()).unwrap();
        let s2: TeamState = serde_json::from_str(&rt).unwrap();
        assert!(s2.current_team_id.is_none());
        assert!(s2.sync_cursors.is_empty());
    }

    // ------------------------------------------------------ cursor ops
    #[test]
    fn cursor_accessors_read_and_write() {
        let mut s = TeamState::default();
        assert_eq!(s.cursor_for("t1"), "");
        s.set_cursor("t1", "cur_v1".into());
        assert_eq!(s.cursor_for("t1"), "cur_v1");
        // other team untouched
        assert_eq!(s.cursor_for("t2"), "");
    }

    // ------------------------------------------------------ current_membership / current_role
    #[test]
    fn current_membership_resolves_by_id() {
        let mut s = TeamState::default();
        s.teams.push(make_membership("t-a", "Alpha", "admin"));
        s.teams.push(make_membership("t-b", "Beta", "viewer"));
        // No current_team_id yet
        assert!(s.current_membership().is_none());
        assert_eq!(s.current_role(), TeamRole::Viewer);

        s.current_team_id = Some("t-a".into());
        let m = s.current_membership().unwrap();
        assert_eq!(m.team.id, "t-a");
        assert_eq!(s.current_role(), TeamRole::Admin);

        s.current_team_id = Some("t-missing".into());
        assert!(s.current_membership().is_none());
        assert_eq!(s.current_role(), TeamRole::Viewer);
    }

    // ------------------------------------------------------ ensure_default_team
    #[test]
    fn ensure_default_team_noop_when_set_or_empty() {
        let mut s = TeamState::default();
        s.ensure_default_team();
        assert!(s.current_team_id.is_none());

        s.current_team_id = Some("existing".into());
        s.teams.push(make_membership("t-x", "X", "viewer"));
        s.ensure_default_team();
        assert_eq!(s.current_team_id.as_deref(), Some("existing"));
    }

    #[test]
    fn ensure_default_team_picks_first_when_unset() {
        let mut s = TeamState::default();
        s.teams.push(make_membership("t-1", "one", "editor"));
        s.teams.push(make_membership("t-2", "two", "viewer"));
        s.ensure_default_team();
        assert_eq!(s.current_team_id.as_deref(), Some("t-1"));
    }

    // ------------------------------------------------------ sync_entries / servers
    #[test]
    fn sync_entry_and_server_lookups() {
        let mut s = TeamState::default();
        assert!(s.sync_entry_for("any").is_none());
        let empty_srv = s.servers_for_team("any");
        assert!(empty_srv.is_empty());

        s.sync_entries.insert(
            "t-a".into(),
            TeamSyncEntry {
                team_id: "t-a".into(),
                team_name: "A".into(),
                role: "viewer".into(),
                vault_config: None,
                credential: None,
                servers: vec![
                    TeamServer {
                        id: "s1".into(),
                        name: "S1".into(),
                        host: "h1".into(),
                        port: 22,
                        username: "u1".into(),
                        tags: Default::default(),
                        vault_credential_path: String::new(),
                        sort_order: 0,
                    },
                ],
            },
        );
        let e = s.sync_entry_for("t-a").unwrap();
        assert_eq!(e.team_id, "t-a");
        let svs = s.servers_for_team("t-a");
        assert_eq!(svs.len(), 1);
        assert_eq!(svs[0].host, "h1");
    }

    // ------------------------------------------------------ clear_session
    #[test]
    fn clear_session_wipes_all_session_state_fields() {
        let mut s = TeamState::default();
        s.user = Some(make_user("u"));
        s.teams.push(make_membership("t-1", "t", "admin"));
        s.current_team_id = Some("t-1".into());
        s.sync_cursors.insert("t-1".into(), "c".into());
        s.sync_entries.insert(
            "t-1".into(),
            TeamSyncEntry {
                team_id: "t-1".into(),
                team_name: "T".into(),
                role: "admin".into(),
                vault_config: None,
                credential: None,
                servers: vec![],
            },
        );
        s.last_error = "oops".into();
        s.last_sync_unix = Some(42);
        s.clear_session();

        assert!(s.user.is_none());
        assert!(s.teams.is_empty());
        assert!(s.current_team_id.is_none());
        assert!(s.sync_cursors.is_empty());
        assert!(s.sync_entries.is_empty());
        assert!(s.last_error.is_empty());
        // clear_session wipes everything except last_sync_unix (not listed)
    }
}
