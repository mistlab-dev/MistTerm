//! `mist frag` — 片段管理与执行（list / run）。

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashMap;

use crate::core::fragment::{expand_fragment_command_stages, FragmentManager};
use super::exec;
use super::CliContext;

#[derive(Serialize)]
struct FragRow {
    id: String,
    title: String,
    category: String,
    command: String,
    tags: Vec<String>,
    variables: Vec<String>,
}

/// 加载默认存储路径的片段管理器
fn load_default_fragments() -> FragmentManager {
    let path = FragmentManager::default_config_path();
    FragmentManager::load(&path).unwrap_or_default()
}

/// `mist frag list` — 列出所有保存的命令片段
pub fn run_list(json: bool) -> Result<i32> {
    let mgr = load_default_fragments();
    let fragments = mgr.get_all();

    if json {
        let rows: Vec<FragRow> = fragments
            .iter()
            .map(|f| FragRow {
                id: f.id.clone(),
                title: f.title.clone(),
                category: f.category.clone(),
                command: f.command.clone(),
                tags: f.tags.clone(),
                variables: f.variables.iter().map(|v| v.name.clone()).collect(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(0);
    }

    if fragments.is_empty() {
        println!("未找到任何命令片段");
        return Ok(0);
    }

    println!("{:<24} {:<16} {:<16} {}", "标题", "分类", "标签", "命令");
    println!("{}", "-".repeat(80));
    for f in fragments {
        let tags = if f.tags.is_empty() {
            "—".to_string()
        } else {
            f.tags.join(",")
        };
        let cmd_preview = if f.command.len() > 30 {
            format!("{}...", &f.command[..30])
        } else {
            f.command.clone()
        };
        println!(
            "{:<24} {:<16} {:<16} {}",
            f.title, f.category, tags, cmd_preview
        );
    }

    Ok(0)
}

/// 解析形如 `key=value` 的命令行变量
fn parse_vars(vars: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for v in vars {
        if let Some((k, val)) = v.split_once('=') {
            map.insert(k.trim().to_string(), val.trim().to_string());
        }
    }
    map
}

/// `mist frag run <name_or_id> [target]` — 运行片段
pub fn run_run(
    ctx: &mut CliContext,
    name_or_id: &str,
    target: Option<&str>,
    group: Option<&str>,
    all_targets: bool,
    serial: bool,
    parallel: usize,
    vars: &[String],
    json: bool,
) -> Result<i32> {
    let mgr = load_default_fragments();
    let frag = mgr
        .get_all()
        .iter()
        .find(|f| f.title == name_or_id || f.id == name_or_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("未找到片段: {name_or_id}"))?;

    let cli_vars = parse_vars(vars);
    let mut final_vars = frag.variable_defaults();
    final_vars.extend(cli_vars);

    if all_targets || group.is_some() {
        // 批量执行模式
        let expanded = expand_fragment_command_stages(&frag.command, None, &final_vars)
            .map_err(|e| anyhow::anyhow!("展开片段表达式失败: {e}"))?;

        exec::run_batch(
            ctx,
            group,
            all_targets,
            serial,
            parallel,
            &expanded,
            json,
        )
    } else {
        // 单机执行模式
        let t = target.ok_or_else(|| anyhow::anyhow!("缺少目标 target（或使用 --group/--all）"))?;
        let session = ctx.resolve_target(t)?;

        let expanded = expand_fragment_command_stages(&frag.command, Some(&session), &final_vars)
            .map_err(|e| anyhow::anyhow!("展开片段表达式失败: {e}"))?;

        exec::run_single(ctx, t, &expanded, json)
    }
}
