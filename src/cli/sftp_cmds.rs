//! `mist get / put / rls` — 基于 SFTP 的文件传输。

use anyhow::Result;
use crate::ssh::{SftpClient, SshClient};
use serde::Serialize;
use std::path::{Path, PathBuf};

use super::context::split_target_path;
use super::CliContext;

#[derive(Serialize)]
struct RlsRow {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    permissions: String,
    modified: String,
}

fn open_sftp(ctx: &mut CliContext, target: &str) -> Result<(SftpClient, SshClient, String)> {
    let session = ctx.resolve_target(target)?;
    let config = ctx.ssh_config(&session)?;
    let mut client = SshClient::new(config);
    client
        .connect()
        .map_err(|e| anyhow::anyhow!("连接失败 {}: {}", session.name, e))?;
    ctx.mark_connected(&session);
    let sftp = SftpClient::new(client.get_session())
        .map_err(|e| anyhow::anyhow!("打开 SFTP 失败: {e}"))?;
    Ok((sftp, client, session.name))
}

/// `mist rls target:/path` — 列远端目录。
pub fn run_rls(ctx: &mut CliContext, spec: &str, json: bool) -> Result<i32> {
    let (target, remote) = split_target_path(spec)
        .ok_or_else(|| anyhow::anyhow!("格式应为 <target>:<path>，例如 prod:/var/log"))?;
    let (sftp, _client, _name) = open_sftp(ctx, &target)?;
    let entries = sftp
        .list_dir(Path::new(&remote))
        .map_err(|e| anyhow::anyhow!("列目录失败 {remote}: {e}"))?;

    if json {
        let rows: Vec<RlsRow> = entries
            .iter()
            .map(|e| RlsRow {
                name: e.name.clone(),
                path: e.path.display().to_string(),
                is_dir: e.is_dir,
                size: e.size,
                permissions: e.permissions.clone(),
                modified: e.modified.format("%Y-%m-%d %H:%M:%S").to_string(),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(0);
    }

    for e in &entries {
        let marker = if e.is_dir { "d" } else { "-" };
        println!(
            "{}{} {:>10} {} {}",
            marker,
            &e.permissions[1..],
            e.size_human(),
            e.modified.format("%Y-%m-%d %H:%M"),
            e.name
        );
    }
    Ok(0)
}

/// `mist get target:/remote local` — 下载。
pub fn run_get(ctx: &mut CliContext, spec: &str, local: &str) -> Result<i32> {
    let (target, remote) = split_target_path(spec)
        .ok_or_else(|| anyhow::anyhow!("格式应为 <target>:<remote>，例如 prod:/var/log/app.log"))?;
    let (sftp, _client, name) = open_sftp(ctx, &target)?;

    // local 是目录时，用远端文件名拼上
    let local_path = {
        let p = PathBuf::from(local);
        if p.is_dir() {
            let fname = Path::new(&remote)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| "download".to_string());
            p.join(fname)
        } else {
            p
        }
    };

    let n = sftp
        .download(Path::new(&remote), &local_path)
        .map_err(|e| anyhow::anyhow!("下载失败 {name}:{remote} → {}: {e}", local_path.display()))?;
    println!("{} → {} ({} 字节)", remote, local_path.display(), n);
    Ok(0)
}

/// `mist put local target:/remote` — 上传。
pub fn run_put(ctx: &mut CliContext, local: &str, spec: &str) -> Result<i32> {
    let (target, remote) = split_target_path(spec)
        .ok_or_else(|| anyhow::anyhow!("格式应为 <target>:<remote>，例如 prod:/tmp/app.tar.gz"))?;
    let local_path = PathBuf::from(local);
    if !local_path.is_file() {
        anyhow::bail!("本地文件不存在: {local}");
    }
    let (sftp, _client, name) = open_sftp(ctx, &target)?;

    // remote 以 / 结尾时拼本地文件名
    let remote_path = if remote.ends_with('/') {
        let fname = local_path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| "upload".to_string());
        format!("{remote}{fname}")
    } else {
        remote
    };

    let n = sftp
        .upload(&local_path, Path::new(&remote_path))
        .map_err(|e| anyhow::anyhow!("上传失败 {local} → {name}:{remote_path}: {e}"))?;
    println!("{} → {} ({} 字节)", local_path.display(), remote_path, n);
    Ok(0)
}
