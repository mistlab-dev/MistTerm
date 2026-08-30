//! 片段执行事件日志（用于时间范围内增量统计与成员对比）。

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MAX_EVENTS: usize = 8_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FragmentUsageEvent {
    pub ts: i64,
    pub fragment_id: String,
    /// `personal` | `team`
    pub scope: String,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub success: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FragmentUsageLogFile {
    #[serde(default)]
    events: Vec<FragmentUsageEvent>,
}

#[derive(Debug, Clone, Default)]
pub struct FragmentUsageLog {
    events: Vec<FragmentUsageEvent>,
    dirty: bool,
}

impl FragmentUsageLog {
    pub fn load() -> Self {
        let path = Self::log_path();
        let file: FragmentUsageLogFile =
            crate::security::encrypted_file::load_encrypted_json(&path);
        Self {
            events: file.events,
            dirty: false,
        }
    }

    pub fn log_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("fragment_usage_events.json");
        p
    }

    pub fn append(&mut self, event: FragmentUsageEvent) {
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            let drop = self.events.len() - MAX_EVENTS;
            self.events.drain(0..drop);
        }
        self.dirty = true;
    }

    pub fn save_if_dirty(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let file = FragmentUsageLogFile {
            events: self.events.clone(),
        };
        crate::security::encrypted_file::save_encrypted_json(&Self::log_path(), &file)?;
        self.dirty = false;
        Ok(())
    }

    pub fn events_since(&self, cutoff: i64) -> impl Iterator<Item = &FragmentUsageEvent> {
        self.events.iter().filter(move |e| e.ts >= cutoff)
    }

    pub fn all_events(&self) -> &[FragmentUsageEvent] {
        &self.events
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemberPeriodStats {
    pub user_id: String,
    pub display_name: String,
    pub run_count: u64,
    pub success_count: u64,
}

pub fn member_stats_in_range(
    events: &[FragmentUsageEvent],
    cutoff: i64,
    team_id: &str,
    members: &[crate::core::team::TeamMember],
) -> Vec<MemberPeriodStats> {
    let mut map: HashMap<String, MemberPeriodStats> = HashMap::new();
    for e in events {
        if e.ts < cutoff {
            continue;
        }
        if e.team_id.as_deref() != Some(team_id) {
            continue;
        }
        if e.scope != "team" {
            continue;
        }
        let uid = e
            .user_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let ent = map.entry(uid.clone()).or_insert_with(|| MemberPeriodStats {
            user_id: uid.clone(),
            display_name: e
                .display_name
                .clone()
                .unwrap_or_else(|| uid.clone()),
            ..Default::default()
        });
        ent.run_count += 1;
        if e.success {
            ent.success_count += 1;
        }
    }
    for m in members {
        if m.user_id.is_empty() {
            continue;
        }
        let name = if m.display_name.is_empty() {
            if m.username.is_empty() {
                m.email.clone()
            } else {
                m.username.clone()
            }
        } else {
            m.display_name.clone()
        };
        map.entry(m.user_id.clone())
            .and_modify(|e| {
                if e.display_name == e.user_id || e.display_name.is_empty() {
                    e.display_name = name.clone();
                }
            })
            .or_insert_with(|| MemberPeriodStats {
                user_id: m.user_id.clone(),
                display_name: name,
                ..Default::default()
            });
    }
    let mut rows: Vec<_> = map.into_values().collect();
    rows.sort_by(|a, b| b.run_count.cmp(&a.run_count));
    rows
}

pub fn apply_period_stats_to_fragments(
    fragments: &[crate::core::FragmentStats],
    events: &[FragmentUsageEvent],
    cutoff: i64,
    scope_filter: &str,
) -> Vec<crate::core::FragmentStats> {
    let mut agg: HashMap<String, (u32, u32, u64, i64)> = HashMap::new();
    for e in events {
        if e.ts < cutoff || e.scope != scope_filter {
            continue;
        }
        let ent = agg.entry(e.fragment_id.clone()).or_insert((0, 0, 0, 0));
        ent.0 += 1;
        if e.success {
            ent.1 += 1;
        }
        ent.2 += e.duration_ms;
        ent.3 = ent.3.max(e.ts);
    }
    let mut out = Vec::new();
    for f in fragments {
        let Some((usage, success, total_ms, last)) = agg.get(&f.id).copied() else {
            continue;
        };
        let mut s = f.clone();
        s.usage_count = usage;
        s.success_count = success;
        s.total_time_ms = total_ms;
        s.last_used = Some(last);
        out.push(s);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::FragmentStats;
    use crate::core::team::TeamMember;

    fn mk(
        ts: i64,
        fid: &str,
        scope: &str,
        tid: Option<&str>,
        uid: Option<&str>,
        dn: Option<&str>,
        success: bool,
        dur: u64,
    ) -> FragmentUsageEvent {
        FragmentUsageEvent {
            ts,
            fragment_id: fid.into(),
            scope: scope.into(),
            team_id: tid.map(|s| s.into()),
            user_id: uid.map(|s| s.into()),
            display_name: dn.map(|s| s.into()),
            success,
            duration_ms: dur,
        }
    }

    fn fs(id: &str) -> FragmentStats {
        FragmentStats::new(
            id.into(),
            id.into(),
            "echo x".into(),
            "ops".into(),
        )
    }

    // ------------------------------------------------ FragmentUsageLog basics
    #[test]
    fn default_log_is_empty_and_save_not_yet_dirty() {
        let log = FragmentUsageLog::default();
        assert!(log.all_events().is_empty());
        assert!(!log.dirty);
        // save_if_dirty short-circuits because dirty=false — no file I/O, no panic.
        let mut log2 = FragmentUsageLog::default();
        assert!(log2.save_if_dirty().is_ok());
    }

    #[test]
    fn append_adds_event_and_sets_dirty_flag() {
        let mut log = FragmentUsageLog::default();
        log.append(mk(100, "f1", "personal", None, None, None, true, 10));
        assert_eq!(log.all_events().len(), 1);
        assert!(log.dirty);
        assert_eq!(log.all_events()[0].ts, 100);
    }

    #[test]
    fn append_evicts_old_events_beyond_max_events() {
        let mut log = FragmentUsageLog::default();
        // MAX_EVENTS = 8000. We'll simulate appending MAX+2 events, expecting
        // only the last MAX remain. To keep it fast, we test locally with a
        // smaller scenario by using direct manipulation of events instead of
        // MAX (8000) iterations... But the MAX is module private. So append
        // 8002 events and confirm oldest 2 evicted.
        const PUSH: usize = 8002;
        for i in 0..PUSH {
            log.append(mk(i as i64, &format!("f{i}"), "personal", None, None, None, true, 1));
        }
        let evts = log.all_events();
        assert_eq!(evts.len(), MAX_EVENTS);
        assert_eq!(evts[0].ts, 2); // evicted events 0 and 1
        assert_eq!(evts.last().unwrap().ts, (PUSH - 1) as i64);
    }

    #[test]
    fn events_since_filters_out_before_cutoff() {
        let mut log = FragmentUsageLog::default();
        log.append(mk(1, "a", "personal", None, None, None, true, 1));
        log.append(mk(10, "b", "personal", None, None, None, true, 2));
        log.append(mk(100, "c", "personal", None, None, None, true, 3));
        let collected: Vec<i64> = log.events_since(10).map(|e| e.ts).collect();
        assert_eq!(collected, vec![10, 100]);
    }

    #[test]
    fn fragment_usage_event_serde_round_trip() {
        let e = mk(
            1234,
            "frag-1",
            "team",
            Some("team-x"),
            Some("user-1"),
            Some("Alice"),
            true,
            42,
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: FragmentUsageEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.ts, 1234);
        assert_eq!(back.fragment_id, "frag-1");
        assert_eq!(back.scope, "team");
        assert_eq!(back.team_id.as_deref(), Some("team-x"));
        assert_eq!(back.user_id.as_deref(), Some("user-1"));
        assert_eq!(back.display_name.as_deref(), Some("Alice"));
        assert!(back.success);
        assert_eq!(back.duration_ms, 42);
    }

    #[test]
    fn fragment_usage_event_optional_fields_default_when_absent() {
        let json = r#"{"ts":99,"fragment_id":"x","scope":"personal","success":false,"duration_ms":3}"#;
        let e: FragmentUsageEvent = serde_json::from_str(json).unwrap();
        assert!(e.team_id.is_none());
        assert!(e.user_id.is_none());
        assert!(e.display_name.is_none());
    }

    // ------------------------------------------------ member_stats_in_range
    #[test]
    fn member_stats_groups_by_uid_skips_before_cutoff_wrong_scope_wrong_team() {
        let events = vec![
            // counted: team T1 scope team ts >= 1000
            mk(1000, "f1", "team", Some("T1"), Some("u1"), None, true, 10),
            mk(1005, "f2", "team", Some("T1"), Some("u1"), None, false, 5),
            mk(1010, "f3", "team", Some("T1"), Some("u2"), Some("Bob"), true, 1),
            // ts before cutoff
            mk(999, "f1", "team", Some("T1"), Some("u3"), None, true, 1),
            // wrong team
            mk(1001, "f1", "team", Some("T2"), Some("u9"), None, true, 1),
            // wrong scope
            mk(1002, "f1", "personal", Some("T1"), Some("u9"), None, true, 1),
        ];
        let rows = member_stats_in_range(&events, 1000, "T1", &[]);
        // sorted by run_count desc: u1=2, u2=1
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].user_id, "u1");
        assert_eq!(rows[0].run_count, 2);
        assert_eq!(rows[0].success_count, 1); // 1 true, 1 false
        // fallback display_name = user_id because user had None
        assert_eq!(rows[0].display_name, "u1");
        assert_eq!(rows[1].user_id, "u2");
        assert_eq!(rows[1].display_name, "Bob");
        assert_eq!(rows[1].success_count, 1);
    }

    #[test]
    fn member_stats_enriches_with_member_list_display_name_and_includes_zero_run_members() {
        let events = vec![
            mk(
                2000,
                "f1",
                "team",
                Some("T1"),
                Some("u-noise"),
                // Event provides a display_name that is EQUAL to user_id,
                // causing member-list enrichment to overwrite display_name.
                Some("u-noise"),
                true,
                1,
            ),
        ];
        let members = [
            TeamMember {
                user_id: "u-noise".into(),
                email: "a@x".into(),
                username: "".into(),
                display_name: "Alice".into(),
                role: "viewer".into(),
            },
            // user_id empty → skip
            TeamMember {
                user_id: String::new(),
                email: "e@x".into(),
                username: "x".into(),
                display_name: String::new(),
                role: "viewer".into(),
            },
            // inactive member: never ran anything → still included with 0 counts
            TeamMember {
                user_id: "u-silent".into(),
                email: "s@x".into(),
                username: "silent".into(),
                display_name: String::new(),
                role: "viewer".into(),
            },
            // display_name empty, username empty → fallback email
            TeamMember {
                user_id: "u-mail-only".into(),
                email: "mail@x".into(),
                username: String::new(),
                display_name: String::new(),
                role: "viewer".into(),
            },
        ];
        let rows = member_stats_in_range(&events, 1000, "T1", &members);
        assert_eq!(rows.len(), 3, "expected 3 rows: u-noise (enriched), u-silent, u-mail-only");

        let find = |uid: &str| rows.iter().find(|r| r.user_id == uid).unwrap();

        let a = find("u-noise");
        assert_eq!(a.display_name, "Alice", "event uid==name should be overwritten by member.display_name");
        assert_eq!(a.run_count, 1);

        let s = find("u-silent");
        assert_eq!(s.run_count, 0);
        assert_eq!(s.success_count, 0);
        assert_eq!(s.display_name, "silent"); // username fallback

        let m = find("u-mail-only");
        assert_eq!(m.display_name, "mail@x");
    }

    #[test]
    fn member_stats_empty_events_and_members_is_empty() {
        assert!(member_stats_in_range(&[], 0, "T1", &[]).is_empty());
    }

    #[test]
    fn member_stats_user_id_missing_in_event_becomes_unknown() {
        let events = vec![
            mk(100, "f1", "team", Some("T1"), None, None, true, 1),
        ];
        let rows = member_stats_in_range(&events, 0, "T1", &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].user_id, "unknown");
        assert_eq!(rows[0].display_name, "unknown");
    }

    // ------------------------------------------------ apply_period_stats_to_fragments
    #[test]
    fn apply_period_applies_aggregated_events_with_scope_and_cutoff() {
        let f1 = fs("a");
        let f2 = fs("b");
        let f3 = fs("c");
        let events = vec![
            // personal scope, counted for a
            mk(500, "a", "personal", None, None, None, true, 10),
            mk(600, "a", "personal", None, None, None, true, 20),
            mk(650, "a", "personal", None, None, None, false, 5),
            // team scope → filtered by personal scope filter
            mk(600, "a", "team", None, None, None, true, 999),
            // before cutoff → ignored
            mk(100, "a", "personal", None, None, None, true, 9999),
            // personal b counted (2 runs)
            mk(700, "b", "personal", None, None, None, false, 40),
            mk(701, "b", "personal", None, None, None, false, 5),
            // c: personal but cutoff missed
            mk(50, "c", "personal", None, None, None, true, 1),
        ];
        let got = apply_period_stats_to_fragments(&[f1, f2, f3], &events, 400, "personal");
        assert_eq!(got.len(), 2, "c had no in-range personal events");
        let a = got.iter().find(|s| s.id == "a").unwrap();
        assert_eq!(a.usage_count, 3);
        assert_eq!(a.success_count, 2);
        assert_eq!(a.total_time_ms, 10 + 20 + 5);
        assert_eq!(a.last_used, Some(650)); // max(500,600,650)
        let b = got.iter().find(|s| s.id == "b").unwrap();
        assert_eq!(b.usage_count, 2);
        assert_eq!(b.success_count, 0);
        assert_eq!(b.total_time_ms, 45);
        assert_eq!(b.last_used, Some(701));
    }

    #[test]
    fn apply_period_team_scope_only_aggregates_team_events() {
        let f1 = fs("a");
        let events = vec![
            mk(1000, "a", "personal", None, None, None, true, 1),
            mk(1001, "a", "team", Some("T1"), None, None, true, 7),
            mk(1002, "a", "team", Some("T1"), None, None, false, 3),
        ];
        let got = apply_period_stats_to_fragments(&[f1], &events, 0, "team");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].usage_count, 2);
        assert_eq!(got[0].success_count, 1);
        assert_eq!(got[0].total_time_ms, 10);
    }

    #[test]
    fn apply_period_returns_empty_when_no_events_match() {
        let f = [fs("a"), fs("b")];
        let got = apply_period_stats_to_fragments(&f, &[], 0, "personal");
        assert!(got.is_empty());
    }

    // ------------------------------------------------ MemberPeriodStats Default
    #[test]
    fn member_period_stats_default_is_zeroes_and_empty() {
        let r = MemberPeriodStats::default();
        assert_eq!(r.user_id, "");
        assert_eq!(r.display_name, "");
        assert_eq!(r.run_count, 0);
        assert_eq!(r.success_count, 0);
    }
}
