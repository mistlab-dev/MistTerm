//! #161 粘贴历史：条数上限、截断、敏感内容跳过、再次粘贴与直接粘贴同一条组包路径。

use std::time::Duration;

use mistterm::core::paste_history::*;
use mistterm::ui::terminal_paste::{build_paste_payload, normalize_paste_newlines};

fn note_text(text: &str) -> PasteHistoryNote {
    let (clipped, truncated, original_bytes) = clip_for_history(text);
    PasteHistoryNote {
        text: clipped,
        truncated,
        original_bytes,
    }
}

#[test]
fn first_of_three_pastes_repastes_like_a_direct_paste() {
    let samples = ["10.0.0.8\r\n", "echo a\necho b\r\n", "/tmp/notes.txt"];
    let mut history = PasteHistory::in_memory();
    for sample in samples {
        let normalized = normalize_paste_newlines(sample);
        history.record_note(note_text(&normalized));
    }
    let first = history.get_oldest(0).expect("first paste").text.clone();
    assert_eq!(first, normalize_paste_newlines(samples[0]));
    for bracketed in [false, true] {
        assert_eq!(
            build_paste_payload(samples[0], bracketed),
            build_paste_payload(&first, bracketed),
            "bracketed={bracketed}"
        );
    }
}

#[test]
fn cap_drops_oldest_and_long_entry_is_marked_truncated() {
    let mut history = PasteHistory::in_memory();
    for i in 0..(PASTE_HISTORY_CAP + 1) {
        history.record_note(note_text(&format!("paste-{i}")));
    }
    assert_eq!(history.len(), PASTE_HISTORY_CAP);
    assert_eq!(history.get_oldest(0).unwrap().text, "paste-1");

    let long = format!("中文{}", "中".repeat(20_000));
    history.record_note(note_text(&long));
    let stored = history.entries_newest_first().next().unwrap();
    assert!(stored.truncated);
    assert!(stored.text.len() <= PASTE_ENTRY_MAX_BYTES);
    assert!(long.starts_with(&stored.text));
    assert!(!stored.text.contains("已截断"));
    assert!(stored.preview_oneline(24).contains('…'));
    assert_ne!(
        build_paste_payload(&long, false).len(),
        build_paste_payload(&stored.text, false).len()
    );
}

#[test]
fn disabled_remembering_keeps_old_entries_without_new_writes() {
    let mut history = PasteHistory::in_memory();
    history.record_note(note_text("already-saved"));
    assert_eq!(
        accept_note(false, &mut history, note_text("should-skip")),
        RecordResult::RememberingDisabled
    );
    assert_eq!(history.len(), 1);
    assert_eq!(history.get_oldest(0).unwrap().text, "already-saved");
    assert_eq!(
        accept_note(true, &mut history, note_text("after-on")),
        RecordResult::Stored
    );
    assert_eq!(history.len(), 2);
}

#[test]
fn secret_and_password_prompt_are_skipped() {
    assert!(looks_like_secret(
        "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----"
    ));
    assert!(note_for_payload("ghp_exampletokenvalue", "user@host:~$ ").is_none());
    assert!(note_for_payload("secret", "Password:").is_none());
    assert!(note_for_payload("192.168.0.10", "user@host:~$ ").is_some());
}

#[test]
fn full_payload_then_chunk_path_records_once() {
    let text = "echo hi\necho there\n";
    let payload = build_paste_payload(text, true);
    let mut ledger = PasteDeliveryLedger::default();
    let prepared = note_for_payload(text, "user@host:~$ ").unwrap();
    ledger.push_entry(prepared, payload.len());
    let mut ready = Vec::new();
    let mut rest = payload.len();
    while rest > 0 {
        let n = rest.min(4096);
        ready.extend(ledger.on_written(n));
        rest -= n;
    }
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].text, text);
    assert_eq!(build_paste_payload(&ready[0].text, true), payload);
    assert_eq!(
        build_paste_payload(&ready[0].text, false),
        build_paste_payload(text, false)
    );
}

#[test]
fn background_save_reloads_same_entries() {
    let path = std::env::temp_dir().join(format!(
        "mistterm-paste-history-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&path);
    let mut history = PasteHistory::open_at(path.clone());
    history.record_note(note_text("line1\nline2"));
    history.record_note(note_text("second"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !history.is_persist_idle() && std::time::Instant::now() < deadline {
        history.poll_save();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(history.is_persist_idle(), "background save did not finish");
    let loaded = PasteHistory::open_at(path.clone());
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.get_oldest(0).unwrap().text, "line1\nline2");
    assert_eq!(loaded.entries_newest_first().next().unwrap().text, "second");
    let _ = std::fs::remove_file(&path);
}
