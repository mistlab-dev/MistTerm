//! Unit tests for ai_settings
//!
//! Tests AI settings serialization, deserialization, and methods.

use mistterm::core::ai_settings::AiSettings;

#[test]
fn ai_settings_default() {
    let settings = AiSettings::default();
    assert!(!settings.enabled);
    assert_eq!(settings.base_url, "https://api.openai.com/v1");
    assert_eq!(settings.model, "gpt-4o-mini");
    assert_eq!(settings.timeout_secs, 60);
    assert_eq!(settings.max_tokens, 2048);
}

#[test]
fn ai_settings_has_no_api_key_by_default() {
    let settings = AiSettings::default();
    assert!(!settings.has_api_key());
    assert!(settings.load_api_key().is_none());
}

#[test]
fn ai_settings_has_api_key_when_set() {
    let mut settings = AiSettings::default();
    settings.set_api_key("sk-test123").unwrap();
    assert!(settings.has_api_key());
    assert_eq!(settings.load_api_key(), Some("sk-test123".into()));
}

#[test]
fn ai_settings_api_key_trims_whitespace() {
    let mut settings = AiSettings::default();
    settings.set_api_key("  sk-test456  ").unwrap();
    assert!(settings.has_api_key());
    assert_eq!(settings.load_api_key(), Some("sk-test456".into()));
}

#[test]
fn ai_settings_empty_api_key_not_has_key() {
    let mut settings = AiSettings::default();
    settings.set_api_key("   ").unwrap();
    assert!(!settings.has_api_key());
}

#[test]
fn ai_settings_chat_completions_url() {
    let settings = AiSettings::default();
    assert_eq!(
        settings.chat_completions_url(),
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn ai_settings_chat_completions_url_trims_slashes() {
    let mut settings = AiSettings::default();
    settings.base_url = "https://api.example.com/v1/".into();
    assert_eq!(
        settings.chat_completions_url(),
        "https://api.example.com/v1/chat/completions"
    );
}

#[test]
fn ai_settings_serde_roundtrip() {
    let mut settings = AiSettings::default();
    settings.enabled = true;
    settings.base_url = "https://custom.api.com/v1".into();
    settings.model = "gpt-4".into();
    settings.set_api_key("sk-secret").unwrap();

    let json = serde_json::to_string(&settings).unwrap();
    let deserialized: AiSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.enabled, true);
    assert_eq!(deserialized.base_url, "https://custom.api.com/v1");
    assert_eq!(deserialized.model, "gpt-4");
    assert!(deserialized.has_api_key());
}

#[test]
fn ai_settings_load_api_key_returns_clone() {
    let mut settings = AiSettings::default();
    settings.set_api_key("sk-original").unwrap();

    let key1 = settings.load_api_key();
    let key2 = settings.load_api_key();

    assert_eq!(key1, key2);
    assert_eq!(key1, Some("sk-original".into()));
}

#[test]
fn ai_settings_debug() {
    let settings = AiSettings::default();
    let debug_str = format!("{:?}", settings);
    assert!(debug_str.contains("AiSettings"));
}