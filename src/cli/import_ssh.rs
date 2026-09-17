//! `mist import-ssh-config` — 从 ~/.ssh/config 导入会话。

use anyhow::Result;
use std::path::PathBuf;

use crate::core::ssh_config_importer::{
    default_ssh_config_path, parse_ssh_config_file, SshConfigCandidate,
};
use crate::core::session::SessionConfig;
use super::CliContext;

pub fn run_import(
    ctx: &mut CliContext,
    file_path: Option<PathBuf>,
    dry_run: bool,
    overwrite: bool,
) -> Result<i32> {
    let path = file_path.unwrap_or_else(default_ssh_config_path);
    if !path.exists() {
        anyhow::bail!("SSH 配置文件不存在: {}", path.display());
    }

    println!("正在解析 SSH 配置: {}", path.display());
    let parsed = parse_ssh_config_file(&path)
        .map_err(|e| anyhow::anyhow!("读取文件失败: {e}"))?;

    for w in &parsed.warnings {
        eprintln!("[警告] {w}");
    }

    let importable: Vec<&SshConfigCandidate> = parsed
        .candidates
        .iter()
        .filter(|c| c.importable())
        .collect();

    if importable.is_empty() {
        println!("未检测到可导入的 Host 块。");
        return Ok(0);
    }

    println!("检测到 {} 个可导入候选配置:", importable.len());
    let mut added = 0;
    let mut skipped = 0;

    for c in importable {
        let marker = c.marker_key();
        let existing = ctx
            .sessions
            .list_sessions()
            .iter()
            .find(|s| s.name == c.host_alias || s.ssh_config_marker.as_deref() == Some(&marker));

        if let Some(ex) = existing {
            if !overwrite {
                println!("  [跳过] 已存在同名或相同配置: {} -> {}", c.host_alias, ex.name);
                skipped += 1;
                continue;
            }
        }

        println!("  [准备导入] {} -> {}", c.host_alias, c.display_target());

        if !dry_run {
            let mut s = SessionConfig::default();
            s.id = uuid::Uuid::new_v4().to_string();
            s.name = c.host_alias.clone();
            s.host = c.hostname.clone().unwrap_or_default();
            s.port = c.port;
            s.username = if c.username.is_empty() {
                std::env::var("USER").unwrap_or_else(|_| "root".to_string())
            } else {
                c.username.clone()
            };
            s.private_key_path = c.identity_file.clone();
            s.proxy_jump = c.proxy_jump.clone();
            s.proxy_command = c.proxy_command.clone();
            s.ssh_config_marker = Some(marker);
            s.group = "SSH Config".to_string();

            ctx.sessions.add_session(s);
            added += 1;
        }
    }

    if dry_run {
        println!("\n--dry-run 模式：未实际写入会话。");
    } else {
        println!("\n导入完成！新增 {} 个会话，跳过 {} 个。", added, skipped);
    }

    Ok(0)
}
