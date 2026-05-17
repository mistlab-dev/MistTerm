//! 终端命令执行历史（Ctrl+R 搜索）

use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::PathBuf;

const MEMORY_CAP: usize = 500;
const FILE_CAP: usize = 1000;
const RETENTION_DAYS: i64 = 60;
const MAX_COMMAND_LEN: usize = 1000;

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
}

impl CommandHistory {
    pub fn new() -> Self {
        let path = Self::default_persist_path();
        let mut h = Self {
            entries: VecDeque::new(),
            persist_path: path,
            dirty: false,
        };
        h.load_from_disk();
        h
    }

    pub fn default_persist_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("command_history.json");
        p
    }

    pub fn load_from_disk(&mut self) {
        if !self.persist_path.exists() {
            return;
        }
        let Ok(content) = fs::read_to_string(&self.persist_path) else {
            self.entries.clear();
            return;
        };
        let Ok(file) = serde_json::from_str::<HistoryFile>(&content) else {
            self.entries.clear();
            return;
        };
        self.entries = file.entries.into_iter().collect();
        self.prune_old();
        while self.entries.len() > MEMORY_CAP {
            self.entries.pop_front();
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = self.persist_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut all: Vec<HistoryEntry> = self.entries.iter().cloned().collect();
        self.prune_vec(&mut all);
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
        let cmd = if cmd.len() > MAX_COMMAND_LEN {
            &cmd[..MAX_COMMAND_LEN]
        } else {
            cmd
        };
        if let Some(last) = self.entries.back() {
            if last.command == cmd {
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
            command: cmd.to_string(),
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

    fn prune_old(&mut self) {
        let cutoff = Local::now().timestamp() - RETENTION_DAYS * 86400;
        self.entries.retain(|e| e.executed_at >= cutoff);
    }

    fn prune_vec(&self, entries: &mut Vec<HistoryEntry>) {
        let cutoff = Local::now().timestamp() - RETENTION_DAYS * 86400;
        entries.retain(|e| e.executed_at >= cutoff);
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
        };
        h.record("ls", None, None, true);
        h.record("ls", None, None, true);
        assert_eq!(h.entries.len(), 1);
    }

    #[test]
    fn search_filters() {
        let mut h = CommandHistory {
            entries: VecDeque::new(),
            persist_path: PathBuf::from("/tmp/unused"),
            dirty: false,
        };
        h.record("docker ps", None, Some("srv"), true);
        h.record("git status", None, None, true);
        let r = h.search("dock", true);
        assert_eq!(r.len(), 1);
        assert!(r[0].command.contains("docker"));
    }
}
