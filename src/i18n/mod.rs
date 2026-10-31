//! UI localization: English (default) and Simplified Chinese.
//!
//! Runtime strings use [`tr`] with egui context; logs should stay English only.

use std::borrow::Cow;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::core::credential::{CredentialAuthKind, CredentialCategory};
use crate::core::fragment::SortBy;
use crate::core::session_sort::SessionSortBy;

pub mod builtin_rules;
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

/// 会话「分组名」的展示层翻译。
///
/// 分组名持久化在本地 [`SessionConfig::group`](明文字符串)，早期版本直接写中文的
/// `"默认"` / `"团队"`；英文 UI 下会原封不动显示出中文。本函数把两个**保留词**归一化后走
/// 翻译，用户自定义分组名(非匹配)按原样返回。
pub fn session_group_display_name(ctx: &egui::Context, stored_name: &str) -> String {
    let norm = stored_name.trim();
    // 兼容中文旧数据与英文 UI 创建的分组名
    match norm {
        "默认" | "Default" | "" => tr(ctx, "Default (ungrouped)", "默认（未分组）").to_string(),
        "团队" | "Team" => tr(ctx, "Team", "团队").to_string(),
        other => other.to_string(),
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
    if s.starts_with("Transfer cancelled by user (") {
        return format!(
            "传输已由用户取消({})",
            crate::platform::terminal_interrupt_accel()
        );
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
    if s.starts_with("Failed to read local directory") || s.starts_with("read_dir entry:") {
        return "无法读取该目录，请检查路径和权限。".to_string();
    }
    if s.starts_with("Permission denied") {
        return "没有权限。".to_string();
    }
    if s.contains("Failed to create SFTP channel") {
        return "SFTP 子系统不可用。请检查远端 sshd 的 Subsystem sftp 后重试。".to_string();
    }
    if s.starts_with("Failed to create remote file") {
        return "无法在远端创建文件（目录可能只读或无权限）。".to_string();
    }
    if s.contains("Failed to write remote file") || s.contains("Failed to write to remote") {
        return "写入远端文件失败（可能无写权限）。".to_string();
    }
    if let Some(rest) = s.strip_prefix("Failed to create directory:") {
        return format!("创建目录失败：{rest}");
    }
    if let Some(rest) = s.strip_prefix("Failed to delete:") {
        return format!("删除失败：{rest}");
    }
    if let Some(rest) = s.strip_prefix("Failed to rename:") {
        return format!("重命名失败：{rest}");
    }
    if let Some(rest) = s.strip_prefix("Failed to set permissions:") {
        return format!("设置权限失败：{rest}");
    }
    if s == "SFTP background worker stopped unexpectedly" {
        return "SFTP 后台任务异常中断".to_string();
    }
    if s.contains("shell pump queue disconnected") {
        return "SSH shell 通道已断开，请重新连接会话后再试。".to_string();
    }
    if s.contains("shell pump queue full") {
        return "会话正忙，请稍后再试。".to_string();
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
        other if crate::ui::theme::is_deprecated_dark_theme_storage_name(other) => {
            Cow::Borrowed(tr(ctx, "Midnight", "暗夜"))
        }
        other => Cow::Owned(other.to_string()),
    }
}

/// Built-in starter snippet titles (id → EN / ZH). Covers already-seeded personal libraries
/// that only stored the Chinese `title` from an older starter catalog.
fn starter_fragment_title_pair(id: &str, title: &str) -> Option<(&'static str, &'static str)> {
    let by_id = match id {
        "384781c1-2ae5-44b4-ba69-e0cfcea999e4" => Some(("Disk usage", "磁盘使用")),
        "351fe9f9-41b1-4e01-a272-3566c41dccee" => Some(("Memory usage", "内存使用")),
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890" => Some(("System uptime", "系统运行时间")),
        "b2c3d4e5-f6a7-8901-bcde-f12345678901" => Some(("Listening ports", "监听端口")),
        "c3d4e5f6-a7b8-9012-cdef-123456789012" => Some(("Recent error logs", "最近错误日志")),
        "bee657c6-b09f-4e9b-8869-7e2a04a6d9b6" => Some(("Top processes", "查看进程")),
        "103494ee-6839-4755-8342-b80e391c8563" => Some(("Docker containers", "Docker 容器")),
        "d4e5f6a7-b8c9-0123-def0-234567890123" => Some(("Disk usage top", "磁盘占用 Top")),
        "e5f6a7b8-c9d0-1234-ef01-345678901234" => Some(("Failed systemd units", "失败 systemd 单元")),
        "f6a7b8c9-d0e1-2345-f012-456789012345" => Some(("Load and CPU overview", "负载与 CPU 概览")),
        _ => None,
    };
    if by_id.is_some() {
        return by_id;
    }
    match title.trim() {
        "磁盘使用" => Some(("Disk usage", "磁盘使用")),
        "内存使用" => Some(("Memory usage", "内存使用")),
        "系统运行时间" => Some(("System uptime", "系统运行时间")),
        "监听端口" => Some(("Listening ports", "监听端口")),
        "最近错误日志" => Some(("Recent error logs", "最近错误日志")),
        "查看进程" => Some(("Top processes", "查看进程")),
        "Docker 容器" => Some(("Docker containers", "Docker 容器")),
        "磁盘占用 Top" => Some(("Disk usage top", "磁盘占用 Top")),
        "失败 systemd 单元" => Some(("Failed systemd units", "失败 systemd 单元")),
        "负载与 CPU 概览" => Some(("Load and CPU overview", "负载与 CPU 概览")),
        _ => None,
    }
}

/// Display title for a snippet: prefer `title_en` / starter map under English UI.
pub fn fragment_display_title(ctx: &egui::Context, frag: &crate::core::fragment::FragmentStats) -> String {
    match language(ctx) {
        UiLanguage::Zh => frag.title.clone(),
        UiLanguage::En => {
            if let Some(en) = frag.title_en.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                return en.to_string();
            }
            if let Some((en, _)) = starter_fragment_title_pair(&frag.id, &frag.title) {
                return en.to_string();
            }
            frag.title.clone()
        }
    }
}

/// Display category for a snippet (starter Chinese categories → English under EN UI).
pub fn fragment_display_category(ctx: &egui::Context, frag: &crate::core::fragment::FragmentStats) -> String {
    match language(ctx) {
        UiLanguage::Zh => {
            match frag.category.trim() {
                "" => "—".to_string(),
                "from-history" | "From history" => "来自历史记录".to_string(),
                "recommended" => "推荐".to_string(),
                "from-failure" => "来自失败命令".to_string(),
                other => other.to_string(),
            }
        }
        UiLanguage::En => {
            if let Some(en) = frag
                .category_en
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return en.to_string();
            }
            match frag.category.trim() {
                "" => "—".to_string(),
                "系统监控" | "System monitoring" => tr(ctx, "System monitoring", "系统监控").to_string(),
                "网络" | "Network" => tr(ctx, "Network", "网络").to_string(),
                "日志" | "Logs" => tr(ctx, "Logs", "日志").to_string(),
                "进程管理" | "Processes" => tr(ctx, "Processes", "进程管理").to_string(),
                "Docker" => "Docker".to_string(),
                "来自历史记录" | "From history" | "from-history" => {
                    tr(ctx, "From history", "来自历史记录").to_string()
                }
                "recommended" => tr(ctx, "Recommended", "推荐").to_string(),
                "from-failure" => tr(ctx, "From failures", "来自失败命令").to_string(),
                other => other.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localize_sftp_channel_and_readonly_zh() {
        let ch = localize_backend_error(
            UiLanguage::Zh,
            "Failed to create SFTP channel: blah",
        );
        assert!(ch.contains("SFTP 子系统"), "{ch}");
        let ro = localize_backend_error(
            UiLanguage::Zh,
            "Failed to create remote file /tmp/x: Permission denied",
        );
        assert!(ro.contains("只读") || ro.contains("无权限"), "{ro}");
        let wr = localize_backend_error(UiLanguage::Zh, "Failed to write remote file: EACCES");
        assert!(wr.contains("写"), "{wr}");
    }
}

/// True if `query` (already lowercased) matches the snippet under the current UI language.
pub fn fragment_matches_search(
    ctx: &egui::Context,
    frag: &crate::core::fragment::FragmentStats,
    query_lower: &str,
) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if frag.command.to_lowercase().contains(query_lower) {
        return true;
    }
    if fragment_display_title(ctx, frag)
        .to_lowercase()
        .contains(query_lower)
    {
        return true;
    }
    // Also match the stored (often Chinese) title so EN users can still find by 磁盘使用.
    frag.title.to_lowercase().contains(query_lower)
        || frag
            .title_en
            .as_deref()
            .map(|s| s.to_lowercase().contains(query_lower))
            .unwrap_or(false)
}
