//! 终端命令执行历史（Ctrl+R 搜索）

use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

const MEMORY_CAP: usize = 500;
const FILE_CAP: usize = 1000;
const RETENTION_DAYS: i64 = 60;
const MAX_COMMAND_LEN: usize = 1000;
pub const DISPLAY_COMMAND_MAX: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistoryEntry {
    pub command: String,
    pub executed_at: i64,
    pub session_id: Option<String>,
    pub session_name: Option<String>,
    pub success: bool,
}

impl HistoryEntry {
    pub fn executed_at_local(&self) -> Option<DateTime<Local>> {
        Local.timestamp_opt(self.executed_at, 0).single()
    }

    pub fn display_command(&self) -> String {
        if self.command.chars().count() > DISPLAY_COMMAND_MAX {
            let s: String = self.command.chars().take(DISPLAY_COMMAND_MAX).collect();
            format!("{}…", s)
        } else {
            self.command.clone()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<HistoryEntry>,
}

/// 命令历史：内存 + 可选持久化
pub struct CommandHistory {
    entries: VecDeque<HistoryEntry>,
    persist_path: PathBuf,
    dirty: bool,
    load_rx: Option<mpsc::Receiver<VecDeque<HistoryEntry>>>,
}

impl CommandHistory {
    pub fn new() -> Self {
        let path = Self::default_persist_path();
        let (tx, rx) = mpsc::channel();
        let load_path = path.clone();
        thread::spawn(move || {
            let loaded = Self::load_entries_from_disk(&load_path);
            let _ = tx.send(loaded);
        });
        Self {
            entries: VecDeque::new(),
            persist_path: path,
            dirty: false,
            load_rx: Some(rx),
        }
    }

    /// 轮询后台加载；返回 true 表示刚完成加载
    pub fn poll_background_load(&mut self) -> bool {
        let Some(rx) = &self.load_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(loaded) => {
                self.entries = loaded;
                self.load_rx = None;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.load_rx = None;
                false
            }
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.load_rx.is_none()
    }

    pub fn default_persist_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("command_history.json");
        p
    }

    fn load_entries_from_disk(path: &PathBuf) -> VecDeque<HistoryEntry> {
        if !path.exists() {
            return VecDeque::new();
        }
        let Ok(content) = fs::read_to_string(path) else {
            return VecDeque::new();
        };
        let Ok(file) = serde_json::from_str::<HistoryFile>(&content) else {
            return VecDeque::new();
        };
        let mut entries: VecDeque<HistoryEntry> = file.entries.into_iter().collect();
        let cutoff = Local::now().timestamp() - RETENTION_DAYS * 86400;
        entries.retain(|e| e.executed_at >= cutoff);
        while entries.len() > MEMORY_CAP {
            entries.pop_front();
        }
        entries
    }

    pub fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.persist_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut all: Vec<HistoryEntry> = self.entries.iter().cloned().collect();
        let cutoff = Local::now().timestamp() - RETENTION_DAYS * 86400;
        all.retain(|e| e.executed_at >= cutoff);
        if all.len() > FILE_CAP {
            all.drain(0..all.len() - FILE_CAP);
        }
        let file = HistoryFile { entries: all };
        let content = serde_json::to_string_pretty(&file)?;
        fs::write(&self.persist_path, content)?;
        self.dirty = false;
        Ok(())
    }

    /// 记录一条命令；连续相同命令只保留最新一条
    pub fn record(
        &mut self,
        command: &str,
        session_id: Option<&str>,
        session_name: Option<&str>,
        success: bool,
    ) {
        let cmd = command.trim();
        if cmd.is_empty() || cmd.starts_with('\x1b') {
            return;
        }
        let stored = if cmd.len() > MAX_COMMAND_LEN {
            cmd[..MAX_COMMAND_LEN].to_string()
        } else {
            cmd.to_string()
        };
        if let Some(last) = self.entries.back() {
            if last.command == stored {
                if let Some(back) = self.entries.back_mut() {
                    back.executed_at = Local::now().timestamp();
                    back.success = success;
                    back.session_id = session_id.map(str::to_string);
                    back.session_name = session_name.map(str::to_string);
                }
                self.dirty = true;
                return;
            }
        }
        self.entries.push_back(HistoryEntry {
            command: stored,
            executed_at: Local::now().timestamp(),
            session_id: session_id.map(str::to_string),
            session_name: session_name.map(str::to_string),
            success,
        });
        while self.entries.len() > MEMORY_CAP {
            self.entries.pop_front();
        }
        self.dirty = true;
    }

    pub fn entries_newest_first(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev()
    }

    pub fn search(&self, query: &str, ignore_case: bool) -> Vec<HistoryEntry> {
        let q = query.trim();
        if q.is_empty() {
            return self.entries.iter().rev().cloned().collect();
        }
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                if ignore_case {
                    e.command.to_lowercase().contains(&q.to_lowercase())
                } else {
                    e.command.contains(q)
                }
            })
            .cloned()
            .collect()
    }

    pub fn remove_matching(&mut self, command: &str) {
        self.entries.retain(|e| e.command != command);
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_consecutive() {
        let mut h = CommandHistory {
            entries: VecDeque::new(),
            persist_path: PathBuf::from("/tmp/unused"),
            dirty: false,
            load_rx: None,
        };
        h.record("ls", None, None, true);
        h.record("ls", None, None, false);
        assert_eq!(h.entries.len(), 1);
        assert!(!h.entries[0].success);
    }

    #[test]
    fn search_filters() {
        let mut h = CommandHistory {
            entries: VecDeque::new(),
            persist_path: PathBuf::from("/tmp/unused"),
            dirty: false,
            load_rx: None,
        };
        h.record("docker ps", None, Some("srv"), true);
        h.record("git status", None, None, true);
        let r = h.search("dock", true);
        assert_eq!(r.len(), 1);
        assert!(r[0].command.contains("docker"));
    }
}
