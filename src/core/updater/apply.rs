//! 下载、校验、解压、冒烟验证、替换、备份与回退。
//!
//! 通用流程：下载到缓存目录 → 核对 SHA-256 → 只解压需要的程序文件 → 复制到安装目录的临时名
//! → 运行 `新程序 --version` 确认能启动且版本正确 → 替换（旧文件放进 `.mist-update-backup/`）。
//!
//! - Linux：硬链接备份旧文件后 `rename()` 原子替换；正在运行的旧进程不受影响。
//! - Windows 便携版：正在运行的 exe 不能覆盖但可以改名，所以先把旧文件改名移进备份目录，再放入新文件。
//! - Windows 安装版：校验后以独立进程静默运行新的 setup.exe，由安装程序关闭并（可选）重新打开 Mist。
//! - 任一步失败都会把已经换掉的文件换回来，保证当前版本不受影响。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::check::{InstallPlan, UpdateInfo};
use super::error::UpdateError;
use super::fetch::Fetcher;
use super::install_kind::InstallKind;
use super::lock::UpdateLock;
use super::manifest::{PlatformAsset, KIND_TAR_GZ, KIND_ZIP};
use super::verify;

/// 安装目录中的备份目录名（只保留一份上一个版本）。
pub const BACKUP_DIR: &str = ".mist-update-backup";
const BACKUP_TMP_DIR: &str = ".mist-update-backup.tmp";
const BACKUP_OLD_PREFIX: &str = ".mist-update-backup.old-";
const STAGED_PREFIX: &str = ".mist-update-new-";
const BACKUP_META: &str = "backup.json";

/// 单个解压文件的大小上限（防止压缩炸弹）。
const MAX_EXTRACTED_FILE: u64 = 1024 * 1024 * 1024;

/// 各平台需要更新的程序文件（压缩包内与安装目录内同名）。
pub fn program_files() -> &'static [&'static str] {
    if cfg!(windows) {
        &["Mist.exe", "mist-cli.exe", "mist.cmd"]
    } else {
        &["Mist", "mist"]
    }
}

fn needs_smoke_test(name: &str) -> bool {
    !name.ends_with(".cmd")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyStage {
    Downloading { done: u64, total: u64 },
    Verifying,
    Installing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// 文件已替换，重启 Mist 后生效。
    Installed { version: String, exe: PathBuf },
    /// 已启动 Windows 安装程序；调用方应尽快退出。
    InstallerStarted { version: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BackupMeta {
    version: String,
    #[serde(default)]
    saved_at: String,
}

/// 下载并校验安装包（已有缓存且校验和一致时直接复用）。地址按清单顺序尝试。
pub fn download_asset(
    asset: &PlatformAsset,
    version: &str,
    progress: &mut dyn FnMut(ApplyStage),
    cancel: &AtomicBool,
) -> Result<PathBuf, UpdateError> {
    let dir = super::paths::cache_dir().join(sanitize_component(version));
    std::fs::create_dir_all(&dir).map_err(|e| UpdateError::Install(e.to_string()))?;
    let dest = dir.join(&asset.name);
    if dest.is_file() {
        if let Ok(sum) = verify::sha256_file(&dest) {
            if verify::sha256_matches(&asset.sha256, &sum) {
                log::info!("updater: reusing cached {}", dest.display());
                progress(ApplyStage::Downloading {
                    done: asset.size,
                    total: asset.size,
                });
                return Ok(dest);
            }
        }
        let _ = std::fs::remove_file(&dest);
    }
    let part = dir.join(format!("{}.part", asset.name));
    let fetcher = Fetcher::for_download()?;
    let mut last_err = UpdateError::Network("no download url".into());
    for url in &asset.urls {
        let mut on_bytes = |done: u64, total: u64| progress(ApplyStage::Downloading { done, total });
        match fetcher.download_to(url, &part, asset.size, &mut on_bytes, cancel) {
            Ok(sum) if verify::sha256_matches(&asset.sha256, &sum) => {
                progress(ApplyStage::Verifying);
                std::fs::rename(&part, &dest).map_err(|e| UpdateError::Install(e.to_string()))?;
                return Ok(dest);
            }
            Ok(_) => {
                log::warn!("updater: checksum mismatch from {url}; trying next source");
                last_err = UpdateError::Checksum;
            }
            Err(UpdateError::Cancelled) => {
                let _ = std::fs::remove_file(&part);
                return Err(UpdateError::Cancelled);
            }
            Err(e) => {
                log::warn!("updater: download from {url} failed: {e}");
                // 校验和 / 大小错误比后面的网络错误更值得报告。
                if !matches!(
                    (&last_err, &e),
                    (UpdateError::Checksum | UpdateError::SizeMismatch, UpdateError::Network(_))
                ) {
                    last_err = e;
                }
            }
        }
        let _ = std::fs::remove_file(&part);
    }
    Err(last_err)
}

/// 执行一键更新。调用方需已确认 `info.plan` 为 [`InstallPlan::Auto`]。
pub fn apply_update(
    info: &UpdateInfo,
    exe: &Path,
    relaunch_after_installer: bool,
    progress: &mut dyn FnMut(ApplyStage),
    cancel: &AtomicBool,
) -> Result<ApplyOutcome, UpdateError> {
    let InstallPlan::Auto { kind, asset, .. } = &info.plan else {
        return Err(UpdateError::NotAutoInstallable("manual install required".into()));
    };
    let _lock = UpdateLock::acquire()?;
    let version = info.manifest.version.clone();
    let archive = download_asset(asset, &version, progress, cancel)?;
    progress(ApplyStage::Installing);
    let outcome = match kind {
        InstallKind::LinuxPortable { dir } | InstallKind::WindowsPortable { dir } => {
            install_from_archive(&archive, &asset.kind, dir, exe, &version)?;
            ApplyOutcome::Installed {
                version: version.clone(),
                exe: exe.to_path_buf(),
            }
        }
        InstallKind::WindowsInstaller { .. } => {
            launch_installer(&archive, relaunch_after_installer)?;
            ApplyOutcome::InstallerStarted {
                version: version.clone(),
            }
        }
        other => {
            return Err(UpdateError::NotAutoInstallable(format!("{other:?}")));
        }
    };
    let mut state = super::state::UpdateState::load();
    if matches!(outcome, ApplyOutcome::Installed { .. }) {
        state.installed_pending_restart = Some(version);
        let _ = state.save();
    }
    Ok(outcome)
}

/// 从压缩包更新 `dir` 中的程序文件（只更新目录里已有的程序，以及当前正在运行的这个）。
pub fn install_from_archive(
    archive: &Path,
    archive_kind: &str,
    dir: &Path,
    exe: &Path,
    new_version: &str,
) -> Result<(), UpdateError> {
    let exe_name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| UpdateError::Install("bad exe path".into()))?
        .to_string();
    if !program_files().contains(&exe_name.as_str()) {
        return Err(UpdateError::NotAutoInstallable(format!(
            "unexpected program name {exe_name:?}"
        )));
    }
    let targets: Vec<&str> = program_files()
        .iter()
        .copied()
        .filter(|n| *n == exe_name || dir.join(n).is_file())
        .collect();

    let extract_dir = archive
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("extracted");
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir).map_err(|e| UpdateError::Install(e.to_string()))?;
    let extracted = match archive_kind {
        KIND_TAR_GZ => extract_tar_gz(archive, &extract_dir, &targets)?,
        KIND_ZIP => extract_zip(archive, &extract_dir, &targets)?,
        other => return Err(UpdateError::NotAutoInstallable(format!("archive kind {other:?}"))),
    };
    for t in &targets {
        if !extracted.iter().any(|(n, _)| n == t) {
            return Err(UpdateError::Install(format!("{t} missing from update package")));
        }
    }
    let old_version = super::APP_VERSION.to_string();
    let result = install_files(dir, &extracted, &old_version, new_version);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

fn entry_basename_if_wanted<'a>(path: &Path, wanted: &[&'a str]) -> Option<&'a str> {
    // 只接受 `name` 或 `顶层目录/name`，不做通用解压，从根本上避免路径穿越。
    let comps: Vec<_> = path.components().collect();
    if comps.is_empty() || comps.len() > 2 {
        return None;
    }
    if !comps.iter().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    wanted.iter().copied().find(|w| *w == name)
}

pub fn extract_tar_gz(
    archive: &Path,
    dest: &Path,
    wanted: &[&str],
) -> Result<Vec<(String, PathBuf)>, UpdateError> {
    let f = std::fs::File::open(archive).map_err(|e| UpdateError::Install(e.to_string()))?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let mut out = Vec::new();
    let entries = ar.entries().map_err(|e| UpdateError::Install(format!("tar: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| UpdateError::Install(format!("tar: {e}")))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()
            .map_err(|e| UpdateError::Install(format!("tar: {e}")))?
            .into_owned();
        let Some(name) = entry_basename_if_wanted(&path, wanted) else {
            continue;
        };
        if out.iter().any(|(n, _): &(String, PathBuf)| n == name) {
            return Err(UpdateError::Install(format!("duplicate {name} in package")));
        }
        let target = dest.join(name);
        write_limited(&mut entry, &target)?;
        out.push((name.to_string(), target));
    }
    Ok(out)
}

pub fn extract_zip(
    archive: &Path,
    dest: &Path,
    wanted: &[&str],
) -> Result<Vec<(String, PathBuf)>, UpdateError> {
    let f = std::fs::File::open(archive).map_err(|e| UpdateError::Install(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|e| UpdateError::Install(format!("zip: {e}")))?;
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|e| UpdateError::Install(format!("zip: {e}")))?;
        if !file.is_file() {
            continue;
        }
        let Some(path) = file.enclosed_name() else {
            continue;
        };
        let Some(name) = entry_basename_if_wanted(&path, wanted) else {
            continue;
        };
        if out.iter().any(|(n, _): &(String, PathBuf)| n == name) {
            return Err(UpdateError::Install(format!("duplicate {name} in package")));
        }
        let target = dest.join(name);
        write_limited(&mut file, &target)?;
        out.push((name.to_string(), target));
    }
    Ok(out)
}

fn write_limited<R: std::io::Read>(reader: &mut R, target: &Path) -> Result<(), UpdateError> {
    let mut f = std::fs::File::create(target).map_err(|e| UpdateError::Install(e.to_string()))?;
    let n = std::io::copy(&mut std::io::Read::take(reader, MAX_EXTRACTED_FILE + 1), &mut f)
        .map_err(|e| UpdateError::Install(e.to_string()))?;
    if n > MAX_EXTRACTED_FILE {
        return Err(UpdateError::Install("file in package is too large".into()));
    }
    Ok(())
}

/// 运行 `程序 --version`，确认输出里有期望的版本号。
pub fn smoke_test(program: &Path, expected_version: &str) -> Result<(), UpdateError> {
    let mut child = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| UpdateError::SmokeTest(format!("cannot start: {e}")))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(UpdateError::SmokeTest("timed out".into()));
            }
            Err(e) => return Err(UpdateError::SmokeTest(e.to_string())),
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| UpdateError::SmokeTest(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ok = out.status.success()
        && stdout
            .split_whitespace()
            .any(|w| w.trim_start_matches('v') == expected_version);
    if ok {
        Ok(())
    } else {
        Err(UpdateError::SmokeTest(format!(
            "expected version {expected_version}, got {:?}",
            stdout.trim()
        )))
    }
}

/// 把 `items`（文件名 → 新文件）装进 `dir`，旧文件进入备份目录。失败时全部恢复原状。
pub fn install_files(
    dir: &Path,
    items: &[(String, PathBuf)],
    old_version: &str,
    new_version: &str,
) -> Result<(), UpdateError> {
    // 1. 复制到安装目录内的临时名（同一文件系统，才能原子改名），并做冒烟验证。
    let mut staged: Vec<(String, PathBuf)> = Vec::new();
    let cleanup_staged = |staged: &[(String, PathBuf)]| {
        for (_, p) in staged {
            let _ = std::fs::remove_file(p);
        }
    };
    for (name, src) in items {
        let dst = dir.join(format!("{STAGED_PREFIX}{name}"));
        let _ = std::fs::remove_file(&dst);
        if let Err(e) = copy_synced(src, &dst) {
            cleanup_staged(&staged);
            return Err(map_io(dir, e));
        }
        staged.push((name.clone(), dst));
    }
    for (name, path) in &staged {
        if needs_smoke_test(name) {
            if let Err(e) = smoke_test(path, new_version) {
                cleanup_staged(&staged);
                return Err(e);
            }
        }
    }

    // 2. 新的备份目录。
    let bak_tmp = dir.join(BACKUP_TMP_DIR);
    let _ = std::fs::remove_dir_all(&bak_tmp);
    if let Err(e) = std::fs::create_dir_all(&bak_tmp) {
        cleanup_staged(&staged);
        return Err(map_io(dir, e));
    }

    // 3. 逐个替换；记录已完成的，出错时倒序恢复。
    let mut done: Vec<(String, bool)> = Vec::new(); // (name, had_old)
    let mut failure: Option<UpdateError> = None;
    for (name, staged_path) in &staged {
        let target = dir.join(name);
        let backup = bak_tmp.join(name);
        let had_old = target.exists();
        if had_old {
            if let Err(e) = move_old_to_backup(&target, &backup) {
                failure = Some(map_io(dir, e));
                break;
            }
        }
        if let Err(e) = std::fs::rename(staged_path, &target) {
            // 这一项本身回滚：Windows 上旧文件已被移走，要放回去。
            if had_old {
                restore_from_backup(&backup, &target);
            }
            failure = Some(map_io(dir, e));
            break;
        }
        done.push((name.clone(), had_old));
    }
    if let Some(err) = failure {
        for (name, had_old) in done.iter().rev() {
            let target = dir.join(name);
            if *had_old {
                restore_from_backup(&bak_tmp.join(name), &target);
            } else {
                let _ = std::fs::remove_file(&target);
            }
        }
        cleanup_staged(&staged);
        let _ = std::fs::remove_dir_all(&bak_tmp);
        return Err(err);
    }

    // 4. 记录备份对应的版本，再替换掉上一份备份。
    let meta = BackupMeta {
        version: old_version.to_string(),
        saved_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Ok(body) = serde_json::to_vec_pretty(&meta) {
        let _ = std::fs::write(bak_tmp.join(BACKUP_META), body);
    }
    let bak = dir.join(BACKUP_DIR);
    if bak.exists() && std::fs::remove_dir_all(&bak).is_err() {
        // Windows：上一份备份里的旧 exe 可能还在运行，删不掉就先改名，下次启动再清理。
        let parked = dir.join(format!("{BACKUP_OLD_PREFIX}{}", chrono::Utc::now().timestamp_millis()));
        let _ = std::fs::rename(&bak, parked);
    }
    if let Err(e) = std::fs::rename(&bak_tmp, &bak) {
        log::warn!("updater: could not finalize backup dir: {e}");
    }
    log::info!("updater: installed {new_version} into {}", dir.display());
    Ok(())
}

fn map_io(dir: &Path, e: std::io::Error) -> UpdateError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        UpdateError::NotWritable(dir.to_path_buf())
    } else {
        UpdateError::Install(e.to_string())
    }
}

fn copy_synced(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::copy(src, dst)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::File::open(dst)?.sync_all()?;
    Ok(())
}

/// Unix：硬链接（失败则复制）到备份目录，原文件留在原处，随后由 rename 原子覆盖。
/// Windows：正在运行的 exe 不能覆盖但能改名，直接移进备份目录。
fn move_old_to_backup(target: &Path, backup: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        if std::fs::hard_link(target, backup).is_err() {
            std::fs::copy(target, backup)?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::rename(target, backup)
    }
}

fn restore_from_backup(backup: &Path, target: &Path) {
    #[cfg(unix)]
    {
        // 原子地把备份放回原位（备份是硬链接或副本，原文件可能已被新文件覆盖）。
        let tmp = target.with_file_name(format!(
            "{STAGED_PREFIX}restore-{}",
            target.file_name().and_then(|n| n.to_str()).unwrap_or("x")
        ));
        if std::fs::copy(backup, &tmp).is_ok() {
            let _ = std::fs::rename(&tmp, target);
        }
    }
    #[cfg(not(unix))]
    {
        if target.exists() {
            let _ = std::fs::remove_file(target);
        }
        let _ = std::fs::rename(backup, target);
    }
}

/// 备份目录里保存的上一个版本号。
pub fn backup_version(dir: &Path) -> Option<String> {
    let body = std::fs::read(dir.join(BACKUP_DIR).join(BACKUP_META)).ok()?;
    serde_json::from_slice::<BackupMeta>(&body).ok().map(|m| m.version)
}

/// 回到上一个版本（再执行一次就回到回退前的版本）。返回回退后的版本号。
pub fn rollback(exe: &Path) -> Result<String, UpdateError> {
    let dir = exe
        .parent()
        .ok_or_else(|| UpdateError::Install("bad exe path".into()))?;
    let _lock = UpdateLock::acquire()?;
    let bak = dir.join(BACKUP_DIR);
    let version = backup_version(dir).ok_or(UpdateError::NoBackup)?;
    let items: Vec<(String, PathBuf)> = program_files()
        .iter()
        .filter(|n| bak.join(n).is_file())
        .map(|n| (n.to_string(), bak.join(n)))
        .collect();
    if items.is_empty() {
        return Err(UpdateError::NoBackup);
    }
    // 先复制出来：install_files 会用新的备份替换掉当前备份目录。
    let work = super::paths::cache_dir().join("rollback");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| UpdateError::Install(e.to_string()))?;
    let mut copied = Vec::new();
    for (name, src) in &items {
        let dst = work.join(name);
        std::fs::copy(src, &dst).map_err(|e| UpdateError::Install(e.to_string()))?;
        copied.push((name.clone(), dst));
    }
    let result = install_files(dir, &copied, super::APP_VERSION, &version);
    let _ = std::fs::remove_dir_all(&work);
    result?;
    let mut state = super::state::UpdateState::load();
    state.installed_pending_restart = Some(version.clone());
    // 回退后不应马上再提示同一个新版本。
    state.skipped_version = Some(super::APP_VERSION.to_string());
    let _ = state.save();
    Ok(version)
}

/// 启动后清理：中断留下的临时文件、旧的下载缓存、等重启的标记。失败只记日志。
pub fn cleanup_after_start(exe: Option<&Path>) {
    if let Some(dir) = exe.and_then(Path::parent) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(STAGED_PREFIX) {
                    let _ = std::fs::remove_file(e.path());
                } else if name == BACKUP_TMP_DIR || name.starts_with(BACKUP_OLD_PREFIX) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
    let current = super::current_version();
    if let Ok(rd) = std::fs::read_dir(super::paths::cache_dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stale = semver::Version::parse(&name).map(|v| v <= current).unwrap_or(false)
                || name == "rollback";
            if stale && e.path().is_dir() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let mut state = super::state::UpdateState::load();
    if let Some(p) = state.installed_pending_restart.as_deref() {
        if semver::Version::parse(p).map(|v| v == current).unwrap_or(true) {
            state.installed_pending_restart = None;
            let _ = state.save();
        }
    }
}

/// Windows 安装版：以独立进程静默运行新的安装程序。
pub fn launch_installer(setup: &Path, relaunch: bool) -> Result<(), UpdateError> {
    let log_path = setup.with_extension("log");
    let mut cmd = Command::new(setup);
    cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"]);
    cmd.arg(format!("/LOG={}", log_path.display()));
    if relaunch {
        cmd.arg("/RELAUNCH=1");
    }
    spawn_detached(&mut cmd).map_err(|e| UpdateError::Install(format!("cannot start installer: {e}")))
}

/// 以独立进程启动（不随当前进程退出而结束）。
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ())
}

fn sanitize_component(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || "._-+".contains(c) { c } else { '_' })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;

    /// 写一个假的「程序」：shell 脚本，`--version` 时打印版本号。
    fn fake_program(path: &Path, name: &str, version: &str) {
        let body = format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"{name} {version}\"; fi\n");
        std::fs::write(path, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn make_tar_gz(path: &Path, files: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
    }

    fn script(name: &str, version: &str) -> Vec<u8> {
        format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"{name} {version}\"; fi\n").into_bytes()
    }

    #[test]
    fn tar_extraction_only_takes_wanted_files() {
        let dir = tempfile::tempdir().unwrap();
        let tgz = dir.path().join("p.tar.gz");
        make_tar_gz(
            &tgz,
            &[
                ("Mist-linux-x86_64/Mist", b"gui"),
                ("Mist-linux-x86_64/mist", b"cli"),
                ("Mist-linux-x86_64/README.md", b"readme"),
                ("a/b/c/mist", b"deep"),
            ],
        );
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let got = extract_tar_gz(&tgz, &out_dir, &["Mist", "mist"]).unwrap();
        let names: Vec<_> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["Mist", "mist"]);
        assert_eq!(std::fs::read(out_dir.join("mist")).unwrap(), b"cli");
        assert!(!out_dir.join("README.md").exists());
    }

    #[test]
    fn zip_extraction_only_takes_wanted_files() {
        let dir = tempfile::tempdir().unwrap();
        let zpath = dir.path().join("p.zip");
        {
            let f = std::fs::File::create(&zpath).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("Mist-windows-x86_64/Mist.exe", opts).unwrap();
            w.write_all(b"gui").unwrap();
            w.start_file("Mist-windows-x86_64/README.md", opts).unwrap();
            w.write_all(b"r").unwrap();
            w.start_file("../evil/Mist.exe", opts).unwrap();
            w.write_all(b"evil").unwrap();
            w.finish().unwrap();
        }
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let got = extract_zip(&zpath, &out_dir, &["Mist.exe", "mist-cli.exe"]).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(std::fs::read(out_dir.join("Mist.exe")).unwrap(), b"gui");
    }

    #[test]
    fn path_filter_rejects_traversal() {
        let w = ["Mist", "mist"];
        assert_eq!(entry_basename_if_wanted(Path::new("Mist"), &w), Some("Mist"));
        assert_eq!(entry_basename_if_wanted(Path::new("top/mist"), &w), Some("mist"));
        assert_eq!(entry_basename_if_wanted(Path::new("../mist"), &w), None);
        assert_eq!(entry_basename_if_wanted(Path::new("/abs/mist"), &w), None);
        assert_eq!(entry_basename_if_wanted(Path::new("a/b/mist"), &w), None);
        assert_eq!(entry_basename_if_wanted(Path::new("top/other"), &w), None);
    }

    #[test]
    fn install_backup_and_rollback_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("app");
        std::fs::create_dir_all(&install).unwrap();
        fake_program(&install.join("Mist"), "Mist", "1.0.0");
        fake_program(&install.join("mist"), "mist", "1.0.0");

        let tgz = dir.path().join("Mist-linux-x86_64.tar.gz");
        make_tar_gz(
            &tgz,
            &[
                ("Mist-linux-x86_64/Mist", &script("Mist", "2.0.0")),
                ("Mist-linux-x86_64/mist", &script("mist", "2.0.0")),
            ],
        );
        install_from_archive(&tgz, KIND_TAR_GZ, &install, &install.join("mist"), "2.0.0").unwrap();
        smoke_test(&install.join("Mist"), "2.0.0").unwrap();
        smoke_test(&install.join("mist"), "2.0.0").unwrap();
        // install_from_archive 以当前程序版本作为备份版本号。
        assert_eq!(backup_version(&install).as_deref(), Some(crate::core::updater::APP_VERSION));
        smoke_test(&install.join(BACKUP_DIR).join("mist"), "1.0.0").unwrap();
        // 没有留下临时文件
        let leftovers: Vec<_> = std::fs::read_dir(&install)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(STAGED_PREFIX) || n == BACKUP_TMP_DIR)
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        // 回退：直接调用 install_files，模拟 rollback 的核心步骤。
        let bak = install.join(BACKUP_DIR);
        let work = dir.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut items = Vec::new();
        for n in ["Mist", "mist"] {
            std::fs::copy(bak.join(n), work.join(n)).unwrap();
            items.push((n.to_string(), work.join(n)));
        }
        install_files(&install, &items, "2.0.0", "1.0.0").unwrap();
        smoke_test(&install.join("mist"), "1.0.0").unwrap();
        assert_eq!(backup_version(&install).as_deref(), Some("2.0.0"));
    }

    #[test]
    fn failed_smoke_test_leaves_install_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("app");
        std::fs::create_dir_all(&install).unwrap();
        fake_program(&install.join("mist"), "mist", "1.0.0");
        let tgz = dir.path().join("p.tar.gz");
        // 包里的版本号与清单不符 → 冒烟验证失败
        make_tar_gz(&tgz, &[("x/mist", &script("mist", "1.5.0"))]);
        let err = install_from_archive(&tgz, KIND_TAR_GZ, &install, &install.join("mist"), "2.0.0").unwrap_err();
        assert!(matches!(err, UpdateError::SmokeTest(_)), "{err:?}");
        smoke_test(&install.join("mist"), "1.0.0").unwrap();
        assert!(!install.join(BACKUP_DIR).exists());
        let names: Vec<_> = std::fs::read_dir(&install)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["mist".to_string()]);
    }

    #[test]
    fn missing_program_in_package_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("app");
        std::fs::create_dir_all(&install).unwrap();
        fake_program(&install.join("Mist"), "Mist", "1.0.0");
        fake_program(&install.join("mist"), "mist", "1.0.0");
        let tgz = dir.path().join("p.tar.gz");
        make_tar_gz(&tgz, &[("x/mist", &script("mist", "2.0.0"))]);
        let err = install_from_archive(&tgz, KIND_TAR_GZ, &install, &install.join("mist"), "2.0.0").unwrap_err();
        assert!(matches!(err, UpdateError::Install(_)));
        smoke_test(&install.join("Mist"), "1.0.0").unwrap();
    }

    #[test]
    fn read_only_dir_reports_not_writable() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root 不受目录权限限制
        }
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("app");
        std::fs::create_dir_all(&install).unwrap();
        fake_program(&install.join("mist"), "mist", "1.0.0");
        let src = dir.path().join("new-mist");
        fake_program(&src, "mist", "2.0.0");
        std::fs::set_permissions(&install, std::fs::Permissions::from_mode(0o555)).unwrap();
        let err = install_files(&install, &[("mist".into(), src)], "1.0.0", "2.0.0").unwrap_err();
        std::fs::set_permissions(&install, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(err, UpdateError::NotWritable(_)), "{err:?}");
        smoke_test(&install.join("mist"), "1.0.0").unwrap();
    }

    #[test]
    fn smoke_test_rejects_wrong_version_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("mist");
        fake_program(&p, "mist", "1.2.0");
        assert!(smoke_test(&p, "1.2.0").is_ok());
        assert!(smoke_test(&p, "1.2").is_err());
        assert!(smoke_test(&p, "1.2.0.1").is_err());
        assert!(smoke_test(&dir.path().join("missing"), "1.2.0").is_err());
    }
}
