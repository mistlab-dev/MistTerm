//! `mist import xshell|finalshell <路径>` — 从 Xshell / FinalShell 导入会话。

use anyhow::Result;
use std::path::Path;

use super::CliContext;
use crate::core::foreign_import::{
    candidate_to_session, detect_source, is_already_imported, parse_path, ForeignImportOptions,
    ForeignSource, XshellAccount,
};

pub struct ImportArgs<'a> {
    /// `None` 表示自动判断。
    pub source: Option<ForeignSource>,
    pub path: &'a Path,
    pub dry_run: bool,
    pub windows_user: Option<String>,
    pub windows_sid: Option<String>,
}

pub fn run_import(ctx: &mut CliContext, args: ImportArgs<'_>) -> Result<i32> {
    if !args.path.exists() {
        anyhow::bail!("找不到：{}", args.path.display());
    }
    let source = match args.source.or_else(|| detect_source(args.path)) {
        Some(s) => s,
        None => anyhow::bail!(
            "看不出是 Xshell 还是 FinalShell 的文件，请用 `mist import xshell <路径>` 或 `mist import finalshell <路径>` 指明"
        ),
    };
    let mut opts = ForeignImportOptions::default();
    if let Some(sid) = args.windows_sid.filter(|s| !s.trim().is_empty()) {
        opts.xshell_accounts.push(XshellAccount {
            user: args.windows_user.unwrap_or_default(),
            sid: sid.trim().to_string(),
        });
    }
    // 主密码只从环境变量读，避免留在命令历史里
    if let Ok(mp) = std::env::var("MIST_XSHELL_MASTER_PASSWORD") {
        if !mp.is_empty() {
            opts.xshell_master_password = Some(mp);
        }
    }

    println!("从 {} 导入：{}", source.label(), args.path.display());
    let parsed = parse_path(source, args.path, &opts)
        .map_err(|e| anyhow::anyhow!("读取失败：{e}"))?;
    for w in &parsed.warnings {
        eprintln!("[提醒] {w}");
    }

    let mut names: Vec<String> = ctx.sessions.list_sessions().iter().map(|s| s.name.clone()).collect();
    let (mut added, mut skipped, mut no_password) = (0, 0, 0);
    for c in &parsed.candidates {
        let label = format!("[{}] {} → {}", c.group, c.name, c.display_target());
        if let Some(reason) = &c.skip_reason {
            println!("  跳过 {label}：{reason}");
            skipped += 1;
            continue;
        }
        if is_already_imported(c, ctx.sessions.list_sessions()) {
            println!("  跳过 {label}：已经导入过");
            skipped += 1;
            continue;
        }
        let pw = if c.password.is_some() { "密码已导入" } else { no_password += 1; "没有密码" };
        println!("  导入 {label}（{pw}）");
        for n in &c.notes {
            println!("      {n}");
        }
        if !args.dry_run {
            let s = candidate_to_session(c, &names);
            names.push(s.name.clone());
            ctx.sessions.add_session(s);
        }
        added += 1;
    }
    if args.dry_run {
        println!("\n只是预览（--dry-run），没有保存。可导入 {added} 个，跳过 {skipped} 个。");
    } else {
        println!("\n导入完成：新增 {added} 个会话，跳过 {skipped} 个。");
        if no_password > 0 {
            println!("其中 {no_password} 个没有密码：请在桌面版里右键会话 →「编辑」填上密码或选择私钥。");
        }
    }
    Ok(0)
}
