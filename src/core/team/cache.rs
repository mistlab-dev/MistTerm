//! 团队片段本地缓存（与 personal `fragments.json` 分离）。

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::models::{FragmentSyncResponse, TeamFragment};
use crate::core::FragmentStats;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FragmentUsageOverlay {
    pub usage_count: u32,
    pub success_count: u32,
    pub total_time_ms: u64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamFragmentCache {
    /// team_id → fragments
    #[serde(default)]
    pub by_team: HashMap<String, Vec<TeamFragment>>,
    /// 本机执行团队片段的统计（fragment_id → overlay）
    #[serde(default)]
    pub usage_overlay: HashMap<String, FragmentUsageOverlay>,
}

impl TeamFragmentCache {
    pub fn cache_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("team_fragments_cache.json");
        p
    }

    pub fn load() -> Self {
        crate::security::encrypted_file::load_encrypted_json(&Self::cache_path())
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::cache_path(), self)
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

    pub fn record_usage(&mut self, fragment_id: &str, success: bool, dur_ms: u64) {
        let entry = self.usage_overlay.entry(fragment_id.to_string()).or_default();
        entry.usage_count = entry.usage_count.saturating_add(1);
        if success {
            entry.success_count = entry.success_count.saturating_add(1);
        }
        entry.total_time_ms = entry.total_time_ms.saturating_add(dur_ms);
        entry.last_used_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        );
    }

    pub fn apply_analytics_rows(&mut self, rows: &[super::models::FragmentAnalyticsRow]) {
        for row in rows {
            let entry = self
                .usage_overlay
                .entry(row.fragment_id.clone())
                .or_default();
            if row.usage_count > entry.usage_count {
                entry.usage_count = row.usage_count;
            }
            if row.success_count > entry.success_count {
                entry.success_count = row.success_count;
            }
            if row.total_time_ms > entry.total_time_ms {
                entry.total_time_ms = row.total_time_ms;
            }
            if row.last_used_at.is_some() {
                entry.last_used_at = row.last_used_at;
            }
        }
    }

    fn merge_overlay(&self, mut stats: FragmentStats) -> FragmentStats {
        if let Some(o) = self.usage_overlay.get(&stats.id) {
            stats.usage_count = stats.usage_count.max(o.usage_count);
            stats.success_count = stats.success_count.max(o.success_count);
            stats.total_time_ms = stats.total_time_ms.max(o.total_time_ms);
            if stats.last_used.is_none() {
                stats.last_used = o.last_used_at;
            }
        }
        stats
    }

    pub fn to_fragment_stats(&self, team_id: &str, team_name: &str) -> Vec<FragmentStats> {
        self.fragments_for_team(team_id)
            .iter()
            .map(|f| self.merge_overlay(f.to_fragment_stats(team_name)))
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
