//! 本机 / 远端目录展开，供整目录传输入队。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 展开本机路径：文件直接一条；目录则递归。返回 `(本地绝对路径, 相对路径, 是否仅建空目录)`。
pub(super) fn collect_local_tree(
    root: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<(PathBuf, PathBuf, bool)>, String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Ok(Vec::new());
    }
    let meta = std::fs::metadata(root)
        .map_err(|e| format!("stat {}: {e}", root.display()))?;
    if meta.is_file() {
        let name = root
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("file.bin"));
        return Ok(vec![(root.to_path_buf(), name, false)]);
    }
    if !meta.is_dir() {
        return Err(format!("Not a file or directory: {}", root.display()));
    }
    let mut out = Vec::new();
    collect_local_dir(root, root, &mut out, cancel)?;
    Ok(out)
}

fn collect_local_dir(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(PathBuf, PathBuf, bool)>,
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Ok(());
    }
    let rd = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    let mut any = false;
    for ent in rd {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Ok(());
        }
        let ent = ent.map_err(|e| format!("read_dir entry: {e}"))?;
        let path = ent.path();
        let name = ent.file_name();
        if name == "." || name == ".." {
            continue;
        }
        any = true;
        let ft = ent
            .file_type()
            .map_err(|e| format!("file_type {}: {e}", path.display()))?;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_path_buf();
        if ft.is_dir() {
            let before = out.len();
            collect_local_dir(root, &path, out, cancel)?;
            if out.len() == before {
                out.push((path, rel, true));
            }
        } else if ft.is_file() {
            out.push((path, rel, false));
        }
    }
    if !any && dir != root {
        let rel = dir.strip_prefix(root).unwrap_or(dir).to_path_buf();
        out.push((dir.to_path_buf(), rel, true));
    }
    Ok(())
}

/// 名字像文本文件（可走远程编辑）；过大或明显二进制则否。
pub(super) fn looks_like_editable_text(name: &str, size: u64) -> bool {
    const MAX: u64 = 2 * 1024 * 1024;
    if size > MAX {
        return false;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "txt"
            | "md"
            | "rst"
            | "log"
            | "json"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "conf"
            | "cfg"
            | "env"
            | "xml"
            | "properties"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "py"
            | "rs"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "rb"
            | "php"
            | "lua"
            | "sql"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "vue"
            | "svelte"
            | "service"
            | "nginx"
            | ""
    ) && !name.ends_with(".min.js")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editable_text_heuristics() {
        assert!(looks_like_editable_text("nginx.conf", 100));
        assert!(looks_like_editable_text("README", 100));
        assert!(!looks_like_editable_text("app.bin", 100));
        assert!(!looks_like_editable_text("big.log", 3 * 1024 * 1024));
    }

    #[test]
    fn collect_local_file() {
        let dir = std::env::temp_dir().join("mistterm_sftp_tree_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.txt");
        std::fs::write(&f, b"hi").unwrap();
        let items = collect_local_tree(&f, None).unwrap();
        assert_eq!(items.len(), 1);
        assert!(!items[0].2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
