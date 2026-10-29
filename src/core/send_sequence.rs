//! 命名发送序列：把若干行按顺序发进**当前这一个已连接窗格**。
//!
//! 和命令片段不同：片段是带占位符、可检索的一条命令；这里是写死的按行键入。
//! 行与行之间只有可选的固定间隔，**不会**等远端提示符，也没有 expect。
//! 重放目标永远是一个窗格：调用方自己记下这个窗格，本模块不接受窗格列表。

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 行间固定等待的上限（毫秒）。0 表示上一行写完立刻写下一行。
pub const MAX_DELAY_MS: u32 = 10_000;
pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_BODY_BYTES: usize = 32 * 1024;
pub const MAX_LINES: usize = 200;
pub const MAX_SEQUENCES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceError {
    EmptyName,
    NameTooLong,
    DuplicateName,
    EmptyBody,
    BodyTooLarge,
    TooManyLines,
    DelayTooLong,
    TooMany,
    NotFound,
}

/// 一条已保存的发送序列。`body` 用 `\n` 分行；空行表示只按一次回车。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SendSequence {
    pub id: String,
    pub name: String,
    pub body: String,
    #[serde(default)]
    pub delay_ms: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SendSequenceFile {
    #[serde(default)]
    sequences: Vec<SendSequence>,
}

pub struct SendSequenceStore {
    path: PathBuf,
    sequences: Vec<SendSequence>,
}

impl SendSequenceStore {
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        let file: SendSequenceFile = crate::security::encrypted_file::load_encrypted_json(path);
        let mut sequences = file.sequences;
        if sequences.len() > MAX_SEQUENCES {
            sequences.truncate(MAX_SEQUENCES);
        }
        for seq in &mut sequences {
            seq.delay_ms = seq.delay_ms.min(MAX_DELAY_MS);
            seq.name = seq.name.trim().to_string();
        }
        Self {
            path: path.to_path_buf(),
            sequences,
        }
    }

    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("send_sequences.json");
        p
    }

    pub fn list(&self) -> &[SendSequence] {
        &self.sequences
    }

    pub fn get(&self, id: &str) -> Option<&SendSequence> {
        self.sequences.iter().find(|s| s.id == id)
    }

    pub fn create(
        &mut self,
        name: &str,
        body: &str,
        delay_ms: u32,
    ) -> Result<SendSequence, SequenceError> {
        if self.sequences.len() >= MAX_SEQUENCES {
            return Err(SequenceError::TooMany);
        }
        let seq = build_sequence(None, name, body, delay_ms, &self.sequences)?;
        self.sequences.insert(0, seq.clone());
        Ok(seq)
    }

    pub fn update(
        &mut self,
        id: &str,
        name: &str,
        body: &str,
        delay_ms: u32,
    ) -> Result<SendSequence, SequenceError> {
        if !self.sequences.iter().any(|s| s.id == id) {
            return Err(SequenceError::NotFound);
        }
        let others: Vec<SendSequence> = self
            .sequences
            .iter()
            .filter(|s| s.id != id)
            .cloned()
            .collect();
        let seq = build_sequence(Some(id.to_string()), name, body, delay_ms, &others)?;
        if let Some(slot) = self.sequences.iter_mut().find(|s| s.id == id) {
            *slot = seq.clone();
        }
        Ok(seq)
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.sequences.len();
        self.sequences.retain(|s| s.id != id);
        self.sequences.len() != before
    }

    pub fn save(&self) -> io::Result<()> {
        let file = SendSequenceFile {
            sequences: self.sequences.clone(),
        };
        crate::security::encrypted_file::save_encrypted_json(&self.path, &file)
    }
}

fn build_sequence(
    id: Option<String>,
    name: &str,
    body: &str,
    delay_ms: u32,
    existing: &[SendSequence],
) -> Result<SendSequence, SequenceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SequenceError::EmptyName);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(SequenceError::NameTooLong);
    }
    if existing.iter().any(|s| s.name == name) {
        return Err(SequenceError::DuplicateName);
    }
    if delay_ms > MAX_DELAY_MS {
        return Err(SequenceError::DelayTooLong);
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(SequenceError::BodyTooLarge);
    }
    let body = normalize_newlines(body);
    let lines = sequence_lines(&body);
    if lines.is_empty() {
        return Err(SequenceError::EmptyBody);
    }
    if lines.len() > MAX_LINES {
        return Err(SequenceError::TooManyLines);
    }
    Ok(SendSequence {
        id: id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: name.to_string(),
        body,
        delay_ms,
    })
}

pub fn normalize_newlines(body: &str) -> String {
    body.replace("\r\n", "\n").replace('\r', "\n")
}

/// 按编辑器里的行拆开。`ls\n` 只有一行；单独的 `\n` 是一次回车。中间的空行保留。
pub fn sequence_lines(body: &str) -> Vec<String> {
    normalize_newlines(body)
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// 一行键入：正文加回车。空行就是单独的 `\r`。
pub fn line_payload(line: &str) -> Vec<u8> {
    let mut payload = line.as_bytes().to_vec();
    payload.push(b'\r');
    payload
}

/// 每一行相对起点的固定等待（毫秒）。第 0 行是 0，之后每行加上 `delay_ms`。
/// 这里没有「等到出现提示符」的步骤。
pub fn replay_schedule(lines: &[String], delay_ms: u32) -> Vec<(u32, String)> {
    let delay = delay_ms.min(MAX_DELAY_MS);
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| ((i as u32).saturating_mul(delay), line.clone()))
        .collect()
}

/// 重放标记。0 表示没有进行中的序列。用来在标签被关掉或换位后仍找到原来那个窗格。
pub fn alloc_replay_mark() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_order_blank_lines_and_drop_trailing_newline() {
        assert_eq!(
            sequence_lines("echo a\r\n\r\nls -l\n"),
            vec!["echo a".to_string(), String::new(), "ls -l".to_string()]
        );
        assert_eq!(sequence_lines("ls\n"), vec!["ls".to_string()]);
        assert_eq!(
            sequence_lines("a\rb"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(sequence_lines("").is_empty());
        assert_eq!(sequence_lines("\n"), vec![String::new()]);
        assert_eq!(sequence_lines("\n\n"), vec![String::new(), String::new()]);
        assert_eq!(sequence_lines(" \n"), vec![String::new()]);
    }

    #[test]
    fn schedule_is_ordered_with_fixed_delay_and_no_prompt_wait() {
        let lines = sequence_lines("echo a\necho b\necho c\n");
        assert_eq!(
            replay_schedule(&lines, 250),
            vec![
                (0, "echo a".to_string()),
                (250, "echo b".to_string()),
                (500, "echo c".to_string()),
            ]
        );
        assert_eq!(
            replay_schedule(&lines, 0),
            vec![
                (0, "echo a".to_string()),
                (0, "echo b".to_string()),
                (0, "echo c".to_string()),
            ]
        );
        let capped = replay_schedule(&lines, MAX_DELAY_MS + 50);
        assert_eq!(capped[1].0, MAX_DELAY_MS);
        assert_eq!(capped.len(), 3);
    }

    #[test]
    fn line_payload_is_text_plus_cr() {
        assert_eq!(line_payload(""), vec![b'\r']);
        assert_eq!(line_payload("echo a"), b"echo a\r".to_vec());
        assert!(!line_payload("echo a").windows(2).any(|w| w == b"\x1b["));
    }

    #[test]
    fn replay_marks_are_unique_and_nonzero() {
        let a = alloc_replay_mark();
        let b = alloc_replay_mark();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn crud_roundtrip_preserves_lines_and_delay() {
        let path = std::env::temp_dir().join(format!(
            "mistterm-send-seq-{}-{}.json",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&path);
        let mut store = SendSequenceStore::load_from(&path);
        let created = store
            .create("boot", "echo a\r\necho b\n", 80)
            .expect("create");
        assert_eq!(created.delay_ms, 80);
        assert_eq!(
            sequence_lines(&created.body),
            vec!["echo a".to_string(), "echo b".to_string()]
        );
        store.save().expect("save");
        let loaded = SendSequenceStore::load_from(&path);
        let got = loaded.get(&created.id).expect("loaded");
        assert_eq!(got.name, "boot");
        assert_eq!(sequence_lines(&got.body), sequence_lines(&created.body));
        assert_eq!(got.delay_ms, 80);

        let mut store = loaded;
        assert!(matches!(
            store.create("boot", "echo c\n", 0),
            Err(SequenceError::DuplicateName)
        ));
        let updated = store
            .update(&created.id, "boot", "echo a\n\nuptime\n", 0)
            .expect("same name on update");
        assert_eq!(updated.id, created.id);
        assert_eq!(
            sequence_lines(&updated.body),
            vec!["echo a".to_string(), String::new(), "uptime".to_string()]
        );
        assert!(store.delete(&created.id));
        assert!(store.get(&created.id).is_none());
        store.save().expect("save after delete");
        let again = SendSequenceStore::load_from(&path);
        assert!(again.list().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_empty_overlong_and_too_many_lines() {
        let mut store = SendSequenceStore::load_from(Path::new("/does/not/matter-send-seq"));
        assert_eq!(
            store.create("  ", "echo a\n", 0).unwrap_err(),
            SequenceError::EmptyName
        );
        assert_eq!(
            store.create("ok", "", 0).unwrap_err(),
            SequenceError::EmptyBody
        );
        let blank = store.create("enter", "\n", 0).expect("one enter");
        assert_eq!(sequence_lines(&blank.body), vec![String::new()]);
        assert_eq!(
            store
                .create("ok", "echo a\n", MAX_DELAY_MS + 1)
                .unwrap_err(),
            SequenceError::DelayTooLong
        );
        let long_name: String = "名".repeat(MAX_NAME_CHARS + 1);
        assert_eq!(
            store.create(&long_name, "echo a\n", 0).unwrap_err(),
            SequenceError::NameTooLong
        );
        let many = "x\n".repeat(MAX_LINES + 1);
        assert_eq!(
            store.create("many", &many, 0).unwrap_err(),
            SequenceError::TooManyLines
        );
        let huge = "x".repeat(MAX_BODY_BYTES + 1);
        assert_eq!(
            store.create("huge", &huge, 0).unwrap_err(),
            SequenceError::BodyTooLarge
        );
    }
}
