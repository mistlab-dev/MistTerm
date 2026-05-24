//! 团队片段本地缓存（与 personal `fragments.json` 分离）。

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::models::{FragmentSyncResponse, TeamFragment};
use crate::core::FragmentStats;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamFragmentCache {
    /// team_id → fragments
    #[serde(default)]
    pub by_team: HashMap<String, Vec<TeamFragment>>,
}

impl TeamFragmentCache {
    pub fn cache_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("team_fragments_cache.json");
        p
    }

    pub fn load() -> Self {
        let path = Self::cache_path();
        if !path.exists() {
            return Self::default();
        }
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::cache_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }

    pub fn apply_sync(&mut self, team_id: &str, resp: &FragmentSyncResponse) {
        let list = self.by_team.entry(team_id.to_string()).or_default();
        for frag in &resp.fragments {
            if let Some(i) = list.iter().position(|f| f.id == frag.id) {
                list[i] = frag.clone();
            } else {
                list.push(frag.clone());
            }
        }
        if !resp.deleted_ids.is_empty() {
            list.retain(|f| !resp.deleted_ids.contains(&f.id));
        }
    }

    pub fn fragments_for_team(&self, team_id: &str) -> &[TeamFragment] {
        self.by_team
            .get(team_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn to_fragment_stats(&self, team_id: &str, team_name: &str) -> Vec<FragmentStats> {
        self.fragments_for_team(team_id)
            .iter()
            .map(|f| f.to_fragment_stats(team_name))
            .collect()
    }

    pub fn upsert_fragment(&mut self, team_id: &str, frag: TeamFragment) {
        let list = self.by_team.entry(team_id.to_string()).or_default();
        if let Some(i) = list.iter().position(|f| f.id == frag.id) {
            list[i] = frag;
        } else {
            list.push(frag);
        }
    }

    pub fn remove_fragment(&mut self, team_id: &str, fragment_id: &str) {
        if let Some(list) = self.by_team.get_mut(team_id) {
            list.retain(|f| f.id != fragment_id);
        }
    }

    pub fn find_fragment(&self, team_id: &str, fragment_id: &str) -> Option<TeamFragment> {
        self.by_team
            .get(team_id)?
            .iter()
            .find(|f| f.id == fragment_id)
            .cloned()
    }
}
