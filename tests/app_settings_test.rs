//! app_settings 单元测试
//!
//! 测试应用设置的序列化。

use mistterm::core::app_settings::AppSettings;

#[test]
fn app_settings_default() {
    let settings = AppSettings::default();
    assert_eq!(settings.ui_language, mistterm::i18n::UiLanguage::En);
    assert!(!settings.vault.enabled);
    assert!(settings.audit.enabled);
    assert!(!settings.ai.enabled);
}

#[test]
fn app_settings_serde_roundtrip() {
    let settings = AppSettings::default();

    let json = serde_json::to_string(&settings).unwrap();
    let deserialized: AppSettings = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.ui_language, settings.ui_language);
    assert_eq!(deserialized.vault.enabled, settings.vault.enabled);
    assert_eq!(deserialized.audit.enabled, settings.audit.enabled);
    assert_eq!(deserialized.ai.enabled, settings.ai.enabled);
}

#[test]
fn app_settings_debug() {
    let settings = AppSettings::default();
    let debug_str = format!("{:?}", settings);
    assert!(debug_str.contains("AppSettings"));
}