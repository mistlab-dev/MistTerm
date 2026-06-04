//! AI 对话持久化：上下文引用 source_key 字段兼容。

use mistterm::core::StoredContextRef;

#[test]
fn stored_context_ref_source_key_roundtrip() {
    let ctx = StoredContextRef {
        text: "err: timeout".into(),
        line_count: 1,
        char_count: 12,
        truncated: false,
        original_line_count: 1,
        original_char_count: 12,
        source_key: Some("monitor".into()),
    };
    let json = serde_json::to_string(&ctx).unwrap();
    assert!(json.contains("monitor"));
    let back: StoredContextRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.source_key.as_deref(), Some("monitor"));
}

#[test]
fn stored_context_ref_legacy_json_without_source_key() {
    let json = r#"{
        "text": "line",
        "line_count": 1,
        "char_count": 4,
        "truncated": false,
        "original_line_count": 1,
        "original_char_count": 4
    }"#;
    let back: StoredContextRef = serde_json::from_str(json).unwrap();
    assert!(back.source_key.is_none());
}
