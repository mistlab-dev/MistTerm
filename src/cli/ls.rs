//! `mist ls` — 列出已保存会话。

use anyhow::Result;
use serde::Serialize;

use super::CliContext;

#[derive(Serialize)]
struct LsRow {
    id: String,
    name: String,
    group: String,
    host: String,
    port: u16,
    username: String,
    color_tag: String,
    last_connected_at: Option<i64>,
}

pub fn run(ctx: &CliContext, group: Option<&str>, json: bool) -> Result<()> {
    let mut sessions: Vec<_> = ctx.sessions.get_sessions().to_vec();
    sessions.sort_by(|a, b| {
        // 按 group 再按 name 排
        a.group.cmp(&b.group).then(a.name.cmp(&b.name))
    });
    let filtered: Vec<_> = sessions
        .into_iter()
        .filter(|s| group.map(|g| s.group == g).unwrap_or(true))
        .collect();

    if json {
        let rows: Vec<LsRow> = filtered
            .iter()
            .map(|s| LsRow {
                id: s.id.clone(),
                name: s.name.clone(),
                group: s.group.clone(),
                host: s.host.clone(),
                port: s.port,
                username: s.username.clone(),
                color_tag: s.color_tag.clone(),
                last_connected_at: s.last_connected_at,
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }

    if filtered.is_empty() {
        println!("没有匹配的会话");
        return Ok(());
    }

    // 人类可读表格
    let mut last_group = String::new();
    for s in &filtered {
        if s.group != last_group {
            if !last_group.is_empty() {
                println!();
            }
            println!("[{}]", s.group);
            last_group = s.group.clone();
        }
        let last = s
            .last_connected_at
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| "—".to_string());
        println!(
            "  {:<24} {:<16} {:<22} last: {}",
            s.name,
            format!("{}@{}", s.username, s.host),
            format!(":{}", s.port),
            last
        );
    }
    Ok(())
}
