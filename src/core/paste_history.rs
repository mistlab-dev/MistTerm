//! 最近成功送入 PTY 的粘贴历史（#161）。
//!
//! 只保留最近 [`PASTE_HISTORY_CAP`] 条。单条超过 [`PASTE_ENTRY_MAX_BYTES`] 时按 UTF-8
//! 边界截断；截断标注只出现在界面上，`text` 里没有这段说明，再次粘贴不会把它送进终端。
//!
//! ## 敏感内容（不入库）
//!
//! 命中下列情况时**整段跳过**，不写入内存、也不落盘。粘贴本身仍走原来的
//! `build_paste_payload` / 分片路径：
//!
//! - PEM 私钥头（`BEGIN` + `PRIVATE KEY` / `OPENSSH PRIVATE KEY` / `PGP PRIVATE KEY`）
//! - AWS 访问密钥形态 `AKIA` + 16 位大写字母或数字
//! - GitHub token 前缀 `ghp_` / `gho_` / `github_pat_`
//! - Slack token 前缀 `xoxb-` / `xoxp-` / `xoxa-` / `xoxs-`
//! - 当前光标行像密码或口令提示（`password` / `passphrase` / `密码`）
//!
//! 不做预览脱敏：这些内容根本不进历史。关闭「记住粘贴历史」后不再写入；已有条目仍可查看并再次粘贴。
//!
//! 落盘与命令历史相同，走 `device_key` 加密的 `paste_history.json`。磁盘写入在后台线程，
//! 不占用 UI 线程；进程保存点会再同步刷一次，避免退出丢掉最后一条。

use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 内存与文件里保留的条数。
pub const PASTE_HISTORY_CAP: usize = 20;
/// 单条入库字节上限（按 UTF-8 字符边界截断）。
pub const PASTE_ENTRY_MAX_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PasteEntry {
    pub id: u64,
    /// 再次粘贴时送入 PTY 的正文（已截断时不含界面标注）。
    pub text: String,
    pub truncated: bool,
    pub original_bytes: usize,
    pub pasted_at: i64,
}

impl PasteEntry {
    pub fn pasted_at_local(&self) -> Option<DateTime<Local>> {
        Local.timestamp_opt(self.pasted_at, 0).single()
    }

    /// 列表单行预览。换行显示成 `⏎`。超长或已截断时末尾加 `…`，但那不是 `text` 的一部分。
    pub fn preview_oneline(&self, max_chars: usize) -> String {
        let flat = self.text.replace('\n', "⏎");
        let count = flat.chars().count();
        let mut s: String = flat.chars().take(max_chars).collect();
        if count > max_chars || self.truncated {
            if !s.ends_with('…') {
                s.push('…');
            }
        }
        s
    }
}

/// 从终端交付账本交到历史存储的一条（已经按上限截好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteHistoryNote {
    pub text: String,
    pub truncated: bool,
    pub original_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordResult {
    Stored,
    SkippedEmpty,
    RememberingDisabled,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    entries: Vec<PasteEntry>,
}

/// 按 payload 字节记账。整段 payload 都写进 PTY 之后才产生 [`PasteHistoryNote`]。
/// 不该入库的粘贴记成空洞，避免后面的正常粘贴被提前记账。
#[derive(Debug, Default)]
pub struct PasteDeliveryLedger {
    queue: VecDeque<LedgerSlot>,
}

#[derive(Debug)]
enum LedgerSlot {
    Gap {
        remaining: usize,
    },
    Entry {
        remaining: usize,
        note: PasteHistoryNote,
    },
}

impl PasteDeliveryLedger {
    pub fn push_gap(&mut self, payload_len: usize) {
        if payload_len == 0 {
            return;
        }
        self.queue.push_back(LedgerSlot::Gap {
            remaining: payload_len,
        });
    }

    pub fn push_entry(&mut self, note: PasteHistoryNote, payload_len: usize) {
        if payload_len == 0 {
            return;
        }
        self.queue.push_back(LedgerSlot::Entry {
            remaining: payload_len,
            note,
        });
    }

    /// `n` 字节省写成功。只有某条的 payload 全部写完才返回它。
    pub fn on_written(&mut self, mut n: usize) -> Vec<PasteHistoryNote> {
        let mut out = Vec::new();
        while n > 0 {
            let Some(front) = self.queue.front_mut() else {
                break;
            };
            match front {
                LedgerSlot::Gap { remaining } => {
                    let take = n.min(*remaining);
                    *remaining -= take;
                    n -= take;
                    if *remaining == 0 {
                        self.queue.pop_front();
                    } else {
                        break;
                    }
                }
                LedgerSlot::Entry { remaining, note } => {
                    let take = n.min(*remaining);
                    *remaining -= take;
                    n -= take;
                    if *remaining == 0 {
                        out.push(note.clone());
                        self.queue.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }
        out
    }

    /// 连接断开或硬失败：还没写完的条目丢弃。
    pub fn abandon(&mut self) {
        self.queue.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

pub struct PasteHistory {
    entries: VecDeque<PasteEntry>,
    persist_path: PathBuf,
    persist: bool,
    dirty: bool,
    load_rx: Option<mpsc::Receiver<VecDeque<PasteEntry>>>,
    save_rx: Option<mpsc::Receiver<io::Result<()>>>,
    next_id: u64,
}

impl PasteHistory {
    pub fn new() -> Self {
        let path = Self::default_persist_path();
        let (tx, rx) = mpsc::channel();
        let load_path = path.clone();
        thread::spawn(move || {
            let loaded = Self::load_entries(&load_path);
            let _ = tx.send(loaded);
        });
        Self {
            entries: VecDeque::new(),
            persist_path: path,
            persist: true,
            dirty: false,
            load_rx: Some(rx),
            save_rx: None,
            next_id: 1,
        }
    }

    /// 测试用：不读盘、不写盘。
    pub fn in_memory() -> Self {
        Self {
            entries: VecDeque::new(),
            persist_path: PathBuf::new(),
            persist: false,
            dirty: false,
            load_rx: None,
            save_rx: None,
            next_id: 1,
        }
    }

    /// 测试或显式路径：同步读盘，写入仍走后台线程。
    pub fn open_at(path: PathBuf) -> Self {
        let entries = Self::load_entries(&path);
        let mut history = Self {
            entries,
            persist_path: path,
            persist: true,
            dirty: false,
            load_rx: None,
            save_rx: None,
            next_id: 1,
        };
        history.recompute_next_id();
        history
    }

    pub fn default_persist_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("paste_history.json");
        p
    }

    pub fn poll_background_load(&mut self) -> bool {
        let Some(rx) = &self.load_rx else {
            return false;
        };
        match rx.try_recv() {
            Ok(loaded) => {
                let pending = if self.dirty {
                    std::mem::take(&mut self.entries)
                } else {
                    VecDeque::new()
                };
                self.entries = loaded;
                self.recompute_next_id();
                self.load_rx = None;
                if !pending.is_empty() {
                    for entry in pending {
                        self.push_loaded_pending(entry);
                    }
                    self.dirty = true;
                    self.kick_save();
                }
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

    pub fn poll_save(&mut self) {
        let finished = match &self.save_rx {
            Some(rx) => match rx.try_recv() {
                Ok(Ok(())) => true,
                Ok(Err(err)) => {
                    log::warn!("paste history save failed: {err}");
                    self.dirty = true;
                    true
                }
                Err(mpsc::TryRecvError::Empty) => false,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.dirty = true;
                    true
                }
            },
            None => false,
        };
        if finished {
            self.save_rx = None;
        }
        self.kick_save();
    }

    /// 退出 / eframe `save`：等在飞的后台写结束，再把仍脏的快照同步落盘。
    pub fn flush_blocking(&mut self) -> io::Result<()> {
        if !self.persist {
            return Ok(());
        }
        if let Some(rx) = self.save_rx.take() {
            match rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    log::warn!("paste history save failed: {err}");
                    self.dirty = true;
                }
                Err(_) => {
                    self.dirty = true;
                }
            }
        }
        if !self.dirty {
            return Ok(());
        }
        self.write_now()
    }

    /// 后台写盘已结束，且没有新的脏数据。
    pub fn is_persist_idle(&self) -> bool {
        self.save_rx.is_none() && !self.dirty
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries_newest_first(&self) -> impl Iterator<Item = &PasteEntry> {
        self.entries.iter().rev()
    }

    /// `0` 是最早写入、仍留在队列里的那条（连续粘贴时的「第 1 段」）。
    pub fn get_oldest(&self, index: usize) -> Option<&PasteEntry> {
        self.entries.get(index)
    }

    pub fn remove_id(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() != before {
            self.dirty = true;
            self.kick_save();
            true
        } else {
            false
        }
    }

    pub fn record_note(&mut self, note: PasteHistoryNote) -> RecordResult {
        if note.text.is_empty() {
            return RecordResult::SkippedEmpty;
        }
        if let Some(last) = self.entries.back_mut() {
            if last.text == note.text {
                last.pasted_at = Local::now().timestamp();
                last.truncated = note.truncated;
                last.original_bytes = note.original_bytes;
                self.dirty = true;
                self.kick_save();
                return RecordResult::Stored;
            }
        }
        let id = self.alloc_id();
        let pasted_at = Local::now().timestamp();
        self.entries.push_back(PasteEntry {
            id,
            text: note.text,
            truncated: note.truncated,
            original_bytes: note.original_bytes,
            pasted_at,
        });
        while self.entries.len() > PASTE_HISTORY_CAP {
            self.entries.pop_front();
        }
        self.dirty = true;
        self.kick_save();
        RecordResult::Stored
    }

    fn push_loaded_pending(&mut self, entry: PasteEntry) {
        if let Some(last) = self.entries.back_mut() {
            if last.text == entry.text {
                last.pasted_at = entry.pasted_at;
                last.truncated = entry.truncated;
                last.original_bytes = entry.original_bytes;
                return;
            }
        }
        let mut entry = entry;
        if self.entries.iter().any(|e| e.id == entry.id) {
            entry.id = self.alloc_id();
        } else if entry.id >= self.next_id {
            self.next_id = entry.id.saturating_add(1);
        }
        self.entries.push_back(entry);
        while self.entries.len() > PASTE_HISTORY_CAP {
            self.entries.pop_front();
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        if now > self.next_id {
            self.next_id = now;
        }
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    fn recompute_next_id(&mut self) {
        self.next_id = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
    }

    fn load_entries(path: &PathBuf) -> VecDeque<PasteEntry> {
        let file: HistoryFile = crate::security::encrypted_file::load_encrypted_json(path);
        let mut entries: VecDeque<PasteEntry> = file.entries.into_iter().collect();
        while entries.len() > PASTE_HISTORY_CAP {
            entries.pop_front();
        }
        entries
    }

    fn kick_save(&mut self) {
        if !self.persist || !self.dirty || self.load_rx.is_some() || self.save_rx.is_some() {
            return;
        }
        let snapshot = HistoryFile {
            entries: self.entries.iter().cloned().collect(),
        };
        let path = self.persist_path.clone();
        let (tx, rx) = mpsc::channel();
        self.dirty = false;
        self.save_rx = Some(rx);
        thread::spawn(move || {
            let result = crate::security::encrypted_file::save_encrypted_json(&path, &snapshot);
            let _ = tx.send(result);
        });
    }

    fn write_now(&mut self) -> io::Result<()> {
        let snapshot = HistoryFile {
            entries: self.entries.iter().cloned().collect(),
        };
        crate::security::encrypted_file::save_encrypted_json(&self.persist_path, &snapshot)?;
        self.dirty = false;
        Ok(())
    }
}

impl Default for PasteHistory {
    fn default() -> Self {
        Self::in_memory()
    }
}

/// 偏好关闭时不写入。调用方仍应先判断敏感内容。
pub fn accept_note(
    remember: bool,
    history: &mut PasteHistory,
    note: PasteHistoryNote,
) -> RecordResult {
    if !remember {
        return RecordResult::RememberingDisabled;
    }
    history.record_note(note)
}

/// 把粘贴正文裁到单条上限。返回的字符串是原文的 UTF-8 前缀，不含截断标注。
pub fn clip_for_history(text: &str) -> (String, bool, usize) {
    clip_for_history_with_limit(text, PASTE_ENTRY_MAX_BYTES)
}

pub fn clip_for_history_with_limit(text: &str, max_bytes: usize) -> (String, bool, usize) {
    let original_bytes = text.len();
    if original_bytes <= max_bytes {
        return (text.to_string(), false, original_bytes);
    }
    let end = floor_char_boundary(text, max_bytes);
    (text[..end].to_string(), true, original_bytes)
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

pub fn format_byte_len(n: usize) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

/// 光标行像密码 / 口令提示时，这次粘贴不进历史。
pub fn cursor_line_looks_like_secret_prompt(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains("password") || lower.contains("passphrase") || lower.contains("密码")
}

/// 正文像私钥或常见令牌时，整段不进历史。
pub fn looks_like_secret(text: &str) -> bool {
    // 不分配整段大写副本，避免大粘贴在 UI 线程上再拷一份。
    if contains_ascii_ignore_case(text, "-----BEGIN ")
        && (contains_ascii_ignore_case(text, "PRIVATE KEY")
            || contains_ascii_ignore_case(text, "PGP PRIVATE KEY"))
    {
        return true;
    }
    if contains_aws_access_key(text) {
        return true;
    }
    if text.contains("ghp_") || text.contains("gho_") || text.contains("github_pat_") {
        return true;
    }
    text.contains("xoxb-")
        || text.contains("xoxp-")
        || text.contains("xoxa-")
        || text.contains("xoxs-")
}

fn contains_ascii_ignore_case(haystack: &str, needle: &str) -> bool {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    h.windows(n.len()).any(|window| {
        window
            .iter()
            .zip(n.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

pub fn should_record_paste(text: &str, cursor_line: &str) -> bool {
    !text.is_empty()
        && !cursor_line_looks_like_secret_prompt(cursor_line)
        && !looks_like_secret(text)
}

fn contains_aws_access_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let mut i = 0;
    while i + 20 <= bytes.len() {
        if &bytes[i..i + 4] == b"AKIA"
            && bytes[i + 4..i + 20]
                .iter()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return true;
        }
        i += 1;
    }
    false
}

/// 终端侧：决定这条 payload 是记历史还是只占字节空洞。
pub fn note_for_payload(text: &str, cursor_line: &str) -> Option<PasteHistoryNote> {
    if !should_record_paste(text, cursor_line) {
        return None;
    }
    let (clipped, truncated, original_bytes) = clip_for_history(text);
    Some(PasteHistoryNote {
        text: clipped,
        truncated,
        original_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(text: &str) -> PasteHistoryNote {
        let (clipped, truncated, original_bytes) = clip_for_history(text);
        PasteHistoryNote {
            text: clipped,
            truncated,
            original_bytes,
        }
    }

    #[test]
    fn three_pastes_keep_first_for_repaste() {
        let mut history = PasteHistory::in_memory();
        history.record_note(note("10.0.0.1\n"));
        history.record_note(note("echo a\necho b\n"));
        history.record_note(note("/var/log/syslog"));
        assert_eq!(history.len(), 3);
        assert_eq!(history.get_oldest(0).unwrap().text, "10.0.0.1\n");
    }

    #[test]
    fn cap_drops_oldest() {
        let mut history = PasteHistory::in_memory();
        for i in 0..PASTE_HISTORY_CAP + 1 {
            history.record_note(note(&format!("item-{i}")));
        }
        assert_eq!(history.len(), PASTE_HISTORY_CAP);
        assert_eq!(history.get_oldest(0).unwrap().text, "item-1");
        assert_eq!(
            history.entries_newest_first().next().unwrap().text,
            format!("item-{PASTE_HISTORY_CAP}")
        );
    }

    #[test]
    fn over_byte_cap_truncates_on_char_boundary_without_marker_in_text() {
        let raw = "中".repeat(20_000);
        let (clipped, truncated, original) = clip_for_history(&raw);
        assert!(truncated);
        assert!(original > PASTE_ENTRY_MAX_BYTES);
        assert!(clipped.len() <= PASTE_ENTRY_MAX_BYTES);
        assert!(raw.starts_with(&clipped));
        assert!(!clipped.contains("truncated"));
        assert!(!clipped.contains("已截断"));
        let mut history = PasteHistory::in_memory();
        history.record_note(note(&raw));
        let entry = history.get_oldest(0).unwrap();
        assert!(entry.truncated);
        assert!(entry.preview_oneline(40).ends_with('…'));
        assert!(!entry.text.contains('…') || raw.contains('…'));
    }

    #[test]
    fn remembering_off_does_not_write() {
        let mut history = PasteHistory::in_memory();
        history.record_note(note("keep"));
        let result = accept_note(false, &mut history, note("new"));
        assert_eq!(result, RecordResult::RememberingDisabled);
        assert_eq!(history.len(), 1);
        assert_eq!(history.get_oldest(0).unwrap().text, "keep");
    }

    #[test]
    fn secrets_are_not_recorded() {
        assert!(looks_like_secret(
            "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----"
        ));
        assert!(looks_like_secret("token ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(looks_like_secret("AKIAIOSFODNN7EXAMPLE"));
        assert!(!looks_like_secret("-----BEGIN PUBLIC KEY-----\nMIIB\n"));
        assert!(!looks_like_secret("echo hello"));
        assert!(!should_record_paste("hunter2", "[sudo] password for root:"));
        assert!(should_record_paste("10.1.1.1", "root@host:~# "));
        assert!(
            note_for_payload("-----BEGIN RSA PRIVATE KEY-----\nabc", "root@host:~# ").is_none()
        );
    }

    #[test]
    fn ledger_emits_only_after_full_payload_and_skips_gaps() {
        let mut ledger = PasteDeliveryLedger::default();
        ledger.push_entry(note("first"), 4);
        ledger.push_gap(3);
        ledger.push_entry(note("third"), 2);
        assert!(ledger.on_written(3).is_empty());
        let done = ledger.on_written(1);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].text, "first");
        assert!(ledger.on_written(3).is_empty());
        let done = ledger.on_written(2);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].text, "third");
    }

    #[test]
    fn ledger_abandon_drops_partial() {
        let mut ledger = PasteDeliveryLedger::default();
        ledger.push_entry(note("partial"), 8);
        assert!(ledger.on_written(4).is_empty());
        ledger.abandon();
        assert!(ledger.on_written(4).is_empty());
        assert!(ledger.is_empty());
    }
}
