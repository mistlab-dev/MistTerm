//! UI localization: English (default) and Simplified Chinese.
//!
//! Runtime strings use [`tr`] with egui context; logs should stay English only.

use std::borrow::Cow;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::core::credential::{CredentialAuthKind, CredentialCategory};
use crate::core::fragment::SortBy;
use crate::core::session_sort::SessionSortBy;

pub mod menu;

/// User-facing UI language (persisted in `settings.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    #[default]
    En,
    #[serde(rename = "zh")]
    Zh,
}

impl UiLanguage {
    pub const ALL: [UiLanguage; 2] = [UiLanguage::En, UiLanguage::Zh];

    pub fn label_in_self(self) -> &'static str {
        match self {
            UiLanguage::En => "English",
            UiLanguage::Zh => "简体中文",
        }
    }

    pub fn label_in_other(self) -> &'static str {
        match self {
            UiLanguage::En => "简体中文",
            UiLanguage::Zh => "English",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Locale {
    pub lang: UiLanguage,
}

impl Locale {
    #[inline]
    pub fn tr(self, en: &'static str, zh: &'static str) -> &'static str {
        match self.lang {
            UiLanguage::En => en,
            UiLanguage::Zh => zh,
        }
    }
}

impl From<UiLanguage> for Locale {
    fn from(lang: UiLanguage) -> Self {
        Self { lang }
    }
}

fn locale_id() -> egui::Id {
    // `Id::NULL` is reserved; use a stable app-specific id.
    egui::Id::new("mistterm_ui_language")
}

pub fn set_language(ctx: &egui::Context, lang: UiLanguage) {
    ctx.data_mut(|d| d.insert_temp(locale_id(), lang));
}

pub fn language(ctx: &egui::Context) -> UiLanguage {
    ctx.data(|d| d.get_temp::<UiLanguage>(locale_id()).unwrap_or_default())
}

pub fn locale(ctx: &egui::Context) -> Locale {
    Locale {
        lang: language(ctx),
    }
}

/// Pick `en` or `zh` for the current UI language (from egui context).
#[inline]
pub fn tr(ctx: &egui::Context, en: &'static str, zh: &'static str) -> &'static str {
    locale(ctx).tr(en, zh)
}

pub fn credential_category(ctx: &egui::Context, c: CredentialCategory) -> &'static str {
    match c {
        CredentialCategory::Server => tr(ctx, "Server account", "服务器账号"),
        CredentialCategory::Database => tr(ctx, "Database", "数据库"),
        CredentialCategory::SshKey => tr(ctx, "SSH key", "SSH 密钥"),
        CredentialCategory::Api => tr(ctx, "API / token", "API / 令牌"),
        CredentialCategory::Other => tr(ctx, "Other", "其他"),
    }
}

pub fn credential_auth_kind(ctx: &egui::Context, a: CredentialAuthKind) -> &'static str {
    match a {
        CredentialAuthKind::Password => tr(ctx, "Password", "密码"),
        CredentialAuthKind::SshKey => tr(ctx, "SSH key", "SSH 密钥"),
        CredentialAuthKind::Token => tr(ctx, "Token / API key", "令牌 / API Key"),
    }
}

pub fn session_sort_popup_row(ctx: &egui::Context, s: SessionSortBy) -> &'static str {
    match s {
        SessionSortBy::Name => tr(ctx, "Name (A→Z)", "名称 (A→Z)"),
        SessionSortBy::NameDesc => tr(ctx, "Name (Z→A)", "名称 (Z→A)"),
        SessionSortBy::LastConnected => tr(ctx, "Last connected", "最近连接"),
        SessionSortBy::CreatedAt => tr(ctx, "Created at", "创建时间"),
    }
}

pub fn session_sort_chip_short(ctx: &egui::Context, s: SessionSortBy) -> &'static str {
    match s {
        SessionSortBy::Name => tr(ctx, "A→Z", "A→Z"),
        SessionSortBy::NameDesc => tr(ctx, "Z→A", "Z→A"),
        SessionSortBy::LastConnected => tr(ctx, "Recent", "最近"),
        SessionSortBy::CreatedAt => tr(ctx, "Created", "创建"),
    }
}

pub fn fragment_sort_chip_short(ctx: &egui::Context, s: SortBy) -> &'static str {
    match s {
        SortBy::UsageCount => tr(ctx, "Uses", "次数"),
        SortBy::SuccessRate => tr(ctx, "Success rate", "成功率"),
        SortBy::LastUsed => tr(ctx, "Recent", "最近"),
        SortBy::Name => tr(ctx, "Name", "名称"),
    }
}

/// Tooltip on the session list sort chip (right side of sidebar filter row).
pub fn filter_sort_cycle_hint_sessions(ctx: &egui::Context) -> &'static str {
    tr(
        ctx,
        "Cycle sort: Name (A→Z) → Name (Z→A) → Last connected → Created",
        "点击切换排序：名称(A→Z) → 名称(Z→A) → 最近连接 → 创建",
    )
}

/// Tooltip on the fragment panel filter row sort chip.
pub fn session_log_status(ctx: &egui::Context, key: &str) -> &'static str {
    match key {
        "log_off" => tr(ctx, "Log off", "日志关"),
        _ => tr(ctx, "Log", "日志"),
    }
}

pub fn session_color_tag(ctx: &egui::Context, key: &str) -> String {
    match key {
        "" => tr(ctx, "None", "无").to_string(),
        "red" => tr(ctx, "Red", "红").to_string(),
        "yellow" => tr(ctx, "Yellow", "黄").to_string(),
        "green" => tr(ctx, "Green", "绿").to_string(),
        "blue" => tr(ctx, "Blue", "蓝").to_string(),
        "purple" => tr(ctx, "Purple", "紫").to_string(),
        "gray" => tr(ctx, "Gray", "灰").to_string(),
        other => other.to_string(),
    }
}

pub fn filter_sort_cycle_hint_fragments(ctx: &egui::Context) -> &'static str {
    tr(
        ctx,
        "Cycle sort: Usage → Success rate → Recent → Name",
        "点击切换排序：次数 → 成功率 → 最近 → 名称",
    )
}

/// Map common English backend errors to 简体中文 for status toasts (English passes through).
pub fn localize_backend_error(lang: UiLanguage, err: &str) -> String {
    if lang == UiLanguage::En {
        return err.to_string();
    }
    let s = err.trim();
    if s == "Vault is not enabled" {
        return "Vault 未启用".to_string();
    }
    if s == "API Key is empty" {
        return "API Key 为空".to_string();
    }
    if s == "Transfer cancelled by user" {
        return "传输已由用户取消".to_string();
    }
    if s == "Transfer cancelled by user (Ctrl+C)" {
        return "传输已由用户取消（Ctrl+C）".to_string();
    }
    if s == "Transfer already in progress" {
        return "传输已在进行中".to_string();
    }
    if let Some(rest) = s.strip_prefix("File does not exist:") {
        return format!("文件不存在：{rest}");
    }
    if let Some(rest) = s.strip_prefix("Failed to read file:") {
        return format!("读取文件失败：{rest}");
    }
    if let Some(rest) = s.strip_prefix("File not found:") {
        return format!("未找到文件：{rest}");
    }
    if let Some(rest) = s.strip_prefix("Could not open:") {
        return format!("无法打开：{rest}");
    }
    err.to_string()
}

/// Localize Rhai / fragment template errors for status messages.
pub fn localize_fragment_expr_error(lang: UiLanguage, err: &str) -> String {
    match lang {
        UiLanguage::En => err.to_string(),
        UiLanguage::Zh => {
            if let Some(rest) = err.strip_prefix("Unclosed {{ … }}") {
                return format!("未闭合的 {{ … }}{rest}");
            }
            if err == "{{ }} expression cannot be empty" {
                return "{{ }} 内表达式不能为空".to_string();
            }
            if let Some(rest) = err.strip_prefix("Expression error: ") {
                return format!("表达式错误：{rest}");
            }
            err.to_string()
        }
    }
}

/// UI label for a built-in theme (`Theme::name` stays 暗夜/晨曦/… in saved config).
pub fn theme_display_name(ctx: &egui::Context, stored_name: &str) -> Cow<'static, str> {
    match stored_name {
        "暗夜" => Cow::Borrowed(tr(ctx, "Midnight", "暗夜")),
        "晨曦" => Cow::Borrowed(tr(ctx, "Dawn", "晨曦")),
        "海洋" => Cow::Borrowed(tr(ctx, "Ocean", "海洋")),
        "森林" => Cow::Borrowed(tr(ctx, "Forest", "森林")),
        other => Cow::Owned(other.to_string()),
    }
}
