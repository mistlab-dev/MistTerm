//! 会话新建/编辑表单状态（#301 P1a：从 app.rs 纯搬家）。
//!
//! 两个字段集几乎对称；edit 侧多 4 个 keepalive 字段，new 侧多 `validation_attempted`。
//! 保留两份独立 struct 而非合并为一份：新建与编辑弹窗理论上可并存，
//! 共享字段会在边沿场景互相踩（纯搬家原则，不做语义合并）。

use crate::core::session::SessionAuthMode;
use crate::i18n::Locale;
use crate::ui::vault_form::VaultSecretForm;

/// 新建会话表单草稿。
#[derive(Clone, Debug)]
pub struct NewSessionForm {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub port_str: String,
    pub username: String,
    pub password: String,
    pub group: String,
    pub color_tag: String,
    pub private_key_path: String,
    pub use_ssh_agent: bool,
    pub auth_mode: SessionAuthMode,
    pub validation_attempted: bool,
    pub proxy_jump: String,
    pub proxy_command: String,
    pub local_forwards_text: String,
    pub remote_forwards_text: String,
    pub dynamic_forwards_text: String,
    pub post_connect_command: String,
    pub vault: VaultSecretForm,
}

impl NewSessionForm {
    /// `locale` 仅用于「默认」分组的初始文案（保持与原 `new()` 相同的启动期本地化）。
    pub fn new(locale: Locale) -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: 22,
            port_str: "22".to_string(),
            username: String::new(),
            password: String::new(),
            group: locale.tr("Default", "默认").to_string(),
            color_tag: String::new(),
            private_key_path: String::new(),
            use_ssh_agent: true,
            auth_mode: SessionAuthMode::Standard,
            validation_attempted: false,
            proxy_jump: String::new(),
            proxy_command: String::new(),
            local_forwards_text: String::new(),
            remote_forwards_text: String::new(),
            dynamic_forwards_text: String::new(),
            post_connect_command: String::new(),
            vault: VaultSecretForm::default(),
        }
    }
}

/// 编辑会话表单草稿。
#[derive(Clone, Debug)]
pub struct EditSessionForm {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub port_str: String,
    pub username: String,
    pub password: String,
    pub group: String,
    pub private_key_path: String,
    pub use_ssh_agent: bool,
    pub auth_mode: SessionAuthMode,
    pub color_tag: String,
    pub keepalive_enabled: bool,
    pub keepalive_interval_secs: u32,
    pub keepalive_count_max: u8,
    pub keepalive_auto_reconnect: bool,
    pub proxy_jump: String,
    pub proxy_command: String,
    pub local_forwards_text: String,
    pub remote_forwards_text: String,
    pub dynamic_forwards_text: String,
    pub post_connect_command: String,
    pub vault: VaultSecretForm,
}

/// 编辑弹窗密码框的掩码哨兵：仅当会话确实存有非空密码时才填入 UI，
/// 表示「沿用原密码」。该哨兵绝不允许被当作真实密码写回存储（见
/// [`edit_password_to_store`]）。真实密码永不进 UI（FUNCTIONAL_SPEC §1.3.3）。
pub const EDIT_PASSWORD_MASK_SENTINEL: &str = "****";

/// 编辑弹窗打开时密码框的初始值。
///
/// 会话存有非空密码 → 填掩码哨兵（沿用语义）；没有存密码（含 vault 后端
/// 会话：密码字段恒为空）→ 留空，配合「可留空」占位提示，避免密钥登录的
/// 会话在编辑弹窗里看起来像已设密码。
pub fn edit_password_field_initial(stored_password: &str) -> String {
    if stored_password.is_empty() {
        String::new()
    } else {
        EDIT_PASSWORD_MASK_SENTINEL.to_string()
    }
}

/// 编辑弹窗保存时把密码框值换算成要落盘的密码。
///
/// 规则：空 或 掩码哨兵 → 沿用旧密码（`old_password`）；其它输入 → 采用
/// 新输入。这保证：从未设过密码的会话，保存时不残留掩码哨兵；改密码正常
/// 生效；未触碰密码框时旧密码原样保留。
pub fn edit_password_to_store(field_value: &str, old_password: &str) -> String {
    let trimmed = field_value.trim();
    if trimmed.is_empty() || trimmed == EDIT_PASSWORD_MASK_SENTINEL {
        old_password.to_string()
    } else {
        field_value.to_string()
    }
}

impl EditSessionForm {
    pub fn new(locale: Locale) -> Self {
        Self {
            id: None,
            name: String::new(),
            host: String::new(),
            port: 22,
            port_str: "22".to_string(),
            username: String::new(),
            password: String::new(),
            group: locale.tr("Default", "默认").to_string(),
            private_key_path: String::new(),
            use_ssh_agent: true,
            auth_mode: SessionAuthMode::Standard,
            color_tag: String::new(),
            keepalive_enabled: true,
            keepalive_interval_secs: 30,
            keepalive_count_max: 3,
            keepalive_auto_reconnect: true,
            proxy_jump: String::new(),
            proxy_command: String::new(),
            local_forwards_text: String::new(),
            remote_forwards_text: String::new(),
            dynamic_forwards_text: String::new(),
            post_connect_command: String::new(),
            vault: VaultSecretForm::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- edit_password_field_initial ----

    #[test]
    fn edit_field_initial_empty_stored_password_leaves_field_empty() {
        // 从未设过密码（纯密钥 / SSH Agent / vault 后端）的会话：
        // 编辑弹窗密码框必须真的为空，不得预填掩码。
        assert_eq!(edit_password_field_initial(""), "");
        assert!(edit_password_field_initial("")
            .chars()
            .all(|c| c != '*'));
    }

    #[test]
    fn edit_field_initial_nonempty_stored_password_uses_sentinel_not_plaintext() {
        let initial = edit_password_field_initial("s3cret-真实密码");
        assert_eq!(initial, EDIT_PASSWORD_MASK_SENTINEL);
        // 掩码值里不允许出现真实密码的任何片段。
        assert!(!initial.contains("s3cret"));
        assert!(!initial.contains("真实密码"));
    }

    #[test]
    fn edit_field_initial_never_contains_plaintext_for_long_password() {
        let stored = "a".repeat(64);
        let initial = edit_password_field_initial(&stored);
        assert_eq!(initial, "****");
        assert_ne!(initial, stored);
    }

    // ---- edit_password_to_store ----

    #[test]
    fn store_unchanged_sentinel_preserves_old_password() {
        // 已有密码的会话，打开弹窗填了哨兵、用户没动 → 旧密码原样保留。
        assert_eq!(
            edit_password_to_store(EDIT_PASSWORD_MASK_SENTINEL, "old-pass"),
            "old-pass"
        );
    }

    #[test]
    fn store_empty_field_on_password_session_preserves_old_password() {
        // 已有密码的会话，用户清空输入框 → 仍是沿用旧密码（不发明清密码协议）。
        assert_eq!(edit_password_to_store("", "old-pass"), "old-pass");
        assert_eq!(edit_password_to_store("   ", "old-pass"), "old-pass");
    }

    #[test]
    fn store_on_never_set_password_stays_empty_and_never_persists_sentinel() {
        // 密钥会话从未设密码：即使误填了哨兵，落盘也必须是空，绝不能把
        // "****" 存成真实密码。
        let stored = edit_password_to_store("", "");
        assert_eq!(stored, "");
        assert_ne!(stored, EDIT_PASSWORD_MASK_SENTINEL);

        let stored2 = edit_password_to_store(EDIT_PASSWORD_MASK_SENTINEL, "");
        assert_eq!(stored2, "");
        assert_ne!(stored2, EDIT_PASSWORD_MASK_SENTINEL);
    }

    #[test]
    fn store_typed_replacement_wins_over_old_password() {
        assert_eq!(
            edit_password_to_store("new-pass-新密码", "old-pass"),
            "new-pass-新密码"
        );
    }

    #[test]
    fn store_typed_replacement_works_even_if_it_resembles_sentinel_with_spaces() {
        // 前后带空格的哨兵也算「沿用」；但「****x」等其它输入按新密码处理。
        assert_eq!(edit_password_to_store("  ****  ", "old"), "old");
        assert_eq!(edit_password_to_store("****x", "old"), "****x");
        assert_eq!(edit_password_to_store("***", "old"), "***");
    }

    #[test]
    fn store_typed_new_password_not_masked_in_result() {
        let out = edit_password_to_store("typed-new", "");
        assert_eq!(out, "typed-new");
        assert!(out != EDIT_PASSWORD_MASK_SENTINEL);
    }
}
