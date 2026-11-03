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
