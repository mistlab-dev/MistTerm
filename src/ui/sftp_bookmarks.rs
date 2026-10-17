//! SFTP 书签与最近访问（本机全局；远端按主机键隔离）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_BOOKMARKS: usize = 32;
const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct SftpBookmarkStore {
    #[serde(default)]
    pub local: Vec<String>,
    /// `user@host` → 远端路径列表
    #[serde(default)]
    pub remote_by_host: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub recent_local: Vec<String>,
    #[serde(default)]
    pub recent_remote_by_host: HashMap<String, Vec<String>>,
}

impl SftpBookmarkStore {
    pub fn default_path() -> PathBuf {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("sftp_bookmarks.json")
    }

    pub fn load() -> Self {
        let path = Self::default_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::default_path();
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }

    pub fn add_local_bookmark(&mut self, path: &Path) {
        push_unique_front(&mut self.local, path_str(path), MAX_BOOKMARKS);
        self.save();
    }

    pub fn remove_local_bookmark(&mut self, path: &str) {
        self.local.retain(|p| p != path);
        self.save();
    }

    pub fn add_remote_bookmark(&mut self, host_key: &str, path: &Path) {
        let list = self
            .remote_by_host
            .entry(host_key.to_string())
            .or_default();
        push_unique_front(list, path_str(path), MAX_BOOKMARKS);
        self.save();
    }

    pub fn remove_remote_bookmark(&mut self, host_key: &str, path: &str) {
        if let Some(list) = self.remote_by_host.get_mut(host_key) {
            list.retain(|p| p != path);
            self.save();
        }
    }

    pub fn touch_recent_local(&mut self, path: &Path) {
        let item = path_str(path);
        if !is_menu_path(&item) {
            return;
        }
        push_unique_front(&mut self.recent_local, item, MAX_RECENT);
        self.save();
    }

    pub fn touch_recent_remote(&mut self, host_key: &str, path: &Path) {
        let item = path_str(path);
        if !is_menu_path(&item) {
            return;
        }
        let list = self
            .recent_remote_by_host
            .entry(host_key.to_string())
            .or_default();
        push_unique_front(list, item, MAX_RECENT);
        self.save();
    }

    pub fn remote_bookmarks(&self, host_key: &str) -> &[String] {
        self.remote_by_host
            .get(host_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn recent_remote(&self, host_key: &str) -> &[String] {
        self.recent_remote_by_host
            .get(host_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

fn path_str(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn is_menu_path(path: &str) -> bool {
    let trimmed = path.trim();
    !trimmed.is_empty() && trimmed != "."
}

fn push_unique_front(list: &mut Vec<String>, item: String, max: usize) {
    list.retain(|p| p != &item);
    list.insert(0, item);
    if list.len() > max {
        list.truncate(max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_unique_front_dedupes_and_caps() {
        let mut v = Vec::new();
        push_unique_front(&mut v, "a".into(), 3);
        push_unique_front(&mut v, "b".into(), 3);
        push_unique_front(&mut v, "a".into(), 3);
        push_unique_front(&mut v, "c".into(), 3);
        push_unique_front(&mut v, "d".into(), 3);
        assert_eq!(v, vec!["d", "c", "a"]);
    }
}
