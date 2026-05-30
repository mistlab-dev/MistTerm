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
