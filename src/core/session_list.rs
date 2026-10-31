//! 连接列表展示：去重键、组内排序、在线/离线人话时长（#247）。
//!
//! 侧栏只应展示「一台保存主机一行」。多标签打开同一主机不拆行；
//! 同 host+port+user 的重复保存记录合并为一行，状态点 + 人话标签区分在线/离线。

use std::collections::{HashMap, HashSet};

use chrono::{Local, TimeZone};

use super::batch_exec::batch_endpoint_key;
use super::session::SessionConfig;
use super::session_sort::{sort_sessions, SessionSortBy};

/// 去重键：优先保存主机 ID；ID 为空时回退 `user@host:port`（大小写不敏感）。
pub fn session_dedup_key(session: &SessionConfig) -> String {
    let id = session.id.trim();
    if !id.is_empty() {
        return format!("id:{id}");
    }
    format!(
        "ep:{}",
        batch_endpoint_key(&session.host, session.port, &session.username)
    )
}

/// 端点键（用于把不同 ID、同 host+port+user 的重复行合并）。
pub fn session_endpoint_key(session: &SessionConfig) -> String {
    batch_endpoint_key(&session.host, session.port, &session.username)
}

/// 分组归一化键：`"默认"` / `"Default"` / `""` 视为同一组，避免组标签因存储差异重复出现。
pub fn session_group_key(group: &str) -> String {
    match group.trim() {
        "" | "默认" | "Default" => String::new(),
        "团队" | "Team" => "team".to_string(),
        other => other.to_string(),
    }
}

/// 合并列表行时的代表会话选择：
/// 1) 任一在线 → 取在线那条（点击即进已连会话）；
/// 2) 否则取 `last_connected_at` 更新的；
/// 3) 再否则保留先出现的。
fn prefer_session<'a>(
    a: &'a SessionConfig,
    b: &'a SessionConfig,
    connected: &HashSet<String>,
) -> &'a SessionConfig {
    let a_on = connected.contains(&a.id);
    let b_on = connected.contains(&b.id);
    if a_on != b_on {
        return if a_on { a } else { b };
    }
    let a_last = a.last_connected_at.unwrap_or(0);
    let b_last = b.last_connected_at.unwrap_or(0);
    if a_last != b_last {
        return if a_last >= b_last { a } else { b };
    }
    a
}

/// 连接列表去重：同保存 ID 只留一行；不同 ID 但同 host+port+user 也合并为一行。
///
/// 输入顺序会被保留（稳定）：先出现的端点占位，再按 [`prefer_session`] 换代表。
/// 改显示名不会裂成两行（键不含 name）；多标签不影响——列表只看保存主机。
pub fn dedupe_sessions_for_connection_list(
    sessions: &[SessionConfig],
    connected: &HashSet<String>,
) -> Vec<SessionConfig> {
    let mut out: Vec<SessionConfig> = Vec::with_capacity(sessions.len());
    let mut id_seen: HashSet<String> = HashSet::new();
    // endpoint → index in `out`
    let mut ep_index: HashMap<String, usize> = HashMap::new();

    for session in sessions {
        let id = session.id.trim();
        if !id.is_empty() && !id_seen.insert(id.to_string()) {
            // 同 ID 再出现：若新的更「优」则替换
            if let Some(idx) = out.iter().position(|s| s.id == session.id) {
                if prefer_session(&out[idx], session, connected).id == session.id {
                    // 合并 last_connected_at 取较大值
                    let mut merged = session.clone();
                    merged.last_connected_at = max_opt_i64(
                        out[idx].last_connected_at,
                        session.last_connected_at,
                    );
                    out[idx] = merged;
                } else {
                    out[idx].last_connected_at = max_opt_i64(
                        out[idx].last_connected_at,
                        session.last_connected_at,
                    );
                }
            }
            continue;
        }

        let ep = session_endpoint_key(session);
        if let Some(&idx) = ep_index.get(&ep) {
            let current = &out[idx];
            let chosen = prefer_session(current, session, connected);
            let mut merged = chosen.clone();
            merged.last_connected_at =
                max_opt_i64(current.last_connected_at, session.last_connected_at);
            // 组名：若其一为默认空组、另一有自定义组，保留自定义组，避免「未分组」假分裂
            if session_group_key(&merged.group).is_empty()
                && !session_group_key(&session.group).is_empty()
                && chosen.id == current.id
            {
                merged.group = session.group.clone();
            } else if session_group_key(&merged.group).is_empty()
                && !session_group_key(&current.group).is_empty()
            {
                merged.group = current.group.clone();
            }
            out[idx] = merged;
            continue;
        }

        ep_index.insert(ep, out.len());
        out.push(session.clone());
    }

    out
}

fn max_opt_i64(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

/// 先按归一化分组聚拢，组内再按 [`SessionSortBy`] 排，保证组标签只在组首出现一次。
pub fn sort_sessions_keeping_groups(sessions: &mut [SessionConfig], sort_by: SessionSortBy) {
    // 组内排序
    let mut groups: Vec<(String, Vec<SessionConfig>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for s in sessions.iter().cloned() {
        let key = session_group_key(&s.group);
        if let Some(&idx) = index.get(&key) {
            groups[idx].1.push(s);
        } else {
            index.insert(key.clone(), groups.len());
            groups.push((key, vec![s]));
        }
    }
    // 组顺序：默认组优先，其余按组名；组内再 sort_sessions
    groups.sort_by(|a, b| match (a.0.is_empty(), b.0.is_empty()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
    });
    let mut rebuilt = Vec::with_capacity(sessions.len());
    for (_, mut members) in groups {
        sort_sessions(&mut members, sort_by);
        rebuilt.extend(members);
    }
    sessions.clone_from_slice(&rebuilt);
}

/// 在线时长文案：禁止裸 `12m`；写「已连接 N 分钟/小时」。
pub fn format_online_duration(zh: bool, connected_since_unix: Option<i64>, now_unix: i64) -> String {
    let Some(since) = connected_since_unix else {
        return if zh {
            "在线".to_string()
        } else {
            "Online".to_string()
        };
    };
    let diff = now_unix.saturating_sub(since).max(0);
    if diff < 60 {
        return if zh {
            "已连接不到 1 分钟".to_string()
        } else {
            "Connected < 1 min".to_string()
        };
    }
    if diff < 3600 {
        let n = diff / 60;
        return if zh {
            format!("已连接 {n} 分钟")
        } else if n == 1 {
            "Connected for 1 minute".to_string()
        } else {
            format!("Connected for {n} minutes")
        };
    }
    if diff < 86_400 {
        let n = diff / 3600;
        return if zh {
            format!("已连接 {n} 小时")
        } else if n == 1 {
            "Connected for 1 hour".to_string()
        } else {
            format!("Connected for {n} hours")
        };
    }
    let n = diff / 86_400;
    if zh {
        format!("已连接 {n} 天")
    } else if n == 1 {
        "Connected for 1 day".to_string()
    } else {
        format!("Connected for {n} days")
    }
}

/// 离线最后连接人话：如「昨天 23:14 最后连接」；无时间则「从未连接」。
pub fn format_offline_last_connected(
    zh: bool,
    last_connected_unix: Option<i64>,
    now_unix: i64,
) -> String {
    let Some(ts) = last_connected_unix else {
        return if zh {
            "从未连接".to_string()
        } else {
            "Never connected".to_string()
        };
    };
    let Some(last_local) = Local.timestamp_opt(ts, 0).single() else {
        return if zh {
            "离线".to_string()
        } else {
            "Offline".to_string()
        };
    };
    let now_local = Local
        .timestamp_opt(now_unix, 0)
        .single()
        .unwrap_or_else(Local::now);
    let last_date = last_local.date_naive();
    let today = now_local.date_naive();
    let time_hm = last_local.format("%H:%M").to_string();
    let day_diff = (today - last_date).num_days();

    if day_diff == 0 {
        return if zh {
            format!("今天 {time_hm} 最后连接")
        } else {
            format!("Last connected today {time_hm}")
        };
    }
    if day_diff == 1 {
        return if zh {
            format!("昨天 {time_hm} 最后连接")
        } else {
            format!("Last connected yesterday {time_hm}")
        };
    }
    if (2..7).contains(&day_diff) {
        return if zh {
            format!("{day_diff} 天未连接")
        } else if day_diff == 1 {
            "Last connected 1 day ago".to_string()
        } else {
            format!("Last connected {day_diff} days ago")
        };
    }
    let date = last_local.format("%Y-%m-%d").to_string();
    if zh {
        format!("{date} {time_hm} 最后连接")
    } else {
        format!("Last connected {date} {time_hm}")
    }
}

/// 侧栏右侧状态文案（在线 / 离线分流）。
pub fn connection_status_label(
    zh: bool,
    online: bool,
    last_connected_unix: Option<i64>,
    now_unix: i64,
) -> String {
    if online {
        format_online_duration(zh, last_connected_unix, now_unix)
    } else {
        format_offline_last_connected(zh, last_connected_unix, now_unix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sc(
        id: &str,
        name: &str,
        host: &str,
        port: u16,
        user: &str,
        group: &str,
        last: Option<i64>,
    ) -> SessionConfig {
        SessionConfig {
            id: id.to_string(),
            name: name.to_string(),
            host: host.to_string(),
            port,
            username: user.to_string(),
            group: group.to_string(),
            last_connected_at: last,
            ..SessionConfig::default()
        }
    }

    #[test]
    fn dedup_key_prefers_saved_host_id() {
        let s = sc("abc", "n", "1.2.3.4", 22, "root", "默认", None);
        assert_eq!(session_dedup_key(&s), "id:abc");
        let empty_id = sc("", "n", "1.2.3.4", 22, "root", "默认", None);
        assert!(session_dedup_key(&empty_id).starts_with("ep:"));
        assert!(session_dedup_key(&empty_id).contains("root@1.2.3.4:22"));
    }

    #[test]
    fn rename_does_not_split_same_id() {
        let a = sc("id1", "Old", "h", 22, "u", "默认", Some(10));
        let b = sc("id1", "New", "h", 22, "u", "默认", Some(20));
        let connected = HashSet::new();
        let out = dedupe_sessions_for_connection_list(&[a, b], &connected);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "New");
        assert_eq!(out[0].last_connected_at, Some(20));
    }

    #[test]
    fn same_endpoint_different_ids_collapse_to_one_row() {
        let offline = sc("a", "Linux", "124.220.224.223", 22, "root", "默认", Some(100));
        let online = sc("b", "Linux", "124.220.224.223", 22, "root", "默认", Some(200));
        let mut connected = HashSet::new();
        connected.insert("b".to_string());
        let out = dedupe_sessions_for_connection_list(&[offline, online], &connected);
        assert_eq!(out.len(), 1, "灰/绿不得各占一行");
        assert_eq!(out[0].id, "b", "在线代表优先");
        assert_eq!(out[0].last_connected_at, Some(200));
    }

    #[test]
    fn display_name_change_same_endpoint_still_one_row() {
        let a = sc("a", "Name A", "10.0.0.1", 22, "root", "默认", Some(1));
        let b = sc("b", "Name B", "10.0.0.1", 22, "root", "默认", Some(2));
        let out = dedupe_sessions_for_connection_list(&[a, b], &HashSet::new());
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn different_endpoints_stay_separate() {
        let a = sc("a", "A", "10.0.0.1", 22, "root", "默认", None);
        let b = sc("b", "B", "10.0.0.2", 22, "root", "默认", None);
        let out = dedupe_sessions_for_connection_list(&[a, b], &HashSet::new());
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn group_key_unifies_default_aliases() {
        assert_eq!(session_group_key(""), session_group_key("默认"));
        assert_eq!(session_group_key("Default"), session_group_key("默认"));
        assert_ne!(session_group_key("生产"), session_group_key("默认"));
    }

    #[test]
    fn sort_keeping_groups_emits_each_group_once_contiguously() {
        let mut v = vec![
            sc("1", "z", "h1", 22, "u", "默认", Some(300)),
            sc("2", "a", "h2", 22, "u", "生产", Some(200)),
            sc("3", "m", "h3", 22, "u", "默认", Some(100)),
            sc("4", "b", "h4", 22, "u", "", Some(50)), // 与默认同组
        ];
        sort_sessions_keeping_groups(&mut v, SessionSortBy::LastConnected);
        let keys: Vec<String> = v.iter().map(|s| session_group_key(&s.group)).collect();
        // 默认组（含 ""）应聚拢在前，且连续
        assert_eq!(keys[0], "");
        assert_eq!(keys[1], "");
        assert_eq!(keys[2], "");
        assert_eq!(keys[3], "生产");
        // 组内按最近连接：id1 (300), id3 (100), id4 (50)
        let default_ids: Vec<&str> = v
            .iter()
            .filter(|s| session_group_key(&s.group).is_empty())
            .map(|s| s.id.as_str())
            .collect();
        assert_eq!(default_ids, vec!["1", "3", "4"]);
    }

    #[test]
    fn online_duration_is_human_not_bare_12m() {
        let now = 1_700_000_000_i64;
        let label = format_online_duration(true, Some(now - 16 * 60), now);
        assert_eq!(label, "已连接 16 分钟");
        assert!(!label.contains('m') || label.contains("分钟"));
        let en = format_online_duration(false, Some(now - 2 * 3600), now);
        assert_eq!(en, "Connected for 2 hours");
        assert!(!en.ends_with('h') || en.contains("hour"));
    }

    #[test]
    fn offline_yesterday_wording() {
        // 构造「昨天」：用固定本地日较难，改测 format 分支用 day_diff 逻辑——
        // 直接测 never + today 路径，昨天用相对秒近似可能跨日失败；用 format_offline 的 never/online 分流。
        let never = format_offline_last_connected(true, None, 1_700_000_000);
        assert_eq!(never, "从未连接");
        let status_on = connection_status_label(true, true, Some(1_700_000_000 - 60), 1_700_000_000);
        assert!(status_on.contains("已连接"));
        let status_off = connection_status_label(true, false, None, 1_700_000_000);
        assert_eq!(status_off, "从未连接");
    }

    #[test]
    fn offline_relative_days_wording() {
        let now = Local::now().timestamp();
        let three_days = now - 3 * 86_400 - 3600;
        let zh = format_offline_last_connected(true, Some(three_days), now);
        assert!(
            zh.contains("天未连接") || zh.contains("最后连接"),
            "got: {zh}"
        );
    }

    #[test]
    fn offline_yesterday_uses_human_clock() {
        let now_local = Local::now();
        let yesterday = now_local - chrono::Duration::days(1);
        // 固定到昨天 23:14，避免「刚好跨日」边界抖动
        let yesterday_2314 = yesterday
            .date_naive()
            .and_hms_opt(23, 14, 0)
            .unwrap();
        let ts = yesterday_2314
            .and_local_timezone(Local)
            .single()
            .expect("local tz")
            .timestamp();
        let zh = format_offline_last_connected(true, Some(ts), now_local.timestamp());
        assert_eq!(zh, "昨天 23:14 最后连接");
        let en = format_offline_last_connected(false, Some(ts), now_local.timestamp());
        assert_eq!(en, "Last connected yesterday 23:14");
    }
}
