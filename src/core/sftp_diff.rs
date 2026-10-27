//! 目录对比与单向同步计划（#163）。
//!
//! ## 冲突策略（写死，不覆盖）
//!
//! 同一相对路径在两边都存在，且 **size 或 mtime（Unix 秒）不同**，或者一边是文件、
//! 另一边是目录，视为冲突。同步时 **跳过** 这些项，调用方把它们列出来，**不会覆盖**。
//!
//! 没有共同基线，无法区分「两边都改过」和「只有一边改过」，所以 MVP 一律不覆盖。
//! 仅一侧存在的文件按用户选择的方向复制；仅目标侧多出的文件 **不删除**。
//! 两边都有的目录只当作结构，不因为目录自身的 mtime 记成冲突。
//! 符号链接不参与对比（由扫描方跳过），同步时也不跟随：根下任一段是符号链接则失败。
//! 目标路径已经存在则跳过并列出，不覆盖。空目录会列出，同步时在目标侧创建。
//! 一边是文件、另一边在该路径下还有子项，算结构冲突，不写入。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 扫描一侧目录得到的条目。`rel` 使用 `/` 分隔，不含 `.` 与 `..`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSideMeta {
    pub rel: String,
    pub size: u64,
    pub mtime_secs: i64,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// 仅左侧（本机）有。
    OnlyLeft,
    /// 仅右侧（远端）有。
    OnlyRight,
    /// 两边都有，但 size / mtime 不同，或文件与目录类型不一致。
    BothDiffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub rel: String,
    pub kind: DiffKind,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
    pub left_mtime: Option<i64>,
    pub right_mtime: Option<i64>,
    pub left_is_dir: bool,
    pub right_is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// 左侧 → 右侧（本机上传到远端）。
    LocalToRemote,
    /// 右侧 → 左侧（远端下载到本机）。
    RemoteToLocal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncItem {
    pub rel: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    /// 需要复制的文件（不含冲突、不含仅目标侧存在的条目）。
    pub copy: Vec<SyncItem>,
    /// 仅源侧存在的空目录。
    pub mkdirs: Vec<SyncItem>,
    /// 本次范围内被跳过的冲突相对路径。
    pub skipped_conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkOutcome {
    Done(Vec<DiffSideMeta>),
    Cancelled,
}

/// 把相对路径收成可比对的 key。含 `..` 时返回 `None`（禁止逃出对比根）。
pub fn normalize_rel_key(rel: &str) -> Option<String> {
    let rel = rel.replace('\\', "/");
    let mut parts = Vec::new();
    for part in rel.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}

/// `full` 相对 `root` 的 key。不在根下或含 `..` 时返回 `None`。
pub fn rel_under_root(root: &str, full: &str) -> Option<String> {
    let root = trim_root_slashes(root);
    let full = trim_root_slashes(full);
    if full == root {
        return Some(String::new());
    }
    let rest = if root == "/" {
        full.strip_prefix('/')?
    } else {
        let prefix = format!("{root}/");
        full.strip_prefix(&prefix)?
    };
    normalize_rel_key(rest)
}

pub fn join_local(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = normalize_rel_key(rel)?;
    let mut path = root.to_path_buf();
    if rel.is_empty() {
        return Some(path);
    }
    for seg in rel.split('/') {
        path.push(seg);
    }
    Some(path)
}

pub fn join_remote(root: &str, rel: &str) -> Option<String> {
    let rel = normalize_rel_key(rel)?;
    let root = trim_root_slashes(root);
    if rel.is_empty() {
        return Some(root);
    }
    if root.is_empty() || root == "." {
        Some(rel)
    } else if root == "/" {
        Some(format!("/{rel}"))
    } else {
        Some(format!("{root}/{rel}"))
    }
}

fn trim_root_slashes(root: &str) -> String {
    let root = root.replace('\\', "/");
    if root == "/" {
        return root;
    }
    let trimmed = root.trim_end_matches('/').to_string();
    if trimmed.ends_with(':') {
        format!("{trimmed}/")
    } else {
        trimmed
    }
}

/// 非跟随地看到的节点。`Missing` 表示这一段还不存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsNode {
    Missing,
    Dir,
    File,
    Symlink,
    Other,
}

/// 目标相对路径在非跟随检查后的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestCheck {
    /// 可以新建。路径上已有的段都是真实目录。
    Absent,
    /// 最终段已是真实目录。
    PresentDir,
    /// 最终段已存在，且不是目录、也不是符号链接。
    PresentOther,
    /// 某一段是符号链接。`at` 是相对路径。
    Symlink { at: String },
    /// 祖先不是目录（文件或其它）。`at` 是挡住后面的那段相对路径。
    Blocked { at: String },
}

/// 对 `rel` 的每一段调用 `lookup`（参数是累计相对路径）。不跟随符号链接。
pub fn check_dest_rel(
    rel: &str,
    mut lookup: impl FnMut(&str) -> Result<FsNode, String>,
) -> Result<DestCheck, String> {
    let rel = normalize_rel_key(rel).ok_or_else(|| format!("Invalid path: {rel}"))?;
    if rel.is_empty() {
        return Ok(DestCheck::PresentDir);
    }
    let parts: Vec<&str> = rel.split('/').collect();
    let mut cur = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            cur.push('/');
        }
        cur.push_str(part);
        let last = i + 1 == parts.len();
        match lookup(&cur)? {
            FsNode::Symlink => return Ok(DestCheck::Symlink { at: cur }),
            FsNode::Missing => return Ok(DestCheck::Absent),
            FsNode::Dir if last => return Ok(DestCheck::PresentDir),
            FsNode::Dir => {}
            FsNode::File | FsNode::Other if last => return Ok(DestCheck::PresentOther),
            FsNode::File | FsNode::Other => return Ok(DestCheck::Blocked { at: cur }),
        }
    }
    Ok(DestCheck::Absent)
}

/// `rel` 的父相对路径。文件在根下时是空字符串。
pub fn parent_rel(rel: &str) -> Option<String> {
    let rel = normalize_rel_key(rel)?;
    if rel.is_empty() {
        return None;
    }
    match rel.rfind('/') {
        Some(idx) => Some(rel[..idx].to_string()),
        None => Some(String::new()),
    }
}

pub fn local_node(path: &Path) -> Result<FsNode, String> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            let ft = meta.file_type();
            if ft.is_symlink() {
                Ok(FsNode::Symlink)
            } else if ft.is_dir() {
                Ok(FsNode::Dir)
            } else if ft.is_file() {
                Ok(FsNode::File)
            } else {
                Ok(FsNode::Other)
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FsNode::Missing),
        Err(e) => Err(format!("Failed to lstat {}: {e}", path.display())),
    }
}

pub fn check_local_rel(root: &Path, rel: &str) -> Result<DestCheck, String> {
    check_dest_rel(rel, |prefix| {
        let path = join_local(root, prefix).ok_or_else(|| format!("Invalid path: {prefix}"))?;
        local_node(&path)
    })
}

/// `path` 相对 `root` 的 key。不在根下时返回错误。
pub fn rel_under_local(root: &Path, path: &Path) -> Result<String, String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| format!("Path {} is outside {}", path.display(), root.display()))?;
    let text = rel.to_string_lossy().replace('\\', "/");
    normalize_rel_key(&text).ok_or_else(|| format!("Invalid path: {text}"))
}

fn assert_under_root(root: &Path, path: &Path) -> Result<(), String> {
    let root_c = std::fs::canonicalize(root)
        .map_err(|e| format!("Failed to resolve {}: {e}", root.display()))?;
    let path_c = std::fs::canonicalize(path)
        .map_err(|e| format!("Failed to resolve {}: {e}", path.display()))?;
    if path_c.starts_with(&root_c) {
        Ok(())
    } else {
        Err(format!(
            "Refusing to write outside compare root: {}",
            path_c.display()
        ))
    }
}

fn create_local_dirs(root: &Path, rel: &str) -> Result<(), String> {
    let rel = normalize_rel_key(rel).ok_or_else(|| format!("Invalid path: {rel}"))?;
    if rel.is_empty() {
        return assert_under_root(root, root);
    }
    let mut cur = root.to_path_buf();
    let mut acc = String::new();
    for (i, part) in rel.split('/').enumerate() {
        if i > 0 {
            acc.push('/');
        }
        acc.push_str(part);
        cur.push(part);
        match local_node(&cur)? {
            FsNode::Dir => {}
            FsNode::Missing => {
                std::fs::create_dir(&cur).map_err(|e| format!("mkdir {}: {e}", cur.display()))?;
                match local_node(&cur)? {
                    FsNode::Dir => {}
                    FsNode::Symlink => {
                        return Err(format!("Refusing to follow symlink: {acc}"));
                    }
                    _ => return Err(format!("Not a directory: {acc}")),
                }
            }
            FsNode::Symlink => return Err(format!("Refusing to follow symlink: {acc}")),
            _ => return Err(format!("Not a directory: {acc}")),
        }
    }
    assert_under_root(root, &cur)
}

/// 在对比根下逐段创建目录。已有真实目录则跳过；遇符号链接或非目录则失败，不调用 `create_dir_all`。
pub fn mkdir_local_nofollow(root: &Path, rel: &str) -> Result<(), String> {
    let rel = normalize_rel_key(rel).ok_or_else(|| format!("Invalid path: {rel}"))?;
    if rel.is_empty() {
        return assert_under_root(root, root);
    }
    match check_local_rel(root, &rel)? {
        DestCheck::PresentDir | DestCheck::Absent => create_local_dirs(root, &rel),
        DestCheck::Symlink { at } => Err(format!("Refusing to follow symlink: {at}")),
        DestCheck::Blocked { at } => Err(format!("Parent is not a directory: {at}")),
        DestCheck::PresentOther => Err(format!("Not a directory: {rel}")),
    }
}

/// 在对比根下新建文件。`create_new` 不会跟随已有符号链接，目标已存在则失败。
pub fn create_local_file_nofollow(root: &Path, rel: &str) -> Result<std::fs::File, String> {
    match check_local_rel(root, rel)? {
        DestCheck::Absent => {}
        DestCheck::Symlink { at } => return Err(format!("Refusing to follow symlink: {at}")),
        DestCheck::Blocked { at } => return Err(format!("Parent is not a directory: {at}")),
        DestCheck::PresentDir | DestCheck::PresentOther => {
            return Err(format!("Refusing to overwrite existing path: {rel}"));
        }
    }
    match parent_rel(rel) {
        Some(parent) if !parent.is_empty() => mkdir_local_nofollow(root, &parent)?,
        Some(_) => {}
        None => return Err(format!("Invalid path: {rel}")),
    }
    let path = join_local(root, rel).ok_or_else(|| format!("Invalid path: {rel}"))?;
    match local_node(&path)? {
        FsNode::Missing => {}
        FsNode::Symlink => return Err(format!("Refusing to follow symlink: {rel}")),
        _ => return Err(format!("Refusing to overwrite existing path: {rel}")),
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("Failed to create local file {}: {e}", path.display()))
}

/// 按相对路径合并两侧快照。相同 size 且相同 mtime（秒）的文件不出现在结果里。
/// 一边是文件、另一边在该路径之下还有条目时，双方都记成冲突。
pub fn compare_snapshots(left: &[DiffSideMeta], right: &[DiffSideMeta]) -> Vec<DiffRow> {
    let left = index_side(left);
    let right = index_side(right);
    let keys: BTreeSet<String> = left.keys().chain(right.keys()).cloned().collect();
    let mut rows = Vec::new();
    for key in keys {
        let l = left.get(&key);
        let r = right.get(&key);
        let clash = ancestor_non_dir(&left, &key)
            || ancestor_non_dir(&right, &key)
            || l.is_some_and(|meta| !meta.is_dir && has_descendant(&right, &key))
            || r.is_some_and(|meta| !meta.is_dir && has_descendant(&left, &key));
        if clash {
            rows.push(DiffRow {
                rel: key,
                kind: DiffKind::BothDiffer,
                left_size: l.map(|meta| meta.size),
                right_size: r.map(|meta| meta.size),
                left_mtime: l.map(|meta| meta.mtime_secs),
                right_mtime: r.map(|meta| meta.mtime_secs),
                left_is_dir: l.is_some_and(|meta| meta.is_dir),
                right_is_dir: r.is_some_and(|meta| meta.is_dir),
            });
            continue;
        }
        match (l, r) {
            (Some(l), None) => rows.push(only_row(key, DiffKind::OnlyLeft, l, true)),
            (None, Some(r)) => rows.push(only_row(key, DiffKind::OnlyRight, r, false)),
            (Some(l), Some(r)) => {
                if l.is_dir && r.is_dir {
                    continue;
                }
                let same_file =
                    !l.is_dir && !r.is_dir && l.size == r.size && l.mtime_secs == r.mtime_secs;
                if same_file {
                    continue;
                }
                rows.push(DiffRow {
                    rel: key,
                    kind: DiffKind::BothDiffer,
                    left_size: Some(l.size),
                    right_size: Some(r.size),
                    left_mtime: Some(l.mtime_secs),
                    right_mtime: Some(r.mtime_secs),
                    left_is_dir: l.is_dir,
                    right_is_dir: r.is_dir,
                });
            }
            (None, None) => {}
        }
    }
    rows
}

fn ancestor_non_dir(index: &BTreeMap<String, DiffSideMeta>, rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return false;
    }
    let mut cur = String::new();
    for part in &parts[..parts.len() - 1] {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        if index.get(&cur).is_some_and(|meta| !meta.is_dir) {
            return true;
        }
    }
    false
}

fn has_descendant(index: &BTreeMap<String, DiffSideMeta>, rel: &str) -> bool {
    let mut prefix = String::from(rel);
    prefix.push('/');
    index.keys().any(|key| key.starts_with(&prefix))
}

fn index_side(entries: &[DiffSideMeta]) -> BTreeMap<String, DiffSideMeta> {
    let mut map = BTreeMap::new();
    for entry in entries {
        let Some(key) = normalize_rel_key(&entry.rel) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        map.insert(
            key.clone(),
            DiffSideMeta {
                rel: key,
                size: entry.size,
                mtime_secs: entry.mtime_secs,
                is_dir: entry.is_dir,
            },
        );
    }
    map
}

fn only_row(rel: String, kind: DiffKind, side: &DiffSideMeta, left: bool) -> DiffRow {
    DiffRow {
        rel,
        kind,
        left_size: left.then_some(side.size),
        right_size: (!left).then_some(side.size),
        left_mtime: left.then_some(side.mtime_secs),
        right_mtime: (!left).then_some(side.mtime_secs),
        left_is_dir: left && side.is_dir,
        right_is_dir: !left && side.is_dir,
    }
}

/// `only == None` 表示全部差异；`Some` 只处理勾选的相对路径。
///
/// 冲突永远进入 `skipped_conflicts`，不会进入 `copy` / `mkdirs`。
pub fn plan_sync(
    rows: &[DiffRow],
    only: Option<&HashSet<String>>,
    direction: SyncDirection,
) -> SyncPlan {
    let mut copy = Vec::new();
    let mut mkdirs = Vec::new();
    let mut skipped_conflicts = Vec::new();
    for row in rows {
        if let Some(set) = only {
            if !set.contains(&row.rel) {
                continue;
            }
        }
        let (source_dir, source_size) = match direction {
            SyncDirection::LocalToRemote => (row.left_is_dir, row.left_size.unwrap_or(0)),
            SyncDirection::RemoteToLocal => (row.right_is_dir, row.right_size.unwrap_or(0)),
        };
        match (direction, row.kind) {
            (_, DiffKind::BothDiffer) => skipped_conflicts.push(row.rel.clone()),
            (SyncDirection::LocalToRemote, DiffKind::OnlyLeft)
            | (SyncDirection::RemoteToLocal, DiffKind::OnlyRight) => {
                if dest_prefix_blocks(rows, row, direction) {
                    skipped_conflicts.push(row.rel.clone());
                    continue;
                }
                let item = SyncItem {
                    rel: row.rel.clone(),
                    is_dir: source_dir,
                    size: source_size,
                };
                if source_dir {
                    mkdirs.push(item);
                } else {
                    copy.push(item);
                }
            }
            _ => {}
        }
    }
    SyncPlan {
        copy,
        mkdirs,
        skipped_conflicts,
    }
}

/// 同步前要确保存在的目录（空目录本身 + 文件的祖先）。按字典序，彼此独立，可逐个 `mkdir -p`。
pub fn directory_rels(plan: &SyncPlan) -> Vec<String> {
    let mut set = BTreeSet::new();
    for item in &plan.mkdirs {
        push_dir_chain(&mut set, &item.rel, true);
    }
    for item in &plan.copy {
        push_dir_chain(&mut set, &item.rel, false);
    }
    set.into_iter().collect()
}

fn dest_prefix_blocks(rows: &[DiffRow], row: &DiffRow, direction: SyncDirection) -> bool {
    let dest_file = |other: &DiffRow| match direction {
        SyncDirection::LocalToRemote => other.right_size.is_some() && !other.right_is_dir,
        SyncDirection::RemoteToLocal => other.left_size.is_some() && !other.left_is_dir,
    };
    let dest_present = |other: &DiffRow| match direction {
        SyncDirection::LocalToRemote => other.right_size.is_some() || other.right_is_dir,
        SyncDirection::RemoteToLocal => other.left_size.is_some() || other.left_is_dir,
    };
    let source_file = match direction {
        SyncDirection::LocalToRemote => !row.left_is_dir,
        SyncDirection::RemoteToLocal => !row.right_is_dir,
    };
    if rows
        .iter()
        .any(|other| is_strict_prefix(&other.rel, &row.rel) && dest_file(other))
    {
        return true;
    }
    source_file
        && rows
            .iter()
            .any(|other| is_strict_prefix(&row.rel, &other.rel) && dest_present(other))
}

fn is_strict_prefix(prefix: &str, rel: &str) -> bool {
    let mut with_slash = String::from(prefix);
    with_slash.push('/');
    rel.starts_with(&with_slash)
}

fn push_dir_chain(set: &mut BTreeSet<String>, rel: &str, include_self: bool) {
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    let last = if include_self {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };
    let mut cur = String::new();
    for (i, part) in parts.iter().take(last).enumerate() {
        if i > 0 {
            cur.push('/');
        }
        cur.push_str(part);
        set.insert(cur.clone());
    }
}

/// 递归扫描本机目录。取消在条目之间生效；符号链接与非常规文件跳过。
pub fn walk_local_diff(
    root: &Path,
    cancel: &AtomicBool,
    scanned: &AtomicU64,
    mut on_tick: impl FnMut(),
) -> Result<WalkOutcome, String> {
    let meta = std::fs::metadata(root)
        .map_err(|e| format!("Failed to read local directory {}: {e}", root.display()))?;
    if !meta.is_dir() {
        return Err(format!("Not a folder: {}", root.display()));
    }
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(WalkOutcome::Cancelled);
        }
        let rd = std::fs::read_dir(&dir)
            .map_err(|e| format!("Failed to read local directory {}: {e}", dir.display()))?;
        let mut kept = 0usize;
        for ent in rd {
            if cancel.load(Ordering::Relaxed) {
                return Ok(WalkOutcome::Cancelled);
            }
            scanned.fetch_add(1, Ordering::Relaxed);
            on_tick();
            let ent = ent.map_err(|e| format!("read_dir entry: {e}"))?;
            let name = ent.file_name();
            let name = name.to_string_lossy();
            if name == "." || name == ".." {
                continue;
            }
            let ft = ent
                .file_type()
                .map_err(|e| format!("file_type {}: {e}", ent.path().display()))?;
            if ft.is_symlink() {
                continue;
            }
            let child_rel = if rel.is_empty() {
                name.to_string()
            } else {
                format!("{rel}/{name}")
            };
            let Some(child_rel) = normalize_rel_key(&child_rel) else {
                continue;
            };
            if ft.is_dir() {
                kept += 1;
                stack.push((ent.path(), child_rel));
            } else if ft.is_file() {
                kept += 1;
                let meta = ent
                    .metadata()
                    .map_err(|e| format!("stat {}: {e}", ent.path().display()))?;
                out.push(DiffSideMeta {
                    rel: child_rel,
                    size: meta.len(),
                    mtime_secs: mtime_secs(&meta),
                    is_dir: false,
                });
            }
        }
        if kept == 0 && !rel.is_empty() {
            let meta =
                std::fs::metadata(&dir).map_err(|e| format!("stat {}: {e}", dir.display()))?;
            out.push(DiffSideMeta {
                rel,
                size: 0,
                mtime_secs: mtime_secs(&meta),
                is_dir: true,
            });
        }
    }
    Ok(WalkOutcome::Done(out))
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel: &str, size: u64, mtime: i64) -> DiffSideMeta {
        DiffSideMeta {
            rel: rel.to_string(),
            size,
            mtime_secs: mtime,
            is_dir: false,
        }
    }

    fn dir(rel: &str, mtime: i64) -> DiffSideMeta {
        DiffSideMeta {
            rel: rel.to_string(),
            size: 0,
            mtime_secs: mtime,
            is_dir: true,
        }
    }

    #[test]
    fn compare_three_acceptance_scenes() {
        let left = vec![
            file("only_local.txt", 10, 1),
            file("changed.txt", 10, 1),
            file("same.txt", 4, 9),
        ];
        let right = vec![
            file("only_remote.txt", 8, 2),
            file("changed.txt", 11, 1),
            file("same.txt", 4, 9),
        ];
        let rows = compare_snapshots(&left, &right);
        let kinds: Vec<(&str, DiffKind)> = rows.iter().map(|r| (r.rel.as_str(), r.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                ("changed.txt", DiffKind::BothDiffer),
                ("only_local.txt", DiffKind::OnlyLeft),
                ("only_remote.txt", DiffKind::OnlyRight),
            ]
        );
        assert!(rows.iter().all(|r| r.rel != "same.txt"));
    }

    #[test]
    fn mtime_mismatch_is_conflict_and_identical_dirs_are_not() {
        let left = vec![file("a.txt", 3, 10), dir("empty", 1)];
        let right = vec![file("a.txt", 3, 20), dir("empty", 99)];
        let rows = compare_snapshots(&left, &right);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rel, "a.txt");
        assert_eq!(rows[0].kind, DiffKind::BothDiffer);
    }

    #[test]
    fn file_versus_directory_is_conflict() {
        let rows = compare_snapshots(&[file("x", 1, 1)], &[dir("x", 1)]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, DiffKind::BothDiffer);
        assert!(rows[0].right_is_dir);
        assert!(!rows[0].left_is_dir);
    }

    #[test]
    fn path_keys_ignore_slash_style() {
        let left = vec![file("sub\\a.txt", 1, 1)];
        let right = vec![file("./sub/a.txt", 2, 1)];
        let rows = compare_snapshots(&left, &right);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rel, "sub/a.txt");
        assert_eq!(rows[0].kind, DiffKind::BothDiffer);
    }

    #[test]
    fn dotdot_is_dropped() {
        assert_eq!(normalize_rel_key("a/../../b"), None);
        assert_eq!(rel_under_root("/root", "/root/../etc/passwd"), None);
        assert_eq!(rel_under_root("/root", "/root/a/b"), Some("a/b".into()));
        assert_eq!(rel_under_root("/", "/a"), Some("a".into()));
        assert_eq!(rel_under_root("C:/r", "C:/r/x"), Some("x".into()));
        assert_eq!(join_remote("C:/r/", "a/b").as_deref(), Some("C:/r/a/b"));
        assert_eq!(join_remote("/", "a").as_deref(), Some("/a"));
        assert!(join_local(Path::new("/tmp"), "../etc").is_none());
    }

    #[test]
    fn plan_upload_checked_only_local_and_skip_conflict() {
        let rows = compare_snapshots(
            &[file("only_local.txt", 10, 1), file("changed.txt", 10, 1)],
            &[file("only_remote.txt", 8, 2), file("changed.txt", 99, 1)],
        );
        let mut checked = HashSet::new();
        checked.insert("only_local.txt".to_string());
        let plan = plan_sync(&rows, Some(&checked), SyncDirection::LocalToRemote);
        assert_eq!(plan.copy.len(), 1);
        assert_eq!(plan.copy[0].rel, "only_local.txt");
        assert!(plan.mkdirs.is_empty());
        assert!(plan.skipped_conflicts.is_empty());

        let all = plan_sync(&rows, None, SyncDirection::LocalToRemote);
        assert_eq!(all.copy.len(), 1);
        assert_eq!(all.copy[0].rel, "only_local.txt");
        assert_eq!(all.skipped_conflicts, vec!["changed.txt".to_string()]);
        assert!(all.copy.iter().all(|c| c.rel != "only_remote.txt"));

        let down = plan_sync(&rows, None, SyncDirection::RemoteToLocal);
        assert_eq!(down.copy.len(), 1);
        assert_eq!(down.copy[0].rel, "only_remote.txt");
        assert_eq!(down.skipped_conflicts, vec!["changed.txt".to_string()]);
    }

    #[test]
    fn checked_conflict_is_listed_and_not_copied() {
        let rows = compare_snapshots(&[file("changed.txt", 1, 1)], &[file("changed.txt", 2, 1)]);
        let mut checked = HashSet::new();
        checked.insert("changed.txt".to_string());
        let plan = plan_sync(&rows, Some(&checked), SyncDirection::LocalToRemote);
        assert!(plan.copy.is_empty());
        assert_eq!(plan.skipped_conflicts, vec!["changed.txt".to_string()]);
    }

    #[test]
    fn directory_rels_include_file_parents_and_empty_dirs() {
        let plan = SyncPlan {
            copy: vec![SyncItem {
                rel: "a/b/c.txt".into(),
                is_dir: false,
                size: 1,
            }],
            mkdirs: vec![SyncItem {
                rel: "a/empty".into(),
                is_dir: true,
                size: 0,
            }],
            skipped_conflicts: vec![],
        };
        assert_eq!(
            directory_rels(&plan),
            vec!["a".to_string(), "a/b".to_string(), "a/empty".to_string()]
        );
    }

    #[test]
    fn walk_local_relative_paths_and_empty_dir() {
        let root = std::env::temp_dir().join(format!(
            "mistterm_sftp_diff_walk_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("only_local.txt"), b"local").unwrap();
        std::fs::write(root.join("sub").join("b.txt"), b"bb").unwrap();
        let cancel = AtomicBool::new(false);
        let scanned = AtomicU64::new(0);
        let outcome = walk_local_diff(&root, &cancel, &scanned, || {}).unwrap();
        let WalkOutcome::Done(entries) = outcome else {
            panic!("expected done");
        };
        let mut rels: Vec<_> = entries.iter().map(|e| e.rel.as_str()).collect();
        rels.sort();
        assert_eq!(rels, vec!["empty", "only_local.txt", "sub/b.txt"]);
        assert!(entries.iter().find(|e| e.rel == "empty").unwrap().is_dir);
        assert!(!entries.iter().any(|e| e.rel == "sub"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_local_thousand_entries_can_cancel() {
        let root = std::env::temp_dir().join(format!(
            "mistterm_sftp_diff_cancel_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..1000 {
            std::fs::write(root.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let cancel = AtomicBool::new(false);
        let scanned = AtomicU64::new(0);
        let mut seen = 0u64;
        let outcome = walk_local_diff(&root, &cancel, &scanned, || {
            seen += 1;
            if seen == 20 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert!(matches!(outcome, WalkOutcome::Cancelled));
        assert!(scanned.load(Ordering::Relaxed) < 1000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ancestor_file_and_child_are_conflicts() {
        let rows = compare_snapshots(&[file("a/b.txt", 1, 1)], &[file("a", 2, 1)]);
        assert!(rows.iter().all(|row| row.kind == DiffKind::BothDiffer));
        assert_eq!(rows.len(), 2);
        let up = plan_sync(&rows, None, SyncDirection::LocalToRemote);
        assert!(up.copy.is_empty());
        assert!(up.mkdirs.is_empty());
        assert!(up.skipped_conflicts.iter().any(|rel| rel == "a/b.txt"));
        let down = plan_sync(&rows, None, SyncDirection::RemoteToLocal);
        assert!(down.copy.is_empty());
        assert!(down.skipped_conflicts.iter().any(|rel| rel == "a"));
    }

    #[test]
    fn plan_blocks_file_prefix_even_if_rows_are_only_one_side() {
        let rows = vec![
            DiffRow {
                rel: "a/b.txt".into(),
                kind: DiffKind::OnlyLeft,
                left_size: Some(1),
                right_size: None,
                left_mtime: Some(1),
                right_mtime: None,
                left_is_dir: false,
                right_is_dir: false,
            },
            DiffRow {
                rel: "a".into(),
                kind: DiffKind::OnlyRight,
                left_size: None,
                right_size: Some(2),
                left_mtime: None,
                right_mtime: Some(1),
                left_is_dir: false,
                right_is_dir: false,
            },
        ];
        let up = plan_sync(&rows, None, SyncDirection::LocalToRemote);
        assert!(up.copy.is_empty());
        assert_eq!(up.skipped_conflicts, vec!["a/b.txt".to_string()]);
        let down = plan_sync(&rows, None, SyncDirection::RemoteToLocal);
        assert!(down.copy.is_empty());
        assert_eq!(down.skipped_conflicts, vec!["a".to_string()]);
    }

    #[test]
    fn check_dest_rel_rejects_symlink_and_existing_file() {
        let nodes = |rel: &str| match rel {
            "link" => Ok(FsNode::Symlink),
            "link/x" => Ok(FsNode::Missing),
            "taken.txt" => Ok(FsNode::File),
            "dir" => Ok(FsNode::Dir),
            "dir/new.txt" => Ok(FsNode::Missing),
            "file" => Ok(FsNode::File),
            "file/child" => Ok(FsNode::Missing),
            _ => Ok(FsNode::Missing),
        };
        assert!(matches!(
            check_dest_rel("link/x", nodes).unwrap(),
            DestCheck::Symlink { at } if at == "link"
        ));
        assert_eq!(
            check_dest_rel("taken.txt", nodes).unwrap(),
            DestCheck::PresentOther
        );
        assert_eq!(
            check_dest_rel("dir/new.txt", nodes).unwrap(),
            DestCheck::Absent
        );
        assert!(matches!(
            check_dest_rel("file/child", nodes).unwrap(),
            DestCheck::Blocked { at } if at == "file"
        ));
    }

    #[test]
    fn local_symlink_is_not_followed_and_existing_file_is_present() {
        let root = std::env::temp_dir().join(format!(
            "mistterm_sftp_diff_link_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let outside = std::env::temp_dir().join(format!(
            "mistterm_sftp_diff_out_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("taken.txt"), b"keep").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            let err = mkdir_local_nofollow(&root, "link/nested").unwrap_err();
            assert!(err.contains("symlink"));
            assert!(!outside.join("nested").exists());
            let err = create_local_file_nofollow(&root, "link/evil.txt").unwrap_err();
            assert!(err.contains("symlink"));
            assert!(!outside.join("evil.txt").exists());
        }
        assert_eq!(
            check_local_rel(&root, "taken.txt").unwrap(),
            DestCheck::PresentOther
        );
        let err = create_local_file_nofollow(&root, "taken.txt").unwrap_err();
        assert!(err.contains("overwrite"));
        assert_eq!(std::fs::read(root.join("taken.txt")).unwrap(), b"keep");
        mkdir_local_nofollow(&root, "real/child").unwrap();
        assert!(root.join("real/child").is_dir());
        let file = create_local_file_nofollow(&root, "real/child/new.txt").unwrap();
        drop(file);
        assert!(root.join("real/child/new.txt").is_file());
        let canon_root = std::fs::canonicalize(&root).unwrap();
        let canon_file = std::fs::canonicalize(root.join("real/child/new.txt")).unwrap();
        assert!(canon_file.starts_with(&canon_root));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn walk_local_skips_fifo_and_socket() {
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!(
            "mistterm_sftp_diff_fifo_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("ok.txt"), b"a").unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(root.join("pipe"))
            .status()
            .expect("mkfifo");
        assert!(status.success());
        let _sock = UnixListener::bind(root.join("sock.sock")).unwrap();
        let cancel = AtomicBool::new(false);
        let scanned = AtomicU64::new(0);
        let outcome = walk_local_diff(&root, &cancel, &scanned, || {}).unwrap();
        let WalkOutcome::Done(entries) = outcome else {
            panic!("expected done");
        };
        let rels: Vec<_> = entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, vec!["ok.txt"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
