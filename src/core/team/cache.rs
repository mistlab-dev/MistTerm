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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::team::models::{
        FragmentAnalyticsRow, FragmentSyncResponse, TeamFragment,
    };

    fn tf(id: &str, title: &str) -> TeamFragment {
        TeamFragment {
            id: id.to_string(),
            team_id: String::new(),
            title: title.to_string(),
            command: format!("echo {id}"),
            category: String::new(),
            tags: "[]".to_string(),
            variables: "{}".to_string(),
            scope: String::new(),
            status: "published".to_string(),
            revision: 1,
            locked_by: String::new(),
            locked_at: None,
            created_by: None,
            updated_by: None,
            created_at: None,
            updated_at: None,
            usage_count: 0,
            success_count: 0,
            total_time_ms: 0,
            last_used_at: None,
        }
    }

    // ---------------------------------------------------- Default + Serde
    #[test]
    fn default_is_empty_and_roundtrips() {
        let c = TeamFragmentCache::default();
        assert!(c.by_team.is_empty());
        assert!(c.usage_overlay.is_empty());
        let json = serde_json::to_string(&c).unwrap();
        let c2: TeamFragmentCache = serde_json::from_str(&json).unwrap();
        assert_eq!(c2.by_team.len(), 0);
        let c3: TeamFragmentCache = serde_json::from_str("{}").unwrap();
        assert!(c3.by_team.is_empty());
        assert!(c3.usage_overlay.is_empty());
    }

    // ---------------------------------------------------- upsert/remove/find
    #[test]
    fn upsert_adds_then_updates_by_id() {
        let mut c = TeamFragmentCache::default();
        c.upsert_fragment("t1", tf("a", "A"));
        c.upsert_fragment("t1", tf("b", "B"));
        assert_eq!(c.fragments_for_team("t1").len(), 2);
        assert_eq!(c.find_fragment("t1", "a").unwrap().title, "A");
        assert!(c.find_fragment("t2", "a").is_none());

        // overwrite `a` with a new title
        let mut a2 = tf("a", "A'");
        a2.revision = 42;
        c.upsert_fragment("t1", a2);
        let list = c.fragments_for_team("t1");
        assert_eq!(list.len(), 2);
        let found = list.iter().find(|f| f.id == "a").unwrap();
        assert_eq!(found.title, "A'");
        assert_eq!(found.revision, 42);
    }

    #[test]
    fn remove_and_missing_team_nops() {
        let mut c = TeamFragmentCache::default();
        c.upsert_fragment("t1", tf("a", "A"));
        c.upsert_fragment("t1", tf("b", "B"));
        c.remove_fragment("missing-team", "a"); // noop
        c.remove_fragment("t1", "not-exist"); // noop
        assert_eq!(c.fragments_for_team("t1").len(), 2);
        c.remove_fragment("t1", "a");
        let list = c.fragments_for_team("t1");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "b");
    }

    // ---------------------------------------------------- apply_sync
    #[test]
    fn apply_sync_merges_fragments_and_removes_deleted() {
        let mut c = TeamFragmentCache::default();
        c.upsert_fragment("t1", tf("old-a", "OA"));
        c.upsert_fragment("t1", tf("keep", "K"));

        c.apply_sync(
            "t1",
            &FragmentSyncResponse {
                cursor: "c1".into(),
                fragments: vec![tf("new-n", "NN"), tf("old-a", "OA-updated")],
                deleted_ids: vec!["keep".to_string()],
                server_time: None,
            },
        );
        let list = c.fragments_for_team("t1");
        let mut ids: Vec<&str> = list.iter().map(|f| f.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["new-n", "old-a"]);
        let old_a = list.iter().find(|f| f.id == "old-a").unwrap();
        assert_eq!(old_a.title, "OA-updated");
        // `keep` is removed
        assert!(!ids.contains(&"keep"));
    }

    #[test]
    fn apply_sync_to_another_team_is_isolated() {
        let mut c = TeamFragmentCache::default();
        c.upsert_fragment("t1", tf("a", "A"));
        c.apply_sync(
            "t2",
            &FragmentSyncResponse {
                cursor: "c".into(),
                fragments: vec![tf("b", "B")],
                deleted_ids: vec![],
                server_time: None,
            },
        );
        assert_eq!(c.fragments_for_team("t1").len(), 1);
        assert_eq!(c.fragments_for_team("t2").len(), 1);
        assert_eq!(c.fragments_for_team("missing").len(), 0);
    }

    // ---------------------------------------------------- record_usage
    #[test]
    fn record_usage_accumulates_saturating_and_sets_last_used() {
        let mut c = TeamFragmentCache::default();
        c.record_usage("f1", true, 10);
        c.record_usage("f1", true, 5);
        c.record_usage("f1", false, 3);
        let o = c.usage_overlay.get("f1").unwrap();
        assert_eq!(o.usage_count, 3);
        assert_eq!(o.success_count, 2);
        assert_eq!(o.total_time_ms, 18);
        assert!(o.last_used_at.is_some());

        // saturation for u32/u64 should never wrap (even when repeated near-max).
        let mut big = FragmentUsageOverlay {
            usage_count: u32::MAX,
            success_count: u32::MAX,
            total_time_ms: u64::MAX,
            last_used_at: Some(1),
        };
        let before = (big.usage_count, big.success_count, big.total_time_ms);
        big.usage_count = big.usage_count.saturating_add(5);
        big.success_count = big.success_count.saturating_add(5);
        big.total_time_ms = big.total_time_ms.saturating_add(5);
        assert_eq!(
            (big.usage_count, big.success_count, big.total_time_ms),
            before
        );
    }

    // ---------------------------------------------------- apply_analytics_rows
    #[test]
    fn apply_analytics_rows_only_takes_larger_numbers() {
        let mut c = TeamFragmentCache::default();
        c.record_usage("f1", true, 10); // u=1 s=1 t=10
        c.apply_analytics_rows(&[
            FragmentAnalyticsRow {
                fragment_id: "f1".into(),
                usage_count: 5,    // wins
                success_count: 0,  // loses
                total_time_ms: 3,  // loses
                last_used_at: None,
            },
            FragmentAnalyticsRow {
                fragment_id: "f2".into(),
                usage_count: 7,
                success_count: 7,
                total_time_ms: 70,
                last_used_at: Some(1234),
            },
        ]);
        let f1 = c.usage_overlay.get("f1").unwrap();
        assert_eq!(f1.usage_count, 5);
        assert_eq!(f1.success_count, 1); // kept original max
        assert_eq!(f1.total_time_ms, 10); // kept original max

        let f2 = c.usage_overlay.get("f2").unwrap();
        assert_eq!(f2.usage_count, 7);
        assert_eq!(f2.success_count, 7);
        assert_eq!(f2.total_time_ms, 70);
        assert_eq!(f2.last_used_at, Some(1234));
    }

    // ---------------------------------------------------- to_fragment_stats
    #[test]
    fn to_fragment_stats_merges_overlay_into_stats() {
        let mut c = TeamFragmentCache::default();
        let mut f = tf("s1", "Stats One");
        f.usage_count = 1; // embedded in the fragment itself (low)
        c.upsert_fragment("t1", f);
        // Simulate local execution: overlay has higher counts + last_used.
        let before_unix = 100;
        let mut o = FragmentUsageOverlay {
            usage_count: 5,
            success_count: 4,
            total_time_ms: 1000,
            last_used_at: Some(before_unix),
        };
        c.usage_overlay.insert("s1".into(), o.clone());

        let stats = c.to_fragment_stats("t1", "Acme");
        assert_eq!(stats.len(), 1);
        let s = &stats[0];
        assert_eq!(s.id, "s1");
        assert!(s.tags.iter().any(|t| t == "@Acme"), "team tag missing");
        // overlay wins (max) over fragment's embedded usage fields.
        assert_eq!(s.usage_count, 5);
        assert_eq!(s.success_count, 4);
        assert_eq!(s.total_time_ms, 1000);
        assert_eq!(s.last_used, Some(before_unix));

        // When fragment already has last_used, overlay does not clobber it.
        o.last_used_at = Some(9999);
        c.usage_overlay.insert("s1".into(), o);
        // Simulate: mutate fragment to have last_used_at by editing directly via find/upsert
        let mut frag = c.find_fragment("t1", "s1").unwrap();
        frag.last_used_at = Some(5000);
        c.upsert_fragment("t1", frag);
        let stats2 = c.to_fragment_stats("t1", "Acme");
        // fragment's own last_used_at (5000) should win (merge only fills None).
        assert_eq!(stats2[0].last_used, Some(5000));
    }

    #[test]
    fn overlay_default_all_zeroes_and_none() {
        let o = FragmentUsageOverlay::default();
        assert_eq!(o.usage_count, 0);
        assert_eq!(o.success_count, 0);
        assert_eq!(o.total_time_ms, 0);
        assert!(o.last_used_at.is_none());
    }
}
