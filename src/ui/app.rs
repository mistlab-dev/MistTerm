//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等。
//!
//! 传文件三种入口彼此独立：**终端内 `rz`+ZMODEM**、**SFTP 侧栏**、**工具栏「上传」SCP 直传**（另见 `TerminalView::start_upload_to_remote` 的 cat 直传 API）。

use eframe::egui;
use rfd::FileDialog;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use crate::core::{
    candidate_to_session, default_ssh_config_path, is_already_imported, parse_ssh_config_file,
    pending_imports, SshConfigParseResult,
    AppSettings, AuditCategory, AuditEvent, AuditLogger, AuditOutcome, CmdAuditAction,
    CmdAuditAlertRequest, CmdAuditCacheStore, CmdAuditEngine, CmdAuditResult, CommandHistory,
    CommandSendResult, Credential,
    CredentialAuthKind, SecretResolver, SessionLogSettings, SessionLogWriter, SecretBackend,
    TempKeyFile, spawn_cleanup_old_logs, DEFAULT_RETENTION_DAYS,
    SessionSortBy, SshConfigCandidate, command_preview, expand_command_template,
    expand_fragment_command_stages, expand_rhai_blocks, list_placeholder_keys, merge_rhai_context,
    apply_vault_for_team, append_dynamic_forward_line, append_local_forward_line,
    append_remote_forward_line, parse_dynamic_forwards_text, parse_local_forwards_text,
    parse_remote_forwards_text, parse_vault_credential_path, PortForwardKind,
    status_bar_summary, FragmentManager,
    FragmentStats,
    SessionConfig, SessionManager, SortBy, TeamService,
};
use crate::core::batch_exec::{
    run_batch_parallel, BatchExecJob, BatchExecRow, BatchTarget, TEAM_TARGET_PREFIX,
};
use crate::ssh::{JumpHop, SshConfig, parse_jump_chain, parse_jump_endpoint};
use crate::ui::batch_exec_dialog::{BatchExecDialog, BatchExecUiAction};
use crate::ui::command_history_overlay::{CommandHistoryAction, CommandHistoryOverlay};
use crate::ui::help_docs_dialog::{HelpDocsDialog, HelpPage};
use crate::ui::audit_log_dialog::AuditLogDialog;
use crate::ui::session_log_dialog::SessionLogDialog;
use crate::ui::team_members_dialog::TeamMembersDialog;
use crate::ui::vault_form::VaultSecretForm;
use crate::ui::ssh_config_import_dialog::SshConfigImportDialog;
use crate::ui::sidebar::Sidebar;
use crate::ui::terminal::TerminalView;
use crate::ui::monitor_panel::MonitorPanel;
use crate::ui::ai_panel::AiPanel;
use crate::ui::sftp_panel::SftpPanel;
use crate::ui::port_forward_panel::PortForwardPanel;
use crate::ui::theme::ThemeManager;
use crate::ui::fragment_library::FragmentLibraryState;
use crate::ui::credential_panel::{CredentialPanel, CredentialPanelAction};
use crate::ui::cloud_sync_panel::{CloudSyncPanel, CloudSyncDeps};
use crate::ui::team_fragment_dialog::{
    open_create_editor, open_edit_editor, show_team_fragment_conflict_modal,
    show_team_fragment_editor_modal, TeamFragmentConflictState, TeamFragmentEditorState,
};
use crate::ui::team_ui::TeamLoginForm;
use crate::ui::layout_util;
use crate::ui::tab_pane::{TabLayout, TerminalPane, TerminalTab};

/// eframe 自定义持久化键（RON）；与 egui 自带的窗口几何持久化并存（FUNCTIONAL_SPEC §8.1）
const MISTTERM_UI_STORAGE_KEY: &str = "mistterm_ui_v1";

/// 命令片段侧栏：个人库 vs 团队同步库。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FragmentListScope {
    #[default]
    Personal,
    Team,
    Market,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct MistTermUiPersist {
    sidebar_width: f32,
    sidebar_collapsed: bool,
    sidebar_user_dismissed_responsive: bool,
    #[serde(default)]
    auto_reconnect_enabled: bool,
    #[serde(default)]
    session_sort_by: SessionSortBy,
    #[serde(default = "default_session_log_enabled")]
    session_log_enabled: bool,
    #[serde(default = "default_keepalive_enabled_persist")]
    default_keepalive_enabled: bool,
    #[serde(default = "default_keepalive_interval_persist")]
    default_keepalive_interval_secs: u32,
    #[serde(default = "default_keepalive_count_max_persist")]
    default_keepalive_count_max: u8,
    #[serde(default)]
    session_log_retention_days: u32,
    #[serde(default)]
    session_log_include_ansi: bool,
    #[serde(default)]
    ssh_import_banner_dismissed: bool,
}

fn default_session_log_enabled() -> bool {
    true
}
fn default_keepalive_enabled_persist() -> bool {
    true
}
fn default_keepalive_interval_persist() -> u32 {
    30
}
fn default_keepalive_count_max_persist() -> u8 {
    3
}

fn truncate_status(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{}…", head)
    } else {
        head
    }
}

/// Leading marker for transient error status styling (invisible); avoids locale-sensitive `starts_with`.
pub(crate) const STATUS_ERROR_MARKER: char = '\u{200b}';

#[inline]
pub(crate) fn status_message_body(msg: &str) -> &str {
    msg.strip_prefix(STATUS_ERROR_MARKER).unwrap_or(msg)
}

pub(crate) fn status_message_wrap_error(display: impl Into<String>) -> String {
    let s = display.into();
    if s.starts_with(STATUS_ERROR_MARKER) {
        return s;
    }
    format!("{STATUS_ERROR_MARKER}{s}")
}

/// FUNCTIONAL_SPEC §7 快捷键单一真源（关于页与帮助共用；随平台显示 ⌘ 或 Ctrl）。
pub(crate) fn mistterm_functional_spec_shortcuts(ctx: &egui::Context) -> String {
    use crate::i18n::UiLanguage;
    use crate::platform::shortcuts as s;

    fn en() -> String {
        format!(
            "Keyboard shortcuts (primary: {})\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — switch to tab N\n\
             {} — next tab (Shift reverses)\n\
             {}\n\
             {}\n\
             {}\n\
             {} — search in terminal viewport\n\
             {} — Preferences\n\
             {} — About & this cheatsheet\n\
             {} — command history (in terminal)\n\
             {} — AI assistant panel\n\
             {} — send terminal selection to AI",
            s::primary_modifier_label(),
            s::help_line("N", "New session"),
            s::help_line("E", "Edit selected session"),
            s::help_line("T", "New terminal tab"),
            s::help_line("W", "Close current tab"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line("J", "Focus connection search"),
            s::help_line("K", "Focus snippet search"),
            format!("{} — Quick snippet picker", s::accel_shift("J")),
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::accel_shift("A"),
            s::accel_shift("L"),
        )
    }

    fn zh() -> String {
        format!(
            "键盘快捷键（主修饰键：{}）\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — 切换第 N 个标签\n\
             {} — 下一标签；加 Shift 为上一标签\n\
             {}\n\
             {}\n\
             {}\n\
             {} — 终端内搜索\n\
             {} — 偏好设置\n\
             {} — 关于与本说明\n\
             {} — 命令历史（终端内）\n\
             {} — AI 助手面板\n\
             {} — 终端选区发送到 AI",
            s::primary_modifier_label(),
            s::help_line("N", "新建会话"),
            s::help_line("E", "编辑所选会话"),
            s::help_line("T", "新终端标签"),
            s::help_line("W", "关闭当前标签"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line("J", "聚焦连接搜索"),
            s::help_line("K", "聚焦片段搜索"),
            s::accel_shift("J").to_owned() + " — 快速片段选择器",
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::accel_shift("A"),
            s::accel_shift("L"),
        )
    }

    match crate::i18n::language(ctx) {
        UiLanguage::En => en(),
        UiLanguage::Zh => zh(),
    }
}

/// 底栏 / 提示文案颜色：错误类用主题红，其余用弱文字色（避免顶栏大块告警色）
fn status_message_text_color(msg: &str, theme: &crate::ui::theme::Theme) -> egui::Color32 {
    let body = status_message_body(msg);
    if msg.starts_with(STATUS_ERROR_MARKER)
        || body.starts_with("Expression error")
        || body.starts_with("表达式错误")
        || body.starts_with("Insert failed")
        || body.starts_with("插入失败")
        || body.starts_with("Upload failed")
        || body.starts_with("上传失败")
        || body.starts_with("File upload failed")
        || body.starts_with("文件上传失败")
        || body.starts_with("Save failed")
        || body.starts_with("保存失败")
        || body.starts_with("Failed to parse credential")
        || body.starts_with("解析凭据失败")
        || body.starts_with("Failed to update session")
        || body.starts_with("更新会话失败")
        || (body.starts_with("ZMODEM") && body.contains("failed"))
        || (body.starts_with("SCP ") && body.contains("failed"))
        || (body.contains("ZMODEM") && body.contains("失败"))
        || (body.starts_with("SCP ") && body.contains("失败"))
    {
        theme.red_color()
    } else {
        theme.color_caption_text()
    }
}

/// 设计文档 §5.4：`{次数}次 · {成功率}%成功 · {耗时}s`
fn format_fragment_stats_line(ctx: &egui::Context, frag: &FragmentStats) -> String {
    if frag.usage_count == 0 {
        return crate::i18n::tr(ctx, "Unused", "未使用").to_string();
    }
    let rate = (frag.success_count as f32 / frag.usage_count as f32) * 100.0;
    let avg_s = frag.total_time_ms as f64 / frag.usage_count as f64 / 1000.0;
    format!(
        "{}{} · {:.0}%{} · {:.1}s",
        frag.usage_count,
        crate::i18n::tr(ctx, "×", "次"),
        rate,
        crate::i18n::tr(ctx, " success", "成功"),
        avg_s,
    )
}

fn localize_terminal_insert_fragment_error(ctx: &egui::Context, err: &str) -> String {
    match err {
        TerminalView::ERR_FRAGMENT_NOT_CONNECTED => {
            crate::i18n::tr(ctx, "Terminal not connected", "终端未连接").to_string()
        }
        TerminalView::ERR_FRAGMENT_NO_SSH_HANDLE => crate::i18n::tr(
            ctx,
            "SSH session handle unavailable",
            "连接句柄不可用",
        )
        .to_string(),
        s if let Some(rest) = s.strip_prefix(TerminalView::FRAGMENT_SEND_FAILED_PREFIX) => {
            format!(
                "{}: {}",
                crate::i18n::tr(ctx, "Send failed", "发送失败"),
                rest
            )
        }
        _ => err.to_string(),
    }
}

/// 流水线：先 `{{ … }}`（Rhai）再 `expand_command_template` 替换会话字段，避免 `{{ md5(<user>) }}` 被提前展开成非法 Rhai；仍含 `<key>` 时需用户填写。
const SESSION_PLACEHOLDER_KEYS: &[&str] = &[
    "host",
    "hostname",
    "user",
    "username",
    "port",
    "session",
    "session_name",
    "name",
];

pub(crate) fn placeholders_needing_user(template: &str) -> Vec<String> {
    list_placeholder_keys(template)
        .into_iter()
        .filter(|k| {
            !SESSION_PLACEHOLDER_KEYS.contains(&k.as_str())
        })
        .collect()
}

/// 变量输入对话框状态
#[derive(Clone, Debug, Default)]
pub struct FragmentVariableDialog {
    pub open: bool,
    pub fragment_id: Option<String>,
    pub fragment_title: String,
    pub values: std::collections::HashMap<String, String>,
    /// 为 true 时在终端内插入（粘贴）；为 false 时直接「执行」发送一行命令。
    pub paste_after_fill: bool,
    /// 发送前可编辑的最终命令（含 `{{ … }}` 时在确认时会再次求值）。
    pub command_edit: String,
    /// 点「插入/执行」时若 `{{ … }}` 展开失败，在弹窗内展示（避免用户以为按钮失灵）。
    pub last_finalize_error: Option<String>,
}

/// 「填写片段变量」弹窗关闭后的动作（插入终端 vs ⌘J 直接发送）
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FragmentVarsCompletion {
    /// 右侧栏逻辑：粘贴进终端并记 usage 统计
    #[default]
    PasteInsertStats,
    /// 快速选择器：合成一行后发出去
    QuickExecuteSend,
}

/// 快速片段选择器状态
#[derive(Clone, Debug, Default)]
pub struct FragmentQuickSelector {
    pub open: bool,
    pub search_query: String,
    pub selected_index: usize,
}

/// FUNCTIONAL_SPEC §8.2 窗口宽度档位（用于提示与底栏 chip）
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResponsiveLayoutBand {
    Narrow,
    Medium,
    Wide,
}

/// 主应用程序
pub struct MistTermApp {
    /// 会话管理器
    session_manager: SessionManager,
    /// 命令片段管理器
    fragment_manager: FragmentManager,

    /// 当前选中的会话 ID
    selected_session_id: Option<String>,
    
    /// 侧边栏状态
    sidebar_collapsed: bool,
    sidebar_width: f32,
    /// 用户曾主动折叠左侧连接栏；宽屏时响应式不自动展开（FUNCTIONAL_SPEC §8 注意）
    sidebar_user_dismissed_responsive: bool,
    /// 上一帧的响应式布局档位（窄 / 中 / 宽），仅用于检测变化
    last_responsive_layout_band: Option<ResponsiveLayoutBand>,
    
    /// 终端标签页
    tabs: Vec<TerminalTab>,
    active_tab: Option<usize>,
    
    /// 状态栏信息
    status_message: String,
    
    /// 是否显示新建会话对话框
    show_new_session_dialog: bool,
    show_edit_session_dialog: bool,
    show_about_dialog: bool,
    /// 原型 / 常见桌面习惯：⌘, 偏好设置（主题等）
    show_preferences_dialog: bool,
    show_fragments_dialog: bool,
    show_fragment_panel: bool,  // 命令片段侧边栏
    /// 本帧命令片段 `SidePanel` 槽位矩形（`ui.max_rect()`）
    fragment_panel_slot_rect: Option<egui::Rect>,
    /// 本帧任意右侧 dock（片段/SFTP/监控等）与主区交界的最左 **屏幕 x**（多栏时取 min，即贴主区的那条边）
    right_dock_outer_left_x: Option<f32>,
    show_monitor_panel: bool,   // 监控面板
    show_ai_panel: bool,
    show_ai_settings_dialog: bool,
    /// 终端视口搜索（当前屏缓冲，不含卷动历史）
    show_terminal_search: bool,
    /// 打开查找条后首帧聚焦输入框
    terminal_search_pending_focus: bool,
    terminal_search_query: String,
    terminal_search_ignore_case: bool,
    terminal_search_hits: Vec<crate::terminal::SearchHit>,
    terminal_search_cur: usize,
    show_sftp_panel: bool,       // SFTP 文件浏览器
    show_port_forward_panel: bool,
    /// 上次已同步 SFTP 列表的终端标签索引（切换标签时重置远端浏览状态）
    sftp_last_tab: Option<usize>,
    port_forward_last_tab: Option<usize>,
    /// 监控面板绑定的终端标签（切换标签时重新绑定当前 SSH 会话）
    monitor_last_tab: Option<usize>,
    
    /// 新建会话表单
    new_session_name: String,
    new_session_host: String,
    new_session_port: u16,
    new_session_port_str: String,
    new_session_username: String,
    new_session_password: String,
    new_session_group: String,
    new_session_color_tag: String,
    new_session_private_key_path: String,
    new_session_use_ssh_agent: bool,
    new_session_proxy_jump: String,
    new_session_proxy_command: String,
    new_session_local_forwards_text: String,
    new_session_remote_forwards_text: String,
    new_session_dynamic_forwards_text: String,
    new_session_vault: VaultSecretForm,

    edit_session_id: Option<String>,
    edit_session_name: String,
    edit_session_host: String,
    edit_session_port: u16,
    edit_session_port_str: String,
    edit_session_username: String,
    edit_session_password: String,
    edit_session_group: String,
    edit_session_private_key_path: String,
    edit_session_use_ssh_agent: bool,
    edit_session_color_tag: String,
    edit_session_keepalive_enabled: bool,
    edit_session_keepalive_interval_secs: u32,
    edit_session_keepalive_count_max: u8,
    edit_session_keepalive_auto_reconnect: bool,
    edit_session_proxy_jump: String,
    edit_session_proxy_command: String,
    edit_session_local_forwards_text: String,
    edit_session_remote_forwards_text: String,
    edit_session_dynamic_forwards_text: String,
    edit_session_vault: VaultSecretForm,
    sidebar_search_query: String,
    sidebar_filter: String,
    session_sort_by: SessionSortBy,
    fragment_search_query: String,

    /// 片段排序方式
    fragment_sort_by: SortBy,
    /// 变量输入对话框
    variable_dialog: FragmentVariableDialog,
    /// `show_fragment_vars_dialog` 确认后的行为（见 [`FragmentVarsCompletion`]）
    fragment_vars_completion: FragmentVarsCompletion,
    /// 快速片段选择器
    quick_selector: FragmentQuickSelector,

    monitor_panel: MonitorPanel,
    ai_panel: AiPanel,
    sftp_panel: SftpPanel,
    port_forward_panel: PortForwardPanel,
    fragment_library: FragmentLibraryState,
    credential_panel: CredentialPanel,
    cloud_sync_panel: CloudSyncPanel,
    team_service: TeamService,
    team_login_form: TeamLoginForm,
    team_fragment_editor: TeamFragmentEditorState,
    team_fragment_conflict: Option<TeamFragmentConflictState>,
    team_fragment_selected_id: Option<String>,
    fragment_list_scope: FragmentListScope,
    show_fragment_analytics_dialog: bool,
    fragment_analytics_snapshot: crate::core::FragmentAnalyticsDashboard,
    fragment_analytics_range: crate::core::FragmentAnalyticsTimeRange,
    fragment_usage_log: crate::core::FragmentUsageLog,
    fragment_recommendations: Vec<crate::core::FragmentRecommendation>,
    market_catalog: crate::core::MarketCatalogState,
    market_catalog_refresh_pending: bool,
    market_catalog_refresh_rx: Option<std::sync::mpsc::Receiver<crate::core::MarketCatalogState>>,
    market_catalog_query_fingerprint: (String, String),
    market_catalog_debounce_deadline: Option<std::time::Instant>,

    pending_fragment_id: Option<String>,
    pending_fragment_name: String,
    pending_fragment_command: String,
    /// 「填写片段变量」模态里用户可编辑的完整命令行
    pending_fragment_command_edit: String,
    pending_fragment_vars: Vec<(String, String)>,
    show_fragment_vars_dialog: bool,
    fragment_filter_category: String,
    /// 连接就绪后要插入的片段（标签索引、片段 id、命令）
    pending_fragment_insert: Option<(usize, Option<String>, String)>,

    /// 主题管理器
    theme_manager: ThemeManager,
    /// Vault / 审计等应用设置
    app_settings: AppSettings,
    audit_logger: AuditLogger,

    /// 网络断开后是否自动重连（偏好设置，§1.4）
    auto_reconnect_enabled: bool,
    /// ≥10MB 上传：待用户选择 SCP 或 ZMODEM 的本地路径
    large_upload_pending_path: Option<std::path::PathBuf>,

    command_history: CommandHistory,
    command_history_overlay: CommandHistoryOverlay,
    ssh_import_dialog: SshConfigImportDialog,
    ssh_config_candidates: Vec<SshConfigCandidate>,
    ssh_config_path: std::path::PathBuf,
    ssh_import_banner_dismissed: bool,
    title_ssh_import_dismissed: bool,
    /// macOS 系统菜单栏（`muda` / NSMenu）
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::platform::macos_menu::NativeAppMenu>,
    session_log_settings: SessionLogSettings,
    session_log_dialog: SessionLogDialog,
    audit_log_dialog: AuditLogDialog,
    team_members_dialog: TeamMembersDialog,
    help_docs_dialog: HelpDocsDialog,
    session_log_enabled: bool,
    default_keepalive_enabled: bool,
    default_keepalive_interval_secs: u32,
    default_keepalive_count_max: u8,

    /// FUNCTIONAL_SPEC §1.3.4：Delete 删除会话前的确认 `(session_id, display_name)`
    delete_session_confirm: Option<(String, String)>,
    /// §2.3.5：关闭仍连接/握手中的标签前确认
    close_tab_confirm_idx: Option<usize>,
    /// 团队命令审计本地引擎
    cmd_audit_engine: CmdAuditEngine,
    /// 敏感命令二次确认（标签索引、命令、匹配详情）
    cmd_audit_confirm: Option<CmdAuditConfirmState>,
    /// 批量多机 SSH 执行
    batch_exec_dialog: BatchExecDialog,
    batch_exec_rx: Option<std::sync::mpsc::Receiver<Vec<BatchExecRow>>>,
}

/// 命令审计确认弹窗状态
#[derive(Clone)]
struct CmdAuditConfirmState {
    tab_idx: usize,
    command: String,
    audit: CmdAuditResult,
    started: Instant,
}

impl MistTermApp {
    /// FUNCTIONAL_SPEC §8.2：窗口宽度 ≥ 此值视为「宽屏」，左侧可自动展开、右侧 dock 可打开
    const RESP_LAYOUT_WIDE_MIN_PX: f32 = 1200.0;
    /// §8.2：宽度 **小于** 此值为「窄屏」，左侧连接栏自动折叠且关闭所有右侧 dock
    const RESP_LAYOUT_NARROW_LT_PX: f32 = 800.0;

    /// 片段变量类弹窗：正文 / 单行输入 / 按钮统一字号（egui 默认 Body 往往偏大）
    /// 应用当前主题（由 ThemeManager 统一管理）
    fn apply_current_theme(&self, ctx: &egui::Context) {
        self.theme_manager.apply_theme(ctx);
    }

    // ── 通用 UI 辅助函数（统一字体大小和间距，按设计规范固定值） ──

    /// 表单字段标签（见 [`crate::ui::chrome::form_field_label`]）
    fn ui_field_label(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, text: &str) {
        crate::ui::chrome::form_field_label(ui, theme, text);
    }

    /// 单行表单输入（可读占位符 + 统一输入框样式）
    fn ui_form_singleline(
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        id: &str,
        text: &mut String,
        hint: &str,
        desired_width: f32,
        password: bool,
    ) {
        crate::ui::chrome::form_singleline_field(
            ui,
            theme,
            ui.make_persistent_id(id),
            text,
            hint,
            desired_width,
            password,
        );
    }

    /// 端口输入（与其它单行框同款样式，避免 DragValue 默认灰底）
    fn ui_form_port(
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        id: &str,
        port_str: &mut String,
        port: &mut u16,
        desired_width: f32,
    ) {
        let response = crate::ui::chrome::form_singleline_field(
            ui,
            theme,
            ui.make_persistent_id(id),
            port_str,
            "22",
            desired_width,
            false,
        );
        if response.changed() {
            let trimmed = port_str.trim();
            if let Ok(p) = trimmed.parse::<u16>() {
                *port = p.clamp(1, 65535);
            } else if trimmed.is_empty() {
                *port = 22;
                *port_str = "22".to_string();
            }
        }
    }

    #[inline]
    fn layout_window_width(ctx: &egui::Context) -> f32 {
        ctx.screen_rect().width()
    }

    #[inline]
    fn layout_band_from_width(w: f32) -> Option<ResponsiveLayoutBand> {
        if !w.is_finite() || w <= 0.0 {
            return None;
        }
        Some(if w < Self::RESP_LAYOUT_NARROW_LT_PX {
            ResponsiveLayoutBand::Narrow
        } else if w < Self::RESP_LAYOUT_WIDE_MIN_PX {
            ResponsiveLayoutBand::Medium
        } else {
            ResponsiveLayoutBand::Wide
        })
    }

    #[inline]
    fn right_dock_open_allowed(w: f32) -> bool {
        w.is_finite() && w >= Self::RESP_LAYOUT_WIDE_MIN_PX
    }

    /// 关闭所有右侧 `SidePanel`（不含居中 `Window` 如片段库弹窗）
    fn close_all_right_dock_panels(&mut self) {
        self.show_fragment_panel = false;
        self.show_monitor_panel = false;
        self.show_ai_panel = false;
        self.show_sftp_panel = false;
        self.show_port_forward_panel = false;
        self.credential_panel.open = false;
        self.cloud_sync_panel.open = false;
        self.monitor_last_tab = None;
        self.sftp_last_tab = None;
        self.port_forward_last_tab = None;
    }

    /// FUNCTIONAL_SPEC §8.2：按窗口宽度收折左栏与右侧 dock
    fn apply_responsive_layout(&mut self, ctx: &egui::Context) {
        let w = Self::layout_window_width(ctx);
        let Some(band) = Self::layout_band_from_width(w) else {
            return;
        };
        if w < Self::RESP_LAYOUT_NARROW_LT_PX {
            self.sidebar_collapsed = true;
            self.close_all_right_dock_panels();
        } else if w < Self::RESP_LAYOUT_WIDE_MIN_PX {
            self.close_all_right_dock_panels();
        } else if !self.sidebar_user_dismissed_responsive {
            self.sidebar_collapsed = false;
        }

        self.last_responsive_layout_band = Some(band);
    }

    /// 窗口宽度不足以打开右侧 dock 时的状态栏提示。
    fn narrow_window_right_dock_hint(ctx: &egui::Context, window_width: f32) -> String {
        use crate::i18n::{UiLanguage, language};
        match language(ctx) {
            UiLanguage::En => format!(
                "Window is narrow (~{:.0}px). Widen to {:.0}px+ to open the right dock",
                window_width,
                Self::RESP_LAYOUT_WIDE_MIN_PX,
            ),
            UiLanguage::Zh => format!(
                "窗口较窄（约 {:.0}px），拉宽到 {:.0}px 以上可打开右侧面板",
                window_width,
                Self::RESP_LAYOUT_WIDE_MIN_PX,
            ),
        }
    }

    fn narrow_window_fragment_panel_hint(ctx: &egui::Context, window_width: f32) -> String {
        use crate::i18n::{UiLanguage, language};
        let k = crate::platform::accel("K");
        match language(ctx) {
            UiLanguage::En => format!(
                "Window is narrow (~{:.0}px). Widen to {:.0}px+, then {k} for snippets sidebar",
                window_width,
                Self::RESP_LAYOUT_WIDE_MIN_PX,
            ),
            UiLanguage::Zh => format!(
                "窗口较窄（约 {:.0}px），拉宽到 {:.0}px 以上后再用 {k} 打开片段侧栏",
                window_width,
                Self::RESP_LAYOUT_WIDE_MIN_PX,
            ),
        }
    }

    fn format_reconnect_status(ctx: &egui::Context, s: crate::core::ReconnectStatus) -> String {
        use crate::i18n::{UiLanguage, language};
        match language(ctx) {
            UiLanguage::En => match s {
                crate::core::ReconnectStatus::GaveUp { max_attempts } => format!(
                    "Disconnected; auto-reconnect stopped after {max_attempts} attempts."
                ),
                crate::core::ReconnectStatus::Scheduled {
                    delay_secs,
                    attempt,
                    max_attempts,
                } => format!(
                    "Disconnected; auto-reconnect in {delay_secs}s ({attempt}/{max_attempts})."
                ),
            },
            UiLanguage::Zh => match s {
                crate::core::ReconnectStatus::GaveUp { max_attempts } => format!(
                    "连接已断开；自动重连已达 {max_attempts} 次上限"
                ),
                crate::core::ReconnectStatus::Scheduled {
                    delay_secs,
                    attempt,
                    max_attempts,
                } => format!(
                    "连接已断开，{delay_secs} 秒后将自动重连（{attempt}/{max_attempts}）"
                ),
            },
        }
    }

    /// 打开任意右侧 dock 前调用；不允许时写状态栏并返回 false
    fn ensure_right_dock_allowed_or_warn(&mut self, ctx: &egui::Context) -> bool {
        let w = Self::layout_window_width(ctx);
        if Self::right_dock_open_allowed(w) {
            true
        } else {
            self.status_message = Self::narrow_window_right_dock_hint(ctx, w);
            false
        }
    }

    /// 创建新的应用实例
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let app_settings = AppSettings::load();
        let team_service = TeamService::new(app_settings.team.clone());
        let boot_loc = crate::i18n::Locale::from(app_settings.ui_language);
        let audit_logger = AuditLogger::new(app_settings.audit.clone());
        let mut session_manager = SessionManager::new();
        let boot_diagnostics = session_manager.take_load_diagnostics().join("；");
        let sessions = session_manager.list_sessions();
        
        // 自动选择第一个会话
        let selected_session_id = sessions.first().map(|s| s.id.clone());

        // Load market cache first, then init fragments from market or fallback to defaults
        let _market_cache = crate::core::market::MarketFragmentCache::load();

        let mut app = Self {
            session_manager,
            fragment_manager: FragmentManager::load(&FragmentManager::default_config_path())
                .unwrap_or_else(|_| FragmentManager::init_from_market_or_defaults(Some(&_market_cache))),
            selected_session_id,
            sidebar_collapsed: false,
            sidebar_width: layout_util::default_sidebar_width(&cc.egui_ctx),
            sidebar_user_dismissed_responsive: false,
            last_responsive_layout_band: None,
            tabs: Vec::new(),
            active_tab: None,
            status_message: {
                let ready = boot_loc.tr("Ready", "就绪").to_string();
                let mut msg = if boot_diagnostics.is_empty() {
                    ready.clone()
                } else {
                    boot_diagnostics
                };
                if !crate::platform::cjk_font_loaded() {
                    let warn = boot_loc
                        .tr(
                            "CJK fonts not loaded; Chinese may render as boxes",
                            "未加载中文字体，界面中文可能显示为方框",
                        )
                        .to_string();
                    if msg.is_empty() || msg == ready {
                        msg = warn;
                    } else {
                        msg = format!(
                            "{}{}{}",
                            msg,
                            boot_loc.tr(" — ", "；"),
                            warn
                        );
                    }
                }
                msg
            },
            show_new_session_dialog: false,
            show_edit_session_dialog: false,
            show_about_dialog: false,
            show_preferences_dialog: false,
            show_fragments_dialog: false,
            show_fragment_panel: false,
            fragment_panel_slot_rect: None,
            right_dock_outer_left_x: None,
            show_monitor_panel: false,
            show_ai_panel: false,
            show_ai_settings_dialog: false,
            show_terminal_search: false,
            terminal_search_pending_focus: false,
            terminal_search_query: String::new(),
            terminal_search_ignore_case: true,
            terminal_search_hits: Vec::new(),
            terminal_search_cur: 0,
            show_sftp_panel: false,
            show_port_forward_panel: false,
            sftp_last_tab: None,
            port_forward_last_tab: None,
            monitor_last_tab: None,
            monitor_panel: MonitorPanel::new(),
            ai_panel: AiPanel::new(),
            sftp_panel: SftpPanel::new(),
            port_forward_panel: PortForwardPanel::new(),
            fragment_library: FragmentLibraryState::new(),
            credential_panel: CredentialPanel::new(),
            cloud_sync_panel: CloudSyncPanel::new(),
            team_service,
            team_login_form: TeamLoginForm::default(),
            team_fragment_editor: TeamFragmentEditorState::default(),
            team_fragment_conflict: None,
            team_fragment_selected_id: None,
            fragment_list_scope: FragmentListScope::Personal,
            show_fragment_analytics_dialog: false,
            fragment_analytics_snapshot: crate::core::FragmentAnalyticsDashboard::default(),
            fragment_analytics_range: crate::core::FragmentAnalyticsTimeRange::default(),
            fragment_usage_log: crate::core::FragmentUsageLog::load(),
            fragment_recommendations: Vec::new(),
            market_catalog: crate::core::MarketCatalogState::load(),
            market_catalog_refresh_pending: false,
            market_catalog_refresh_rx: None,
            market_catalog_query_fingerprint: (String::new(), String::new()),
            market_catalog_debounce_deadline: None,
            pending_fragment_id: None,
            pending_fragment_name: String::new(),
            pending_fragment_command: String::new(),
            pending_fragment_command_edit: String::new(),
            pending_fragment_vars: Vec::new(),
            show_fragment_vars_dialog: false,
            fragment_filter_category: "all".to_string(),
            pending_fragment_insert: None,
            new_session_name: String::new(),
            new_session_host: String::new(),
            new_session_port: 22,
            new_session_port_str: "22".to_string(),
            new_session_username: String::new(),
            new_session_password: String::new(),
            new_session_group: boot_loc.tr("Default", "默认").to_string(),
            new_session_color_tag: String::new(),
            new_session_private_key_path: String::new(),
            new_session_use_ssh_agent: true,
            new_session_proxy_jump: String::new(),
            new_session_proxy_command: String::new(),
            new_session_local_forwards_text: String::new(),
            new_session_remote_forwards_text: String::new(),
            new_session_dynamic_forwards_text: String::new(),
            new_session_vault: VaultSecretForm::default(),
            edit_session_id: None,
            edit_session_name: String::new(),
            edit_session_host: String::new(),
            edit_session_port: 22,
            edit_session_port_str: "22".to_string(),
            edit_session_username: String::new(),
            edit_session_password: String::new(),
            edit_session_group: boot_loc.tr("Default", "默认").to_string(),
            edit_session_private_key_path: String::new(),
            edit_session_use_ssh_agent: true,
            edit_session_color_tag: String::new(),
            edit_session_keepalive_enabled: true,
            edit_session_keepalive_interval_secs: 30,
            edit_session_keepalive_count_max: 3,
            edit_session_keepalive_auto_reconnect: true,
            edit_session_proxy_jump: String::new(),
            edit_session_proxy_command: String::new(),
            edit_session_local_forwards_text: String::new(),
            edit_session_remote_forwards_text: String::new(),
            edit_session_dynamic_forwards_text: String::new(),
            edit_session_vault: VaultSecretForm::default(),
            sidebar_search_query: String::new(),
            sidebar_filter: "all".to_string(),
            session_sort_by: SessionSortBy::default(),
            fragment_search_query: String::new(),
            fragment_sort_by: SortBy::UsageCount,
            variable_dialog: FragmentVariableDialog::default(),
            fragment_vars_completion: FragmentVarsCompletion::default(),
            quick_selector: FragmentQuickSelector::default(),
            theme_manager: ThemeManager::load(),
            app_settings,
            audit_logger,
            delete_session_confirm: None,
            close_tab_confirm_idx: None,
            cmd_audit_engine: CmdAuditEngine::new(),
            cmd_audit_confirm: None,
            batch_exec_dialog: BatchExecDialog::default(),
            batch_exec_rx: None,
            auto_reconnect_enabled: false,
            large_upload_pending_path: None,
            command_history: CommandHistory::new(),
            command_history_overlay: CommandHistoryOverlay::default(),
            ssh_import_dialog: SshConfigImportDialog::default(),
            ssh_config_candidates: Vec::new(),
            ssh_config_path: default_ssh_config_path(),
            ssh_import_banner_dismissed: false,
            title_ssh_import_dismissed: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            session_log_settings: SessionLogSettings::default(),
            session_log_dialog: SessionLogDialog::default(),
            audit_log_dialog: AuditLogDialog::default(),
            team_members_dialog: TeamMembersDialog::default(),
            help_docs_dialog: HelpDocsDialog::default(),
            session_log_enabled: true,
            default_keepalive_enabled: true,
            default_keepalive_interval_secs: 30,
            default_keepalive_count_max: 3,
        };

        if let Some(storage) = cc.storage {
            if let Some(p) =
                eframe::get_value::<MistTermUiPersist>(storage, MISTTERM_UI_STORAGE_KEY)
            {
                app.sidebar_width = layout_util::clamp_sidebar_width(p.sidebar_width);
                // 每次启动默认展开左侧「连接」栏（不恢复上次折叠状态）
                app.sidebar_collapsed = false;
                app.sidebar_user_dismissed_responsive = false;
                app.auto_reconnect_enabled = p.auto_reconnect_enabled;
                app.session_sort_by = p.session_sort_by;
                app.session_log_enabled = p.session_log_enabled;
                app.default_keepalive_enabled = p.default_keepalive_enabled;
                app.default_keepalive_interval_secs = p.default_keepalive_interval_secs;
                app.default_keepalive_count_max = p.default_keepalive_count_max;
                app.session_log_settings.retention_days =
                    if p.session_log_retention_days == 0 {
                        DEFAULT_RETENTION_DAYS
                    } else {
                        p.session_log_retention_days
                    };
                app.session_log_settings.include_ansi = p.session_log_include_ansi;
                app.ssh_import_banner_dismissed = p.ssh_import_banner_dismissed;
            }
        }
        app.session_log_settings.enabled = app.session_log_enabled;
        {
            let base = app.session_log_settings.base_dir.clone();
            let days = app.session_log_settings.retention_days;
            spawn_cleanup_old_logs(base, days);
        }
        app.refresh_ssh_config_candidates();
        if app.team_service.is_logged_in() {
            app.configure_team_audit_sink();
            app.apply_cmd_audit_cache_for_current_team();
            // 异步拉取团队详情，避免启动时阻塞 UI 线程；UI 渲染期间会先用本地缓存（current_team_detail = None 时回退到 state 名字）。
            app.team_service.spawn_refresh_current_team_detail();
            app.team_service.spawn_cmd_audit_sync();
        }

        app
    }

    fn apply_cmd_audit_cache_for_current_team(&mut self) {
        let Some(tid) = self.team_service.state.current_team_id.as_deref() else {
            return;
        };
        if let Some(payload) = CmdAuditCacheStore::load().payload_for_team(tid) {
            self.cmd_audit_engine.apply_sync(payload);
        }
    }

    fn persist_cmd_audit_cache(&self, team_id: &str, payload: &crate::core::CmdAuditSyncPayload) {
        let mut store = CmdAuditCacheStore::load();
        store.upsert_team(team_id, payload);
        if let Err(e) = store.save() {
            log::warn!("cmd_audit cache save failed: {}", e);
        }
    }

    fn configure_team_audit_sink(&mut self) {
        if !self.team_service.is_logged_in() {
            self.app_settings.audit.http.team_id.clear();
            return;
        }
        self.app_settings.audit.http.enabled = true;
        self.app_settings.audit.http.url = self.team_service.audit_events_url();
        if let Some(tok) = self.team_service.current_access_token() {
            self.app_settings.audit.http.bearer_token = tok;
        }
        self.app_settings.audit.http.team_id = self
            .team_service
            .state
            .current_team_id
            .clone()
            .unwrap_or_default();
        self.audit_logger
            .update_settings(self.app_settings.audit.clone());
    }

    fn report_cmd_audit_alert_to_team(
        &self,
        command: &str,
        audit: &CmdAuditResult,
        action_taken: &str,
    ) {
        let Some(team_id) = self.team_service.state.current_team_id.as_deref() else {
            return;
        };
        let (matched_rule, match_level) = audit
            .matches
            .first()
            .map(|m| (m.rule_id.clone(), m.level.clone()))
            .unwrap_or_else(|| (String::new(), "unknown".into()));
        self.team_service.spawn_cmd_audit_report_alert(
            team_id,
            CmdAuditAlertRequest {
                command: command.to_string(),
                matched_rule,
                match_level,
                action_taken: action_taken.to_string(),
            },
        );
    }

    fn record_cmd_audit_event(
        &mut self,
        action: &str,
        command: &str,
        audit: &CmdAuditResult,
        outcome: AuditOutcome,
    ) {
        let preview = command_preview(command, 200);
        let matches: Vec<serde_json::Value> = audit
            .matches
            .iter()
            .map(|m| {
                serde_json::json!({
                    "rule_id": m.rule_id,
                    "source": m.source,
                    "level": m.level,
                    "message": m.message,
                    "action": format!("{:?}", m.action).to_lowercase(),
                })
            })
            .collect();
        let mut ev = AuditEvent::new(AuditCategory::Command, action, outcome)
            .with_detail(serde_json::json!({
                "command_preview": preview,
                "policy_action": format!("{:?}", audit.action).to_lowercase(),
                "matches": matches,
            }));
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(idx) {
                let sid = tab.primary_session_id();
                ev = ev.with_resource(&sid);
                if let Some(s) = self.session_manager.get_session(&sid) {
                    ev = ev.with_detail(serde_json::json!({
                        "command_preview": command_preview(command, 200),
                        "host": s.host,
                        "policy_action": format!("{:?}", audit.action).to_lowercase(),
                        "matches": matches,
                    }));
                }
            }
        }
        self.audit_logger.record(ev);

        let action_taken = match action {
            "command.confirmed" => "confirmed",
            "command.alert" => "alert",
            _ => "blocked",
        };
        self.report_cmd_audit_alert_to_team(command, audit, action_taken);
    }

    /// 经命令审计后发送到指定标签的 PTY（拦截/确认在 UI 层处理）。
    pub(crate) fn send_audited_command_at(
        &mut self,
        ctx: &egui::Context,
        tab_idx: usize,
        command: &str,
    ) -> CommandSendResult {
        if tab_idx >= self.tabs.len() {
            return CommandSendResult::NotConnected;
        }
        let Some(pane) = self.tabs.get(tab_idx).and_then(|t| t.active_pane()) else {
            return CommandSendResult::NotConnected;
        };
        if !pane.terminal.is_connected() {
            return CommandSendResult::NotConnected;
        }
        let audit = self.cmd_audit_engine.check(command);
        match audit.action {
            CmdAuditAction::Block => {
                self.record_cmd_audit_event("command.blocked", command, &audit, AuditOutcome::Denied);
                let hint = audit
                    .matches
                    .first()
                    .map(|m| m.message.as_str())
                    .unwrap_or("");
                self.status_message = status_message_wrap_error(format!(
                    "{}: {} — {}",
                    crate::i18n::tr(ctx, "Command blocked", "命令已拦截"),
                    command_preview(command, 80),
                    if hint.is_empty() {
                        crate::i18n::tr(ctx, "blocked by team policy", "已被团队策略阻止")
                    } else {
                        hint
                    },
                ));
                return CommandSendResult::Blocked(audit);
            }
            CmdAuditAction::Confirm => {
                self.cmd_audit_confirm = Some(CmdAuditConfirmState {
                    tab_idx,
                    command: command.to_string(),
                    audit: audit.clone(),
                    started: Instant::now(),
                });
                return CommandSendResult::NeedsConfirm {
                    command: command.to_string(),
                    audit,
                };
            }
            CmdAuditAction::Alert => {
                self.record_cmd_audit_event("command.alert", command, &audit, AuditOutcome::Success);
            }
            CmdAuditAction::Allow => {}
        }
        if let Some(pane) = self.tabs.get_mut(tab_idx).and_then(|t| t.active_pane_mut()) {
            pane.terminal.send_command(command);
        }
        CommandSendResult::Sent
    }

    pub(crate) fn send_audited_command_active(
        &mut self,
        ctx: &egui::Context,
        command: &str,
    ) -> CommandSendResult {
        let Some(idx) = self.active_tab else {
            return CommandSendResult::NotConnected;
        };
        self.send_audited_command_at(ctx, idx, command)
    }

    fn confirm_cmd_audit(&mut self, ctx: &egui::Context, proceed: bool) {
        let Some(state) = self.cmd_audit_confirm.take() else {
            return;
        };
        if proceed {
            self.record_cmd_audit_event(
                "command.confirmed",
                &state.command,
                &state.audit,
                AuditOutcome::Success,
            );
            if let Some(pane) = self
                .tabs
                .get_mut(state.tab_idx)
                .and_then(|t| t.active_pane_mut())
            {
                pane.terminal.send_command(&state.command);
                self.status_message = terminal_command_status_message(ctx, &state.command);
            }
        } else {
            self.record_cmd_audit_event(
                "command.cancelled",
                &state.command,
                &state.audit,
                AuditOutcome::Denied,
            );
        }
    }

    fn poll_team_service(&mut self, ctx: &egui::Context) {
        let freq = self.cloud_sync_panel.settings.frequency_minutes;
        if self.team_service.poll(freq) {
            if self.team_service.pending_audit_login {
                self.team_service.pending_audit_login = false;
                self.audit_logger.record(AuditEvent::new(
                    AuditCategory::Auth,
                    "team.login",
                    AuditOutcome::Success,
                ));
            }
            if self.team_service.pending_audit_sync {
                self.team_service.pending_audit_sync = false;
                self.audit_logger.record(AuditEvent::new(
                    AuditCategory::Fragment,
                    "fragment.sync_pull",
                    AuditOutcome::Success,
                ));
            }
            if let Some(payload) = self.team_service.take_cmd_audit_sync_payload() {
                if let Some(tid) = self.team_service.state.current_team_id.clone() {
                    self.persist_cmd_audit_cache(&tid, &payload);
                }
                self.cmd_audit_engine.apply_sync(payload);
            }
            if self.team_service.is_logged_in()
                && !self.team_service.is_busy()
                && self.cmd_audit_engine.needs_sync()
            {
                self.team_service.spawn_cmd_audit_sync();
            }
            if self.team_service.take_pending_initial_sync() {
                self.configure_team_audit_sink();
                self.team_service.spawn_config_sync();
                self.team_service.spawn_cmd_audit_sync();
            }
            if self.team_service.take_pending_vault_apply() {
                self.apply_team_vault_from_sync();
            }
            if self.team_service.pending_fragment_sync_after_config
                && !self.team_service.is_busy()
            {
                self.team_service.pending_fragment_sync_after_config = false;
                self.team_service.spawn_sync_current_team();
            }
            if self.team_service.auth_expired {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "Team session expired — sign in again in Preferences",
                    "团队登录已过期，请在偏好设置中重新登录",
                )
                .to_string();
                self.team_service.auth_expired = false;
            }
            self.configure_team_audit_sink();
            ctx.request_repaint();
        }
    }

    fn refresh_ssh_config_candidates(&mut self) {
        self.ssh_config_candidates = if self.ssh_config_path.exists() {
            parse_ssh_config_file(&self.ssh_config_path)
                .map(|r| r.candidates)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
    }

    fn ssh_pending_import_count(&self) -> usize {
        pending_imports(
            &self.ssh_config_candidates,
            self.session_manager.list_sessions(),
        )
        .len()
    }

    fn open_ssh_import_dialog(&mut self, ctx: &egui::Context) {
        if !self.ssh_config_path.exists() {
            self.status_message = format!(
                "{} {}",
                crate::i18n::tr(ctx, "SSH config file not found:", "未找到 SSH 配置文件："),
                self.ssh_config_path.display()
            );
            return;
        }
        let parse = parse_ssh_config_file(&self.ssh_config_path).unwrap_or(SshConfigParseResult {
            candidates: Vec::new(),
            warnings: vec![crate::i18n::tr(
                ctx,
                "Unable to read SSH config file",
                "无法读取 SSH 配置文件",
            )
            .to_string()],
        });
        self.ssh_config_candidates = parse.candidates.clone();
        let existing = self.session_manager.list_sessions();
        let already_imported: Vec<bool> = parse
            .candidates
            .iter()
            .map(|c| is_already_imported(c, existing))
            .collect();
        if pending_imports(&parse.candidates, existing).is_empty()
            && parse.candidates.iter().any(|c| c.importable())
        {
            self.status_message = crate::i18n::tr(
                ctx,
                "All importable SSH config entries already exist",
                "所有可导入的 SSH 配置已存在",
            )
            .to_string();
        }
        self.ssh_import_dialog.set_candidates(
            parse.candidates,
            already_imported,
            parse.warnings,
        );
    }

    fn import_ssh_indices(&mut self, ctx: &egui::Context, indices: &[usize]) {
        let existing_names: Vec<String> = self
            .session_manager
            .list_sessions()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let mut names = existing_names;
        let mut added = 0usize;
        for &i in indices {
            let Some(c) = self.ssh_import_dialog.candidates.get(i) else {
                continue;
            };
            if !c.importable() {
                continue;
            }
            let mut cfg = candidate_to_session(c, &names);
            if !self.default_keepalive_enabled {
                cfg.keepalive_enabled = false;
            } else {
                cfg.keepalive_interval_secs = self.default_keepalive_interval_secs;
                cfg.keepalive_count_max = self.default_keepalive_count_max;
            }
            cfg.keepalive_auto_reconnect = self.auto_reconnect_enabled;
            let name = cfg.name.clone();
            names.push(name.clone());
            self.session_manager.add_session(cfg);
            added += 1;
        }
        if added > 0 {
            self.audit_logger.record(
                AuditEvent::new(AuditCategory::Session, "session.import_ssh", AuditOutcome::Success)
                    .with_detail(serde_json::json!({ "count": added })),
            );
            self.status_message = match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::En => format!("Imported {added} SSH profile(s)"),
                crate::i18n::UiLanguage::Zh => format!("已导入 {added} 个 SSH 配置"),
            };
            self.refresh_ssh_config_candidates();
        }
    }

    fn poll_connect_audit_from_tabs(&mut self) {
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
            if let Some((ok, host)) = pane.terminal.take_connect_audit() {
                let action = if ok {
                    "connect.success"
                } else {
                    "connect.failure"
                };
                let outcome = if ok {
                    AuditOutcome::Success
                } else {
                    AuditOutcome::Failure
                };
                self.audit_logger.record(
                    AuditEvent::new(AuditCategory::Session, action, outcome)
                        .with_host(&host)
                        .with_session(&pane.session_id),
                );
                if ok {
                    self.audit_logger.record(
                        AuditEvent::new(
                            AuditCategory::Session,
                            "session.connect",
                            AuditOutcome::Success,
                        )
                        .with_host(&host)
                        .with_session(&pane.session_id),
                );
                }
            }
            }
        }
    }

    fn terminal_connect_session(
        &mut self,
        ctx: &egui::Context,
        terminal: &mut TerminalView,
        session: &SessionConfig,
        temp_key: &mut Option<TempKeyFile>,
    ) {
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "shell.connect", AuditOutcome::Success)
                .with_host(&session.host)
                .with_session(&session.id),
        );
        let resolver = SecretResolver::new(self.app_settings.vault.clone());
        let resolved = match resolver.resolve_session(session) {
            Ok(r) => r,
            Err(e) => {
                self.audit_logger.record(
                    AuditEvent::new(
                        AuditCategory::Session,
                        "connect.resolve_failed",
                        AuditOutcome::Failure,
                    )
                    .with_host(&session.host)
                    .with_session(&session.id)
                    .with_detail(serde_json::json!({ "error": e.to_string() })),
                );
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Failed to resolve credentials:", "解析凭据失败："),
                    crate::i18n::localize_backend_error(crate::i18n::language(ctx), &e.to_string())
                );
                return;
            }
        };
        *temp_key = resolved.temp_key_file;
        let theme = self.theme_manager.current_theme();
        let (ka_on, ka_int, ka_max) = Self::session_keepalive_params(session);
        let jump_hops = match self.resolve_proxy_jump_hops(session) {
            Ok(h) => h,
            Err(e) => {
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "ProxyJump resolve failed:", "跳板解析失败："),
                    crate::i18n::localize_backend_error(crate::i18n::language(ctx), &e)
                );
                return;
            }
        };
        terminal.connect(
            theme,
            &session.host,
            session.port,
            &session.username,
            &resolved.password,
            &resolved.private_key_path,
            session.use_ssh_agent,
            ka_on,
            ka_int,
            ka_max,
            &session.proxy_jump,
            &session.proxy_command,
            jump_hops,
            parse_local_forwards_text(&session.local_forwards_text),
            parse_remote_forwards_text(&session.remote_forwards_text),
            parse_dynamic_forwards_text(&session.dynamic_forwards_text),
        );
    }

    /// 将 `ProxyJump` 各跳解析为连接凭据（匹配已保存会话名/主机，或 `user@host:port`）。
    fn resolve_proxy_jump_hops(&self, session: &SessionConfig) -> Result<Vec<JumpHop>, String> {
        let chain = parse_jump_chain(&session.proxy_jump);
        if chain.is_empty() {
            return Ok(Vec::new());
        }
        let resolver = SecretResolver::new(self.app_settings.vault.clone());
        let mut hops = Vec::with_capacity(chain.len());
        for token in &chain {
            if let Some(js) = self.session_manager.find_session_for_jump_token(token) {
                let resolved = resolver
                    .resolve_session(js)
                    .map_err(|e| format!("{} ({}): {}", token, js.name, e))?;
                hops.push(JumpHop {
                    host: js.host.clone(),
                    port: js.port,
                    username: js.username.clone(),
                    password: resolved.password,
                    private_key_path: resolved.private_key_path,
                    use_ssh_agent: js.use_ssh_agent,
                });
            } else {
                let ep = parse_jump_endpoint(token, &session.username)?;
                hops.push(JumpHop {
                    host: ep.host,
                    port: ep.port,
                    username: ep.username,
                    password: String::new(),
                    private_key_path: String::new(),
                    use_ssh_agent: session.use_ssh_agent,
                });
            }
        }
        Ok(hops)
    }

    fn session_keepalive_params(session: &SessionConfig) -> (bool, u32, u8) {
        if session.keepalive_enabled && session.keepalive_interval_secs > 0 {
            (
                true,
                session.keepalive_interval_secs,
                session.keepalive_count_max.max(1),
            )
        } else {
            (false, 0, session.keepalive_count_max.max(1))
        }
    }

    fn ensure_tab_log_writer(&mut self, tab_idx: usize) {
        if !self.session_log_enabled {
            return;
        }
        let settings = self.session_log_settings.clone();
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            return;
        };
        for pane in &mut tab.panes {
            if pane.log_writer.is_some() {
                continue;
            }
            let sid = pane.session_id.clone();
            let (name, host_line) = self
                .session_manager
                .get_session(&sid)
                .map(|s| {
                    (
                        s.name.clone(),
                        format!("{}@{}:{}", s.username, s.host, s.port),
                    )
                })
                .unwrap_or_else(|| (pane.title.clone(), String::new()));
            let mut writer = SessionLogWriter::new(sid, name, host_line, settings.clone());
            writer.write_connected();
            pane.log_writer = Some(writer);
        }
    }

    fn tab_auto_reconnect_enabled(&self, session_id: &str) -> bool {
        self.session_manager
            .get_session(session_id)
            .map(|s| s.keepalive_auto_reconnect)
            .unwrap_or(self.auto_reconnect_enabled)
    }

    fn poll_command_history_from_active_tab(&mut self) {
        let Some(idx) = self.active_tab else {
            return;
        };
        let (sid, sname, cmd) = {
            let Some(tab) = self.tabs.get_mut(idx) else {
                return;
            };
            let Some(pane) = tab.active_pane_mut() else {
                return;
            };
            let cmd = pane.terminal.take_submitted_line();
            let sid = pane.session_id.clone();
            let sname = self
                .session_manager
                .get_session(&sid)
                .map(|s| s.name.clone());
            (sid, sname, cmd)
        };
        if let Some(command) = cmd {
            let preview = command_preview(&command, 120);
            let detail = serde_json::json!({
                "preview": preview,
                "len": command.len(),
            });
            self.audit_logger.record(
                AuditEvent::new(AuditCategory::Command, "command.submit", AuditOutcome::Success)
                    .with_session(&sid)
                    .with_detail(detail.clone()),
            );
            self.audit_logger.record(
                AuditEvent::new(AuditCategory::Command, "shell.exec", AuditOutcome::Success)
                    .with_session(&sid)
                    .with_detail(detail),
            );
            self.command_history.record(
                &command,
                Some(&sid),
                sname.as_deref(),
                false,
            );
        }
    }

    fn poll_session_log_commands(&mut self) {
        if !self.session_log_enabled {
            return;
        }
        let tab_count = self.tabs.len();
        for i in 0..tab_count {
            self.ensure_tab_log_writer(i);
        }
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
                if let Some(writer) = pane.log_writer.as_mut() {
                    while let Some(command) = pane.terminal.take_pending_log_command() {
                        writer.write_prompt_marker(&command);
                    }
                }
            }
        }
    }

    fn append_terminal_output_logs(&mut self) {
        if !self.session_log_enabled {
            return;
        }
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
                if let Some(writer) = pane.log_writer.as_mut() {
                    while let Some(chunk) = pane.terminal.take_pending_log_output() {
                        writer.append_output(&chunk);
                    }
                }
            }
        }
    }

    fn flush_session_log_buffers_for_session(&mut self, session_id: &str) {
        if !self.session_log_enabled {
            return;
        }
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
                if pane.session_id == session_id {
                    if let Some(writer) = pane.log_writer.as_mut() {
                        writer.flush_pending_output();
                    }
                }
            }
        }
    }

    fn active_tab_log_status(&self, ctx: &egui::Context) -> Option<String> {
        let idx = self.active_tab?;
        let tab = self.tabs.get(idx)?;
        tab.active_pane()?.log_writer.as_ref().map(|w| {
            crate::i18n::session_log_status(ctx, w.status_label_key()).to_string()
        })
    }

    fn id_sidebar_connection_search() -> egui::Id {
        egui::Id::new("mistterm_sidebar_connection_search")
    }

    fn id_fragment_panel_search() -> egui::Id {
        egui::Id::new("mistterm_fragment_panel_search")
    }

    /// 存在模态/表单/查找条等需使用标准编辑快捷键（⌘C/V/A）的 UI
    fn global_shortcuts_blocked(&self) -> bool {
        self.show_new_session_dialog
            || self.show_edit_session_dialog
            || self.show_about_dialog
            || self.show_preferences_dialog
            || self.show_fragments_dialog
            || self.show_fragment_vars_dialog
            || self.show_fragment_analytics_dialog
            || self.variable_dialog.open
            || self.fragment_library.open
            || self.show_terminal_search
            || self.delete_session_confirm.is_some()
            || self.close_tab_confirm_idx.is_some()
            || self.cmd_audit_confirm.is_some()
            || self.batch_exec_dialog.open
            || self.quick_selector.open
            || self.large_upload_pending_path.is_some()
            || self.ssh_import_dialog.open
            || self.command_history_overlay.open
            || self.session_log_dialog.open
            || self.audit_log_dialog.open
            || self.team_members_dialog.open
            || self.help_docs_dialog.open
            || self.show_ai_settings_dialog
    }

    /// 是否将键盘输入交给 PTY（弹窗打开或终端未聚焦时不抢键）
    fn should_capture_pty_keyboard(&self) -> bool {
        if self.global_shortcuts_blocked() {
            return false;
        }
        self.active_tab
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.active_terminal())
            .map(|term| term.is_terminal_focused())
            .unwrap_or(false)
    }

    /// 编辑菜单 ⌘C/⌘V/全选 是否应发给远端 PTY（否则发给当前焦点控件）
    fn route_edit_shortcuts_to_terminal(&self) -> bool {
        !self.global_shortcuts_blocked()
            && self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .and_then(|t| t.active_terminal())
                .map(|term| term.is_terminal_focused())
                .unwrap_or(false)
    }

    /// 将剪贴板内容粘贴到当前获得焦点的 egui 控件（如弹窗内 TextEdit）
    pub(crate) fn menu_paste_to_focused_widget(&self, ctx: &egui::Context) {
        if let Ok(mut clip) = arboard::Clipboard::new() {
            if let Ok(text) = clip.get_text() {
                if !text.is_empty() {
                    ctx.input_mut(|i| i.events.push(egui::Event::Paste(text)));
                    ctx.request_repaint();
                }
            }
        }
    }

    pub(crate) fn menu_paste_for_context(&mut self, ctx: &egui::Context) {
        if self.route_edit_shortcuts_to_terminal() {
            self.menu_paste_to_terminal(ctx);
        } else {
            self.menu_paste_to_focused_widget(ctx);
        }
    }

    pub(crate) fn menu_copy_for_context(&mut self, ctx: &egui::Context) {
        if self.route_edit_shortcuts_to_terminal() {
            self.menu_copy_terminal(ctx);
        } else {
            ctx.input_mut(|i| i.events.push(egui::Event::Copy));
            ctx.request_repaint();
        }
    }

    pub(crate) fn menu_select_all_for_context(&mut self, ctx: &egui::Context) {
        if self.route_edit_shortcuts_to_terminal() {
            self.menu_select_all_terminal(ctx);
        }
        // 表单内全选无标准 Event，依赖 ⌘A；菜单项在表单场景下不重复发终端全选
    }

    fn focus_sidebar_connection_search(&mut self, ctx: &egui::Context) {
        if self.sidebar_collapsed {
            self.sidebar_collapsed = false;
            self.sidebar_user_dismissed_responsive = false;
        }
        ctx.memory_mut(|m| m.request_focus(Self::id_sidebar_connection_search()));
        let j = crate::platform::accel("J");
        self.status_message = match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::En => format!("Focused connection search ({j})"),
            crate::i18n::UiLanguage::Zh => format!("已聚焦连接搜索框（{}）", j),
        };
    }

    fn focus_fragment_panel_search(&mut self, ctx: &egui::Context) {
        if !Self::right_dock_open_allowed(Self::layout_window_width(ctx)) {
            let w = Self::layout_window_width(ctx);
            self.status_message = Self::narrow_window_fragment_panel_hint(ctx, w);
            return;
        }
        self.show_fragment_panel = true;
        self.show_sftp_panel = false;
        ctx.memory_mut(|m| m.request_focus(Self::id_fragment_panel_search()));
        let k = crate::platform::accel("K");
        self.status_message = match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::En => format!("Focused snippet search ({k})"),
            crate::i18n::UiLanguage::Zh => format!("已聚焦片段搜索框（{}）", k),
        };
    }

    fn switch_tab_to_index(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = Some(idx);
            self.selected_session_id = Some(self.tabs[idx].primary_session_id());
        }
    }

    fn switch_to_next_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let cur = match self.active_tab {
            Some(i) if i < self.tabs.len() => i,
            _ => 0,
        };
        let next = (cur + 1) % self.tabs.len();
        self.switch_tab_to_index(next);
    }

    fn switch_to_prev_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let cur = match self.active_tab {
            Some(i) if i < self.tabs.len() => i,
            _ => 0,
        };
        let prev = if cur == 0 {
            self.tabs.len() - 1
        } else {
            cur - 1
        };
        self.switch_tab_to_index(prev);
    }

    /// 移除标签前先断开 SSH（FUNCTIONAL_SPEC §1.3.4）
    fn remove_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs[idx].stop_all_logs();
        self.tabs[idx].disconnect_all_panes();
        self.tabs.remove(idx);
        if let Some(active) = self.active_tab {
            if active == idx {
                self.active_tab = self.tabs.len().checked_sub(1);
            } else if active > idx {
                self.active_tab = Some(active - 1);
            }
        }
        self.selected_session_id = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.primary_session_id());
    }

    fn request_close_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let need_confirm = self.tabs[idx].any_connected_or_connecting();
        if need_confirm {
            self.close_tab_confirm_idx = Some(idx);
        } else {
            self.remove_tab_at(idx);
        }
    }

    fn request_close_active_tab(&mut self) {
        let Some(idx) = self.active_tab else {
            return;
        };
        self.request_close_tab_at(idx);
    }

    /// FUNCTIONAL_SPEC §1.3.5：断开 SSH，标签与屏幕缓冲保留（不可再输入）
    fn disconnect_ssh_keep_buffer_at(&mut self, ctx: &egui::Context, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) else {
            return;
        };
        if let Some(w) = pane.log_writer.as_mut() {
            w.stop_log();
        }
        let sid = pane.session_id.clone();
        let host = self
            .session_manager
            .get_session(&sid)
            .map(|s| s.host.clone())
            .unwrap_or_default();
        pane.terminal.disconnect_ssh_keep_buffer();
        self.sync_monitor_panel_to_active_tab();
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "session.disconnect", AuditOutcome::Success)
                .with_session(&sid)
                .with_host(&host),
        );
        self.status_message = crate::i18n::tr(
            ctx,
            "SSH disconnected on this tab (output kept; reconnect or close)",
            "已断开 SSH（本标签输出已保留，可重连或关闭标签）",
        )
        .to_string();
    }

    fn disconnect_ssh_keep_buffer_active(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签")
                .to_string();
            return;
        };
        self.disconnect_ssh_keep_buffer_at(ctx, idx);
    }

    fn reconnect_tab_at(&mut self, ctx: &egui::Context, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let (sid, offline) = {
            let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) else {
                return;
            };
            pane.ssh_auto_reconnect_next = None;
            pane.ssh_auto_reconnect_attempts = 0;
            let sid = pane.session_id.clone();
            let offline = pane.terminal.offline_input_snapshot();
            pane.terminal.disconnect();
            (sid, offline)
        };
        let Some(session) = self.session_manager.get_session(&sid).cloned() else {
            self.status_message = crate::i18n::tr(
                ctx,
                "No session profile found; cannot reconnect",
                "未找到会话配置，无法重连",
            )
            .to_string();
            return;
        };
        let mut temp_key = None;
        let mut terminal = TerminalView::new();
        self.terminal_connect_session(ctx, &mut terminal, &session, &mut temp_key);
        let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) else {
            return;
        };
        pane.terminal = terminal;
        pane.ssh_temp_key = temp_key;
        pane.terminal
            .restore_offline_input_snapshot(offline.0, offline.1);
        pane.title = session.name.clone();
        self.session_manager.mark_session_connected(&sid);
        self.sync_monitor_panel_to_active_tab();
        self.status_message = format!(
            "{} {}",
            crate::i18n::tr(ctx, "Reconnecting:", "正在重连："),
            session.name
        );
    }

    fn reconnect_active_tab(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签")
                .to_string();
            return;
        };
        self.reconnect_tab_at(ctx, idx);
    }

    /// 活动标签：SCP 直传或弹出 ≥10MB 选择（与拖放共用，FUNCTIONAL_SPEC §4.3）
    fn enqueue_upload_for_active_tab(&mut self, ctx: &egui::Context, path: std::path::PathBuf) {
        use crate::core::{decide_upload_dispatch, format_bytes_short, UploadDispatch};

        match decide_upload_dispatch(path.as_path(), self.active_tab.is_some()) {
            UploadDispatch::NoActiveTab => {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "Open a terminal tab first to upload",
                    "请先打开终端标签后再上传",
                )
                .to_string();
            }
            UploadDispatch::PromptLargeFile { size_bytes } => {
                let disp = path.display().to_string();
                self.large_upload_pending_path = Some(path);
                self.status_message = match crate::i18n::language(ctx) {
                    crate::i18n::UiLanguage::En => format!(
                        "Large file (≥10 MB); choose upload method: {} ({})",
                        disp,
                        format_bytes_short(size_bytes)
                    ),
                    crate::i18n::UiLanguage::Zh => format!(
                        "文件较大（≥10 MB），请选择上传方式：{}（{}）",
                        disp,
                        format_bytes_short(size_bytes)
                    ),
                };
            }
            UploadDispatch::ScpDirect { size_bytes } => {
                if let Some(terminal) = self.current_terminal_mut() {
                    match terminal.start_upload(path.as_path()) {
                        Ok(_) => {
                            self.status_message = format!(
                                "{} {}（{}）",
                                crate::i18n::tr(ctx, "Starting SCP upload:", "开始 SCP 上传："),
                                path.display(),
                                format_bytes_short(size_bytes)
                            );
                        }
                        Err(e) => {
                            self.status_message = status_message_wrap_error(format!(
                                "{} {}",
                                crate::i18n::tr(ctx, "Upload failed:", "上传失败："),
                                e
                            ));
                        }
                    }
                }
            }
        }
    }

    fn modal_header(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, title: &str, should_close: &mut bool) {
        if crate::ui::chrome::modal_header(
            ui,
            theme,
            title,
            crate::ui::chrome::modal_title_font_size(theme),
        ) {
            *should_close = true;
        }
    }

    fn modal_header_title_only(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, title: &str) {
        crate::ui::chrome::modal_header_title_only(
            ui,
            theme,
            title,
            crate::ui::chrome::modal_title_font_size(theme),
        );
    }

    /// 居中模态窗打开时不绘制右 dock Foreground，避免与弹窗标题栏 × 叠在同一位置。
    /// 偏好设置等视口居中弹窗不抑制：弹窗在终端区，不与右侧 dock 关闭钮重叠。
    fn suppress_right_dock_foreground(&self) -> bool {
        self.show_new_session_dialog
            || self.show_edit_session_dialog
            || self.show_fragments_dialog
            || self.show_fragment_vars_dialog
            || self.show_ai_settings_dialog
            || self.variable_dialog.open
            || self.ssh_import_dialog.open
            || self.delete_session_confirm.is_some()
            || self.close_tab_confirm_idx.is_some()
            || self.cmd_audit_confirm.is_some()
            || self.batch_exec_dialog.open
            || self.quick_selector.open
            || self.team_fragment_editor.open
            || self.team_fragment_conflict.is_some()
    }

    /// 合并 `<占位符>` 替换与 `{{ … }}` 得到「填写片段变量」弹窗中的初值。
    fn sync_pending_fragment_command_edit(&mut self) {
        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        let m: HashMap<String, String> = self.pending_fragment_vars.iter().cloned().collect();
        self.pending_fragment_command_edit =
            expand_fragment_command_stages(&self.pending_fragment_command, session, &m)
                .unwrap_or_else(|_| self.pending_fragment_command.clone());
    }

    fn build_fragment_command_preview(
        &self,
        fragment: &FragmentStats,
        values: &HashMap<String, String>,
    ) -> String {
        let session = self.session_for_selected();
        crate::core::build_fragment_command_preview(fragment, session, values)
    }

    fn finalize_fragment_command_text(
        &self,
        text: &str,
        values: &HashMap<String, String>,
    ) -> Result<String, String> {
        let session = self.session_for_selected();
        crate::core::finalize_fragment_command_text(text, session, values)
    }

    fn session_for_selected(&self) -> Option<&crate::core::SessionConfig> {
        self.selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid))
    }

    fn finalize_pending_fragment_send(&self) -> Result<String, String> {
        let m: HashMap<String, String> = self.pending_fragment_vars.iter().cloned().collect();
        self.finalize_fragment_command_text(&self.pending_fragment_command_edit, &m)
    }

    fn current_terminal_mut(&mut self) -> Option<&mut TerminalView> {
        let idx = self.active_tab?;
        self.tabs.get_mut(idx)?.active_terminal_mut()
    }

    fn current_terminal(&self) -> Option<&TerminalView> {
        let idx = self.active_tab?;
        self.tabs.get(idx)?.active_terminal()
    }

    /// 监控侧栏跟随当前标签：重新绑定 SSH 会话上的 exec；未连接则清空展示。
    fn sync_monitor_panel_to_active_tab(&mut self) {
        let Some(tab) = self.active_tab.and_then(|i| self.tabs.get(i)) else {
            self.monitor_panel.clear();
            return;
        };
        if let Some(term) = tab.active_terminal() {
            if term.is_connected() {
                if let (Some(h), Some(mgr)) = (
                    term.ssh_session_handle(),
                    term.ssh_manager_clone(),
                ) {
                    self.monitor_panel.init(h, mgr);
                    return;
                }
            }
        }
        self.monitor_panel.clear();
    }

    /// macOS ⌘ 与 Windows/Linux Ctrl（FUNCTIONAL_SPEC §7）
    #[inline]
    fn input_primary_mod(i: &egui::InputState) -> bool {
        i.modifiers.command || i.modifiers.ctrl
    }

    /// 为给定会话配置追加一个新终端标签并发起连接（不检查是否已有同会话标签）
    fn apply_team_vault_from_sync(&mut self) {
        let Some(tid) = self.team_service.state.current_team_id.clone() else {
            return;
        };
        let Some(entry) = self
            .team_service
            .state
            .sync_entry_for(&tid)
            .cloned()
        else {
            return;
        };
        match apply_vault_for_team(&mut self.app_settings.vault, &entry) {
            Ok(()) => {
                let _ = self.app_settings.save();
                self.audit_logger.record(
                    AuditEvent::new(AuditCategory::Config, "config.vault_apply", AuditOutcome::Success)
                        .with_detail(serde_json::json!({ "team_id": tid })),
                );
            }
            Err(e) => {
                self.team_service.state.last_error = e.clone();
                let _ = self.team_service.state.save();
            }
        }
    }

    fn connect_team_server(&mut self, ctx: &egui::Context, server_key: &str) {
        let Some(server) = self
            .team_service
            .current_team_servers()
            .into_iter()
            .find(|s| s.list_key() == server_key)
        else {
            return;
        };
        let team_id = self
            .team_service
            .state
            .current_team_id
            .clone()
            .unwrap_or_default();
        let mut session = SessionConfig::default();
        session.name = server.name.clone();
        session.host = server.host.clone();
        session.port = server.port;
        session.username = server.username.clone();
        session.group = crate::i18n::tr(ctx, "Team", "团队").to_string();
        if !server.vault_credential_path.is_empty() {
            if let Some((mount, path, field)) = parse_vault_credential_path(
                &server.vault_credential_path,
                &self.app_settings.vault.default_mount,
            ) {
                session.secret_backend = SecretBackend::VaultKv {
                    mount,
                    path,
                    field,
                    version: None,
                };
                self.audit_logger.record(
                    AuditEvent::new(AuditCategory::Vault, "config.vault_read", AuditOutcome::Success)
                        .with_resource(&server.vault_credential_path)
                        .with_detail(serde_json::json!({
                            "team_id": team_id,
                            "server_id": server.id,
                        })),
                );
            }
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "shell.connect", AuditOutcome::Success)
                .with_host(&server.host)
                .with_detail(serde_json::json!({
                    "team_id": team_id,
                    "server_id": server.id,
                    "port": server.port,
                })),
        );
        self.push_tab_connecting(ctx, &session);
    }

    fn push_tab_connecting(&mut self, ctx: &egui::Context, session: &SessionConfig) {
        let mut terminal = TerminalView::new();
        let mut temp_key = None;
        self.terminal_connect_session(ctx, &mut terminal, session, &mut temp_key);
        self.tabs.push(TerminalTab::single(TerminalPane::new(
            session.id.clone(),
            session.name.clone(),
            terminal,
        )));
        if let Some(tab) = self.tabs.last_mut() {
            tab.panes[0].ssh_temp_key = temp_key;
        }
        let idx = self.tabs.len() - 1;
        self.ensure_tab_log_writer(idx);
        self.active_tab = Some(idx);
        self.session_manager.mark_session_connected(&session.id);
    }

    fn menu_open_batch_exec(&mut self, ctx: &egui::Context) {
        let mut preselect = Vec::new();
        for tab in &self.tabs {
            for pane in &tab.panes {
                if pane.terminal.is_connected() {
                    preselect.push(pane.session_id.clone());
                }
            }
        }
        self.batch_exec_dialog.open(&preselect);
        ctx.request_repaint();
    }

    fn build_batch_targets(&self, ctx: &egui::Context, include_team: bool) -> Vec<BatchTarget> {
        let mut out = Vec::new();
        for s in self.session_manager.list_sessions() {
            out.push(BatchTarget {
                id: s.id.clone(),
                label: format!("{} · {}", s.name, s.host),
                group: if s.group.is_empty() {
                    crate::i18n::tr(ctx, "Default", "默认").to_string()
                } else {
                    s.group.clone()
                },
            });
        }
        if include_team {
            for srv in self.team_service.current_team_servers() {
                out.push(BatchTarget {
                    id: format!("{TEAM_TARGET_PREFIX}{}", srv.list_key()),
                    label: format!("{} · {}", srv.name, srv.host),
                    group: crate::i18n::tr(ctx, "Team", "团队").to_string(),
                });
            }
        }
        out
    }

    fn team_server_to_session(&self, ctx: &egui::Context, server: &crate::core::team::TeamServer) -> SessionConfig {
        let mut session = SessionConfig::default();
        session.name = server.name.clone();
        session.host = server.host.clone();
        session.port = server.port;
        session.username = server.username.clone();
        session.group = crate::i18n::tr(ctx, "Team", "团队").to_string();
        if !server.vault_credential_path.is_empty() {
            if let Some((mount, path, field)) = parse_vault_credential_path(
                &server.vault_credential_path,
                &self.app_settings.vault.default_mount,
            ) {
                session.secret_backend = SecretBackend::VaultKv {
                    mount,
                    path,
                    field,
                    version: None,
                };
            }
        }
        session
    }

    fn session_to_ssh_config(&self, session: &SessionConfig) -> Result<SshConfig, String> {
        let resolver = SecretResolver::new(self.app_settings.vault.clone());
        let resolved = resolver
            .resolve_session(session)
            .map_err(|e| e.to_string())?;
        let jump_hops = self.resolve_proxy_jump_hops(session)?;
        let (ka_on, ka_int, ka_max) = Self::session_keepalive_params(session);
        let interval = if ka_on {
            ka_int.max(1)
        } else {
            0
        };
        Ok(SshConfig {
            host: session.host.clone(),
            port: session.port,
            username: session.username.clone(),
            password: resolved.password,
            private_key_path: resolved.private_key_path,
            use_ssh_agent: session.use_ssh_agent,
            keepalive_interval_secs: interval,
            keepalive_count_max: ka_max,
            proxy_jump: session.proxy_jump.clone(),
            proxy_command: session.proxy_command.clone(),
            jump_hops,
            local_forwards: parse_local_forwards_text(&session.local_forwards_text),
            remote_forwards: parse_remote_forwards_text(&session.remote_forwards_text),
            dynamic_forwards: parse_dynamic_forwards_text(&session.dynamic_forwards_text),
        })
    }

    fn batch_exec_allowed(&mut self, ctx: &egui::Context, command: &str) -> bool {
        let audit = self.cmd_audit_engine.check(command);
        match audit.action {
            CmdAuditAction::Block => {
                self.record_cmd_audit_event("command.blocked", command, &audit, AuditOutcome::Denied);
                self.status_message = format!(
                    "{}: {}",
                    crate::i18n::tr(ctx, "Command blocked", "命令已拦截"),
                    command_preview(command, 80)
                );
                false
            }
            CmdAuditAction::Confirm => {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "Batch run cannot proceed: command requires confirmation in an interactive terminal.",
                    "无法批量执行：该命令需在交互终端中二次确认。",
                )
                .to_string();
                false
            }
            CmdAuditAction::Alert => {
                self.record_cmd_audit_event("command.alert", command, &audit, AuditOutcome::Success);
                true
            }
            CmdAuditAction::Allow => true,
        }
    }

    fn start_batch_exec(&mut self, ctx: &egui::Context) {
        let command = self.batch_exec_dialog.command.trim().to_string();
        if command.is_empty() || self.batch_exec_dialog.selected.is_empty() {
            return;
        }
        if !self.batch_exec_allowed(ctx, &command) {
            return;
        }
        let parallel = self.batch_exec_dialog.max_parallel as usize;
        let selected: Vec<String> = self.batch_exec_dialog.selected.iter().cloned().collect();
        let mut jobs = Vec::new();
        for id in selected {
            if let Some(key) = id.strip_prefix(TEAM_TARGET_PREFIX) {
                let Some(server) = self
                    .team_service
                    .current_team_servers()
                    .into_iter()
                    .find(|s| s.list_key() == key)
                else {
                    continue;
                };
                let session = self.team_server_to_session(ctx, &server);
                let label = format!("{} · {}", server.name, server.host);
                match self.session_to_ssh_config(&session) {
                    Ok(config) => jobs.push(BatchExecJob {
                        target_id: id,
                        label,
                        config,
                    }),
                    Err(e) => {
                        self.status_message = format!(
                            "{} {}: {}",
                            crate::i18n::tr(ctx, "Credential error for", "凭据错误："),
                            label,
                            e
                        );
                        return;
                    }
                }
            } else if let Some(session) = self.session_manager.get_session(&id) {
                let label = format!("{} · {}", session.name, session.host);
                match self.session_to_ssh_config(session) {
                    Ok(config) => jobs.push(BatchExecJob {
                        target_id: id,
                        label,
                        config,
                    }),
                    Err(e) => {
                        self.status_message = format!(
                            "{} {}: {}",
                            crate::i18n::tr(ctx, "Credential error for", "凭据错误："),
                            label,
                            e
                        );
                        return;
                    }
                }
            }
        }
        if jobs.is_empty() {
            return;
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "batch.exec", AuditOutcome::Success)
                .with_detail(serde_json::json!({
                    "hosts": jobs.len(),
                    "command_preview": command_preview(&command, 120),
                })),
        );
        self.batch_exec_dialog.running = true;
        self.batch_exec_dialog.results.clear();
        let (tx, rx) = std::sync::mpsc::channel();
        self.batch_exec_rx = Some(rx);
        std::thread::spawn(move || {
            let rows = run_batch_parallel(jobs, command, parallel);
            let _ = tx.send(rows);
        });
        ctx.request_repaint();
    }

    fn record_fragment_execution(&mut self, fragment_id: &str, success: bool, dur_ms: u64) {
        let ts = chrono::Utc::now().timestamp();
        let (scope, team_id) = if self.team_service.find_team_fragment(fragment_id).is_some() {
            (
                "team".to_string(),
                self.team_service.state.current_team_id.clone(),
            )
        } else {
            ("personal".to_string(), None)
        };
        let user = self.team_service.state.user.as_ref();
        let display_name = user.map(|u| {
            if !u.display_name.is_empty() {
                u.display_name.clone()
            } else if !u.username.is_empty() {
                u.username.clone()
            } else {
                u.email.clone()
            }
        });
        self.fragment_usage_log.append(crate::core::FragmentUsageEvent {
            ts,
            fragment_id: fragment_id.to_string(),
            scope,
            team_id: team_id.clone(),
            user_id: user.map(|u| u.id.clone()),
            display_name,
            success,
            duration_ms: dur_ms,
        });
        let _ = self.fragment_usage_log.save_if_dirty();

        if self.fragment_manager.get_by_id(fragment_id).is_some() {
            self.fragment_manager
                .record_execution(fragment_id, success, dur_ms);
            let _ = self
                .fragment_manager
                .save(&FragmentManager::default_config_path());
        } else if self.team_service.find_team_fragment(fragment_id).is_some() {
            self.team_service
                .record_fragment_usage(fragment_id, success, dur_ms);
            if let Some(tid) = team_id {
                self.team_service.spawn_report_fragment_usage(
                    &tid,
                    fragment_id,
                    success,
                    dur_ms,
                );
            }
        }
    }

    fn market_item_for_stats(
        &self,
        frag: &FragmentStats,
    ) -> Option<crate::core::MarketFragment> {
        let id = frag
            .tags
            .iter()
            .find_map(|t| t.strip_prefix("mkt:"))?;
        self.market_catalog
            .fragments()
            .iter()
            .find(|f| f.id == id)
            .cloned()
    }

    fn poll_market_catalog_refresh(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.market_catalog_refresh_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(state) => {
                self.market_catalog = state;
                self.market_catalog_refresh_rx = None;
                self.market_catalog_refresh_pending = false;
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.market_catalog_refresh_rx = None;
                self.market_catalog_refresh_pending = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }

    fn start_market_catalog_refresh(&mut self) {
        if self.market_catalog_refresh_rx.is_some() {
            return;
        }
        let api_base = self.app_settings.team.normalized_api_base();
        let token = self.team_service.current_access_token();
        let query = crate::core::MarketCatalogQuery {
            category: self.fragment_filter_category.clone(),
            search: self.fragment_search_query.clone(),
            limit: 200,
            cursor: String::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.market_catalog_refresh_rx = Some(rx);
        self.market_catalog_refresh_pending = true;
        let mut state = self.market_catalog.clone();
        std::thread::spawn(move || {
            state.refresh_blocking(&api_base, token.as_deref(), &query);
            let _ = tx.send(state);
        });
    }

    fn start_market_catalog_load_more(&mut self) {
        if self.market_catalog_refresh_rx.is_some() || !self.market_catalog.has_more() {
            return;
        }
        let api_base = self.app_settings.team.normalized_api_base();
        let token = self.team_service.current_access_token();
        let query = crate::core::MarketCatalogQuery {
            category: self.fragment_filter_category.clone(),
            search: self.fragment_search_query.clone(),
            limit: 200,
            cursor: self.market_catalog.cache.cursor.clone(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.market_catalog_refresh_rx = Some(rx);
        let mut state = self.market_catalog.clone();
        std::thread::spawn(move || {
            state.load_more_blocking(&api_base, token.as_deref(), &query);
            let _ = tx.send(state);
        });
    }

    fn install_market_fragment(&mut self, ctx: &egui::Context, item: &crate::core::MarketFragment) {
        let api_base = self.app_settings.team.normalized_api_base();
        let token = self.team_service.current_access_token();
        match crate::core::install_into_personal_library(&mut self.fragment_manager, item) {
            Ok(()) => {
                self.market_catalog
                    .report_install_blocking(&api_base, token.as_deref(), &item.id);
                let _ = self
                    .fragment_manager
                    .save(&FragmentManager::default_config_path());
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Added to personal library:", "已添加到个人库："),
                    item.title
                );
            }
            Err(e) if e == "already_installed" => {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "This market snippet is already in your library",
                    "该市场片段已在个人库中",
                )
                .to_string();
            }
            Err(e) => {
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Install failed:", "安装失败："),
                    e
                );
            }
        }
    }

    fn refresh_fragment_analytics_dashboard(&mut self) {
        let personal: Vec<_> = self.fragment_manager.get_all().to_vec();
        self.fragment_analytics_snapshot = self.team_service.build_fragment_analytics_dashboard(
            &personal,
            self.fragment_analytics_range,
            &self.fragment_usage_log,
        );
        let cutoff = self.fragment_analytics_range.cutoff_unix();
        self.fragment_recommendations = crate::core::recommend_from_history(
            &self.command_history,
            &personal,
            cutoff,
            8,
        );
    }

    fn export_efficiency_report(&mut self, ctx: &egui::Context) {
        let md = crate::core::build_efficiency_report_markdown(
            &self.fragment_analytics_snapshot,
            self.fragment_analytics_range,
            &self.fragment_recommendations,
        );
        if let Ok(mut clip) = arboard::Clipboard::new() {
            if clip.set_text(&md).is_ok() {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "Efficiency report (Markdown) copied to clipboard",
                    "效率报告（Markdown）已复制到剪贴板",
                )
                .to_string();
                return;
            }
        }
        self.status_message = crate::i18n::tr(
            ctx,
            "Failed to copy efficiency report",
            "复制效率报告失败",
        )
        .to_string();
    }

    fn export_efficiency_report_pdf(&mut self, ctx: &egui::Context) {
        let pdf = match crate::core::build_efficiency_report_pdf(
            &self.fragment_analytics_snapshot,
            self.fragment_analytics_range,
            &self.fragment_recommendations,
        ) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "PDF export failed:", "PDF 导出失败："),
                    e
                );
                return;
            }
        };
        let default_name = format!(
            "mistterm-efficiency-{}.pdf",
            chrono::Local::now().format("%Y%m%d")
        );
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("PDF", &["pdf"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, pdf) {
            Ok(()) => {
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Efficiency report saved:", "效率报告已保存："),
                    path.display()
                );
            }
            Err(e) => {
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Failed to write PDF:", "写入 PDF 失败："),
                    e
                );
            }
        }
    }

    fn add_fragment_from_recommendation(&mut self, ctx: &egui::Context, index: usize) {
        let Some(rec) = self.fragment_recommendations.get(index) else {
            return;
        };
        let title: String = rec.command.chars().take(40).collect();
        let cmd = rec.command.clone();
        self.fragment_manager.add_fragment(
            title.clone(),
            cmd,
            "recommended".to_string(),
        );
        let _ = self
            .fragment_manager
            .save(&FragmentManager::default_config_path());
        self.refresh_fragment_analytics_dashboard();
        self.status_message = format!(
            "{} {}",
            crate::i18n::tr(ctx, "Snippet added:", "已添加片段："),
            title
        );
    }

    fn open_fragment_analytics_dialog(&mut self) {
        if self.team_service.is_logged_in() && self.team_service.team_members.is_empty() {
            self.team_service.spawn_list_team_members();
        }
        self.refresh_fragment_analytics_dashboard();
        self.show_fragment_analytics_dialog = true;
    }

    fn export_fragment_analytics_json(&mut self, ctx: &egui::Context) {
        let json = match crate::core::export_dashboard_json(
            &self.fragment_analytics_snapshot,
            self.fragment_analytics_range,
        ) {
            Ok(j) => j,
            Err(e) => {
                self.status_message = format!(
                    "{}: {e}",
                    crate::i18n::tr(ctx, "Export failed", "导出失败")
                );
                return;
            }
        };
        let default_name = format!(
            "mistterm-analytics-{}.json",
            chrono::Local::now().format("%Y%m%d")
        );
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(&default_name)
            .add_filter("JSON", &["json"])
            .save_file()
        {
            match std::fs::write(&path, &json) {
                Ok(()) => {
                    self.status_message = format!(
                        "{} {}",
                        crate::i18n::tr(ctx, "Analytics JSON saved:", "分析 JSON 已保存："),
                        path.display()
                    );
                    return;
                }
                Err(e) => {
                    self.status_message = format!(
                        "{} {e}",
                        crate::i18n::tr(ctx, "Save failed:", "保存失败：")
                    );
                    return;
                }
            }
        }
        if let Ok(mut clip) = arboard::Clipboard::new() {
            if clip.set_text(&json).is_ok() {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "Analytics JSON copied to clipboard",
                    "分析 JSON 已复制到剪贴板",
                )
                .to_string();
                return;
            }
        }
        self.status_message = crate::i18n::tr(
            ctx,
            "Failed to export analytics JSON",
            "导出分析 JSON 失败",
        )
        .to_string();
    }

    fn schedule_market_catalog_debounce(&mut self) {
        if self.fragment_list_scope != FragmentListScope::Market {
            return;
        }
        self.market_catalog_debounce_deadline = Some(
            std::time::Instant::now() + std::time::Duration::from_millis(450),
        );
    }

    fn poll_market_catalog_debounce(&mut self) {
        let Some(deadline) = self.market_catalog_debounce_deadline else {
            return;
        };
        if std::time::Instant::now() < deadline {
            return;
        }
        self.market_catalog_debounce_deadline = None;
        if self.fragment_list_scope == FragmentListScope::Market {
            self.start_market_catalog_refresh();
        }
    }

    fn sync_market_catalog_query_fingerprint(&mut self) {
        if self.fragment_list_scope != FragmentListScope::Market {
            return;
        }
        let fp = (
            self.fragment_filter_category.clone(),
            self.fragment_search_query.clone(),
        );
        if fp != self.market_catalog_query_fingerprint {
            self.market_catalog_query_fingerprint = fp;
            self.schedule_market_catalog_debounce();
        }
    }

    fn split_tab_at(&mut self, ctx: &egui::Context, idx: usize, layout: TabLayout) {
        if idx >= self.tabs.len() || !self.tabs[idx].can_split() {
            return;
        }
        let active = self.tabs[idx].active_pane.min(self.tabs[idx].panes.len().saturating_sub(1));
        let session_id = self.tabs[idx].panes[active].session_id.clone();
        let Some(session) = self.session_manager.get_session(&session_id).cloned() else {
            return;
        };
        let mut terminal = TerminalView::new();
        let mut temp_key = None;
        self.terminal_connect_session(ctx, &mut terminal, &session, &mut temp_key);
        let n = self.tabs[idx].panes.len() + 1;
        let title2 = format!("{} ({n})", session.name);
        let mut pane2 = TerminalPane::new(session.id.clone(), title2, terminal);
        pane2.ssh_temp_key = temp_key;
        let tab = &mut self.tabs[idx];
        tab.add_pane_with_layout(pane2, layout);
        self.ensure_tab_log_writer(idx);
        self.status_message = crate::i18n::tr(
            ctx,
            "Split terminal pane",
            "已分屏",
        )
        .to_string();
    }

    fn close_pane_tab_at(&mut self, ctx: &egui::Context, tab_idx: usize, pane_idx: usize) {
        if tab_idx >= self.tabs.len() {
            return;
        }
        if self.tabs[tab_idx].close_pane(pane_idx) {
            self.status_message =
                crate::i18n::tr(ctx, "Closed terminal pane", "已关闭该窗格").to_string();
        }
    }

    fn maybe_collapse_narrow_split(&mut self, tab_idx: usize, column_w: f32) {
        if column_w >= crate::ui::tab_pane::NARROW_SPLIT_COLLAPSE_W {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            if tab.is_split() {
                tab.unsplit_keep_active();
            }
        }
    }

    fn unsplit_tab_at(&mut self, ctx: &egui::Context, idx: usize) {
        if idx >= self.tabs.len() || !self.tabs[idx].is_split() {
            return;
        }
        self.tabs[idx].unsplit_keep_active();
        self.status_message = crate::i18n::tr(ctx, "Merged split panes", "已合并分屏").to_string();
    }

    /// ⌘T / Ctrl+T：为左侧当前选中会话新开标签；未选中时提示（与 ⌘N 新建配置区分）
    fn open_new_tab_from_selection(&mut self, ctx: &egui::Context) {
        let Some(ref sid) = self.selected_session_id else {
            let t = crate::platform::accel("T");
            let n = crate::platform::accel("N");
            self.status_message = match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::En => format!(
                    "Select a connection on the left, then {t} for a new tab ({n} adds a new profile)",
                ),
                crate::i18n::UiLanguage::Zh => format!(
                    "请先在左侧选择一个连接，再按 {t} 新开标签；{n} 为新建会话配置",
                ),
            };
            return;
        };
        let Some(session) = self.session_manager.get_session(sid).cloned() else {
            self.status_message =
                crate::i18n::tr(ctx, "Selected session not found", "未找到所选会话").to_string();
            return;
        };
        self.selected_session_id = Some(session.id.clone());
        self.push_tab_connecting(ctx, &session);
    }

    /// 终端列内查找条（非浮动 Window，避免标题栏占满宽）。返回 `true` 表示关闭。
    fn show_terminal_search_bar(&mut self, ui: &mut egui::Ui, theme: &crate::ui::theme::Theme) -> bool {
        use eframe::egui::{Key, RichText};
        let bar_h = theme.size_terminal_search_bar_h();
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, bar_h), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, theme.chrome_bar_fill());
        ui.painter().hline(
            rect.x_range(),
            rect.top(),
            egui::Stroke::new(1.0, theme.border_divider_color()),
        );

        let ctx = ui.ctx().clone();
        let detail = if self.current_terminal().is_none() {
            crate::i18n::tr(&ctx, "Open a terminal tab first", "请先打开终端标签").to_string()
        } else if self.terminal_search_query.is_empty() {
            crate::i18n::tr(
                &ctx,
                "Matches terminal buffer (incl. scrollback)",
                "匹配终端缓冲（含 scrollback）",
            )
            .to_string()
        } else if self.terminal_search_hits.is_empty() {
            crate::i18n::tr(&ctx, "No matches", "无匹配").to_string()
        } else {
            let hit = self.terminal_search_hits[self.terminal_search_cur];
            match crate::i18n::language(&ctx) {
                crate::i18n::UiLanguage::En => format!(
                    "{}/{} · line {} col {}",
                    self.terminal_search_cur + 1,
                    self.terminal_search_hits.len(),
                    hit.line.0,
                    hit.column + 1
                ),
                crate::i18n::UiLanguage::Zh => format!(
                    "第 {}/{} · 行{} 列{}",
                    self.terminal_search_cur + 1,
                    self.terminal_search_hits.len(),
                    hit.line.0,
                    hit.column + 1
                ),
            }
        };

        let mut close = false;
        let inner = rect.shrink2(egui::vec2(theme.spacing_region_pad_x(), 0.0));
        ui.allocate_ui_at_rect(inner, |ui| {
            ui.set_height(bar_h);
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                close = true;
            }
            if ui.input(|i| i.key_pressed(Key::F3) && i.modifiers.shift) {
                self.terminal_search_step(-1);
            } else if ui.input(|i| i.key_pressed(Key::F3)) {
                self.terminal_search_step(1);
            }
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_status_left_gap();
                ui.label(
                    RichText::new(crate::i18n::tr(&ctx, "Find", "查找"))
                        .size(theme.font_size_panel_title())
                        .color(theme.text_secondary()),
                );
                let search_id = egui::Id::new("mistterm_terminal_search_input");
                let input_w = (ui.available_width() * 0.22).clamp(96.0, 200.0);
                let resp = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    search_id,
                    &mut self.terminal_search_query,
                    crate::i18n::tr(&ctx, "Keyword…", "关键词…"),
                    input_w,
                    false,
                );
                if self.terminal_search_pending_focus {
                    resp.request_focus();
                    self.terminal_search_pending_focus = false;
                }
                if resp.changed() {
                    self.rebuild_terminal_search_matches();
                }
                if resp.has_focus()
                    && ui.ctx().input(|i| i.key_pressed(Key::Enter))
                    && !self.terminal_search_hits.is_empty()
                {
                    self.terminal_search_step(1);
                }
                if ui
                    .checkbox(&mut self.terminal_search_ignore_case, "Aa")
                    .on_hover_text(crate::i18n::tr(&ctx, "Ignore case", "忽略大小写"))
                    .changed()
                {
                    self.rebuild_terminal_search_matches();
                }
                if crate::ui::chrome::chrome_small_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronLeft,
                )
                .on_hover_text(crate::i18n::tr(&ctx, "Previous (Shift + F3)", "上一个 (Shift + F3)"))
                .clicked()
                {
                    self.terminal_search_step(-1);
                }
                if crate::ui::chrome::chrome_small_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronRight,
                )
                .on_hover_text(crate::i18n::tr(&ctx, "Next (F3 / Enter)", "下一个 (F3 / Enter)"))
                .clicked()
                {
                    self.terminal_search_step(1);
                }
                if crate::ui::chrome::close_icon_button(ui, theme).clicked() {
                    close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(detail)
                            .size(theme.font_size_small())
                            .color(theme.text_tertiary()),
                    );
                });
            });
        });
        close
    }

    /// 切换右侧 SFTP 文件浏览器（需宽屏布局）。
    pub(crate) fn toggle_sftp_panel(&mut self, ctx: &egui::Context) {
        if self.show_sftp_panel {
            self.show_sftp_panel = false;
        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_sftp_panel = true;
            self.sftp_last_tab = None;
            self.sftp_panel.request_list_on_open();
        }
    }

    /// 切换右侧命令片段侧栏。
    pub(crate) fn toggle_fragment_sidebar(&mut self, ctx: &egui::Context) {
        if self.show_fragment_panel {
            self.show_fragment_panel = false;
        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_fragment_panel = true;
        }
    }

    /// 切换右侧系统监控面板。
    pub(crate) fn toggle_monitor_panel(&mut self, ctx: &egui::Context) {
        if self.show_monitor_panel {
            self.show_monitor_panel = false;
            self.monitor_last_tab = None;
        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_monitor_panel = true;
            self.sync_monitor_panel_to_active_tab();
            self.monitor_last_tab = self.active_tab;
        }
    }

    /// 切换右侧端口转发面板。
    pub(crate) fn toggle_port_forward_panel(&mut self, ctx: &egui::Context) {
        if self.show_port_forward_panel {
            self.show_port_forward_panel = false;
            self.port_forward_last_tab = None;
        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_port_forward_panel = true;
            self.port_forward_last_tab = self.active_tab;
        }
    }

    fn active_tab_session_profile(&self) -> Option<SessionConfig> {
        let sid = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.primary_session_id())?;
        self.session_manager.get_session(&sid).cloned()
    }

    fn poll_port_forward_panel(&mut self) {
        if let Some(save) = self.port_forward_panel.take_pending_save() {
            self.apply_port_forward_save(save);
        }
        for audit in self.port_forward_panel.take_pending_audits() {
            self.record_port_forward_audit(audit);
        }
        if let Some(t) = self.current_terminal() {
            if t.is_connected() {
                if let (Some(ssh_id), Some(profile)) = (
                    t.ssh_session_id(),
                    self.active_tab_session_profile(),
                ) {
                    self.port_forward_panel
                        .register_profile_forwards(ssh_id, &profile);
                }
            } else if let Some(sid) = t.ssh_session_id() {
                self.port_forward_panel.clear_ssh_session(sid);
            }
        }
    }

    fn record_port_forward_audit(
        &mut self,
        req: crate::ui::port_forward_panel::PortForwardAuditRequest,
    ) {
        let action = if req.started {
            req.kind.audit_action_start()
        } else {
            req.kind.audit_action_stop()
        };
        let mut ev = AuditEvent::new(AuditCategory::Session, action, AuditOutcome::Success)
            .with_detail(req.kind.audit_detail());
        if let Some(host) = req.host {
            ev = ev.with_host(host);
        }
        if let Some(sid) = req.session_profile_id {
            ev = ev.with_session(sid);
        }
        self.audit_logger.record(ev);
    }

    fn apply_port_forward_save(
        &mut self,
        save: crate::ui::port_forward_panel::PortForwardSaveRequest,
    ) {
        let kind = save.kind;
        let id = save.session_profile_id;
        self.session_manager.patch_session(&id, |session| match kind {
            PortForwardKind::Local(f) => {
                append_local_forward_line(&mut session.local_forwards_text, &f);
            }
            PortForwardKind::Remote(f) => {
                append_remote_forward_line(&mut session.remote_forwards_text, &f);
            }
            PortForwardKind::Dynamic(f) => {
                append_dynamic_forward_line(&mut session.dynamic_forwards_text, &f);
            }
        });
    }

    pub(crate) fn toggle_ai_panel(&mut self, ctx: &egui::Context) {
        if self.show_ai_panel {
            self.show_ai_panel = false;
        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_ai_panel = true;
        }
    }

    pub(crate) fn send_terminal_selection_to_ai(&mut self, ctx: &egui::Context) {
        let text = self
            .current_terminal()
            .map(|t| t.selected_text())
            .unwrap_or_default();
        if text.trim().is_empty() {
            self.status_message = crate::i18n::tr(
                ctx,
                "Select text in the terminal first",
                "请先在终端选中内容",
            )
            .to_string();
            return;
        }
        self.ai_panel.attach_context(text);
        self.ai_panel.focus_draft_input(ctx);
        if self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_ai_panel = true;
            self.status_message = crate::i18n::tr(
                ctx,
                "Terminal selection attached to AI",
                "终端选区已附带至 AI",
            )
            .to_string();
        }
    }

    /// 终端「发送到 AI」与 AI 面板「用到终端」桥接。
    pub(crate) fn process_ai_bridge(&mut self, ctx: &egui::Context) {
        self.sync_ai_chat_session();
        let mut open_ai = false;
        let mut attach_text: Option<String> = None;
        let mut attach_source: Option<&str> = None;
        let mut tail_empty = false;
        let mut monitor_empty = false;
        let mut session_log_empty = false;
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(idx) {
                if let Some(pane) = tab.active_pane_mut() {
                    let session_name = self
                        .session_manager
                        .get_session(&pane.session_id)
                        .map(|s| s.name.clone());
                    self.ai_panel.attach_session_meta(
                        pane.terminal.ai_session_meta(session_name),
                    );
                    if pane.terminal.take_pending_send_to_ai() {
                        let text = pane
                            .terminal
                            .take_pending_send_to_ai_text()
                            .unwrap_or_else(|| pane.terminal.selected_text());
                        attach_text = Some(text);
                        open_ai = true;
                    }
                    if pane.terminal.take_pending_send_tail_to_ai() {
                        let text = pane.terminal.tail_plain_text(50);
                        if text.trim().is_empty() {
                            tail_empty = true;
                        } else {
                            attach_text = Some(text);
                            open_ai = true;
                        }
                    }
                }
            }
        }
        if self.monitor_panel.take_pending_send_to_ai() {
            if let Some(text) = self.monitor_panel.snapshot_for_ai() {
                attach_text = Some(text);
                attach_source = Some("monitor");
                open_ai = true;
            } else {
                monitor_empty = true;
                open_ai = true;
            }
        }
        if self.session_log_dialog.take_pending_send_to_ai() {
            if let Some(text) = self.session_log_dialog.content_for_ai() {
                attach_text = Some(text);
                attach_source = Some("session_log");
                open_ai = true;
            } else {
                session_log_empty = true;
                open_ai = true;
            }
        }
        if tail_empty {
            self.status_message = crate::i18n::tr(
                ctx,
                "Terminal buffer is empty",
                "终端缓冲区为空",
            )
            .to_string();
        }
        if monitor_empty {
            self.status_message = crate::i18n::tr(
                ctx,
                "No monitor data yet; wait for a refresh",
                "尚无监控数据，请等待刷新",
            )
            .to_string();
        }
        if session_log_empty {
            self.status_message = crate::i18n::tr(
                ctx,
                "Session log is empty",
                "会话日志为空",
            )
            .to_string();
        }
        if let Some(text) = attach_text {
            self.ai_panel
                .attach_context_labeled(attach_source, text);
            self.ai_panel.focus_draft_input(ctx);
            open_ai = true;
        }
        if open_ai && self.ensure_right_dock_allowed_or_warn(ctx) {
            self.show_ai_panel = true;
        }
        if let Some(cmd) = self.ai_panel.take_command_for_terminal() {
            if let Some(idx) = self.active_tab {
                if self.tabs.get_mut(idx).is_some() {
                    let audit = self.cmd_audit_engine.check(&cmd);
                    match self.send_audited_command_active(ctx, &cmd) {
                        CommandSendResult::Sent => {
                            self.record_cmd_audit_event(
                                "command.ai_suggested",
                                &cmd,
                                &audit,
                                crate::core::AuditOutcome::Success,
                            );
                            self.status_message = terminal_command_status_message(ctx, &cmd);
                        }
                        CommandSendResult::Blocked(_) | CommandSendResult::NeedsConfirm { .. } => {}
                        CommandSendResult::NotConnected => {
                            self.status_message = crate::i18n::tr(
                                ctx,
                                "No active terminal tab; cannot run command",
                                "无活动终端标签，无法执行命令",
                            )
                            .to_string();
                        }
                    }
                    ctx.request_repaint();
                }
            } else {
                self.status_message = crate::i18n::tr(
                    ctx,
                    "No active terminal tab; cannot run command",
                    "无活动终端标签，无法执行命令",
                )
                .to_string();
            }
        }
    }

    fn sync_ai_chat_session(&mut self) {
        let key = self
            .active_tab
            .and_then(|idx| self.tabs.get(idx))
            .and_then(|tab| tab.active_pane())
            .map(|pane| format!("session_{}", pane.session_id))
            .unwrap_or_else(|| "global".to_string());
        let persist = self.app_settings.ai.persist_chats;
        self.ai_panel.set_chat_session_key(key, persist);
    }

    pub(crate) fn menu_open_command_history(&mut self, ctx: &egui::Context) {
        if self
            .current_terminal()
            .map(|t| t.is_connected())
            .unwrap_or(false)
        {
            self.command_history_overlay.open_new();
        } else {
            self.status_message = crate::i18n::tr(
                ctx,
                "Connect to a terminal first to use command history",
                "请先连接终端后再使用命令历史",
            )
            .to_string();
        }
    }

    pub(crate) fn menu_copy_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签")
                .to_string();
            return;
        };
        let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) else {
            return;
        };
        if pane.terminal.menu_copy_to_clipboard() {
            self.status_message =
                crate::i18n::tr(ctx, "Copied to clipboard", "已复制到剪贴板").to_string();
        } else {
            self.status_message =
                crate::i18n::tr(ctx, "Terminal has nothing to copy", "终端无内容可复制").to_string();
        }
        ctx.request_repaint();
    }

    pub(crate) fn menu_paste_to_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签")
                .to_string();
            return;
        };
        if let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) {
            pane.terminal.menu_paste_from_clipboard(ctx);
        }
    }

    pub(crate) fn menu_select_all_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            return;
        };
        if let Some(pane) = self.tabs.get_mut(idx).and_then(|t| t.active_pane_mut()) {
            pane.terminal.menu_select_all();
            ctx.request_repaint();
        }
    }

    pub(crate) fn open_report_issue(&mut self, ctx: &egui::Context) {
        let url = crate::platform::github_new_issue_url(env!("CARGO_PKG_VERSION"));
        if !crate::platform::open_url(&url) {
            self.status_message = crate::i18n::tr(
                ctx,
                "Failed to open browser",
                "无法打开浏览器",
            )
            .to_string();
        }
    }

    pub(crate) fn menu_open_session_log_browser(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签")
                .to_string();
            return;
        };
        let session_id = self.tabs[idx].primary_session_id();
        let name = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.name.clone())
            .unwrap_or(session_id.clone());
        self.flush_session_log_buffers_for_session(&session_id);
        self.session_log_dialog
            .open_for(ctx, &session_id, &name, &self.session_log_settings);
    }

    fn toggle_terminal_search(&mut self) {
        self.show_terminal_search = !self.show_terminal_search;
        if self.show_terminal_search {
            self.terminal_search_pending_focus = true;
            self.rebuild_terminal_search_matches();
        } else {
            self.terminal_search_pending_focus = false;
            self.sync_terminal_search_highlight();
        }
    }

    fn sync_terminal_search_highlight(&mut self) {
        let q_len = self.terminal_search_query.chars().count();
        if !self.show_terminal_search || self.terminal_search_query.is_empty() || q_len == 0 {
            if let Some(t) = self.current_terminal_mut() {
                t.set_search_highlight(None);
            }
            return;
        }
        let Some(hit) = self.terminal_search_hits.get(self.terminal_search_cur).copied() else {
            if let Some(t) = self.current_terminal_mut() {
                t.set_search_highlight(None);
            }
            return;
        };
        if let Some(t) = self.current_terminal_mut() {
            let highlight = t.reveal_search_hit(hit).map(|(line, col)| (line, col, q_len));
            t.set_search_highlight(highlight);
        }
    }

    fn terminal_search_step(&mut self, delta: isize) {
        if self.terminal_search_hits.is_empty() {
            return;
        }
        let n = self.terminal_search_hits.len();
        self.terminal_search_cur = if delta >= 0 {
            (self.terminal_search_cur + 1) % n
        } else {
            (self.terminal_search_cur + n - 1) % n
        };
        self.sync_terminal_search_highlight();
    }

    fn rebuild_terminal_search_matches(&mut self) {
        self.terminal_search_hits.clear();
        let Some(t) = self.current_terminal() else {
            self.terminal_search_cur = 0;
            self.sync_terminal_search_highlight();
            return;
        };
        if self.terminal_search_query.is_empty() {
            self.terminal_search_cur = 0;
            self.sync_terminal_search_highlight();
            return;
        }
        self.terminal_search_hits =
            t.search_all(&self.terminal_search_query, self.terminal_search_ignore_case);
        if self.terminal_search_hits.is_empty() {
            self.terminal_search_cur = 0;
        } else {
            self.terminal_search_cur = self
                .terminal_search_cur
                .min(self.terminal_search_hits.len() - 1);
        }
        self.sync_terminal_search_highlight();
    }

    /// 选择会话
    pub fn select_session(&mut self, ctx: &egui::Context, session_id: &str) {
        self.selected_session_id = Some(session_id.to_string());

        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.primary_session_id() == session_id)
        {
            self.active_tab = Some(idx);
            return;
        }

        if let Some(session) = self.session_manager.get_session(session_id).cloned() {
            self.push_tab_connecting(ctx, &session);
        }
    }

    /// 创建并连接会话
    fn create_and_connect_session(&mut self, ctx: &egui::Context) {
        if self.new_session_name.is_empty() || self.new_session_host.is_empty() {
            self.status_message =
                crate::i18n::tr(ctx, "Enter session name and host", "请填写会话名称和主机地址")
                    .to_string();
            return;
        }

        // 创建会话
        let session = self.session_manager.create_session(
            &self.new_session_name,
            &self.new_session_host,
            self.new_session_port,
            &self.new_session_username,
            &self.new_session_password,
            &self.new_session_group,
            &self.new_session_private_key_path,
        );
        let sid = session.id.clone();
        let backend = self
            .new_session_vault
            .to_backend(&self.app_settings.vault);
        let proxy_jump = self.new_session_proxy_jump.trim().to_string();
        let proxy_command = self.new_session_proxy_command.trim().to_string();
        let local_forwards_text = self.new_session_local_forwards_text.clone();
        let remote_forwards_text = self.new_session_remote_forwards_text.clone();
        let dynamic_forwards_text = self.new_session_dynamic_forwards_text.clone();
        self.session_manager.patch_session(&sid, |s| {
            s.proxy_jump = proxy_jump.clone();
            s.proxy_command = proxy_command.clone();
            s.use_ssh_agent = self.new_session_use_ssh_agent;
            s.local_forwards_text = local_forwards_text;
            s.remote_forwards_text = remote_forwards_text;
            s.dynamic_forwards_text = dynamic_forwards_text;
            s.color_tag = self.new_session_color_tag.clone();
            if !matches!(backend, SecretBackend::LocalEncrypted) {
                s.secret_backend = backend.clone();
                if backend.is_vault() {
                    s.password.clear();
                }
            }
        });
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "session.create", AuditOutcome::Success)
                .with_session(&sid)
                .with_host(&session.host),
        );

        // 选择会话
        self.selected_session_id = Some(sid.clone());
        self.push_tab_connecting(ctx, &session);
        self.reset_new_session_form();
    }

    /// 重置新建会话表单
    fn reset_new_session_form(&mut self) {
        self.new_session_name.clear();
        self.new_session_host.clear();
        self.new_session_port = 22;
        self.new_session_port_str = "22".to_string();
        self.new_session_username.clear();
        self.new_session_password.clear();
        self.new_session_group = crate::i18n::Locale::from(self.app_settings.ui_language)
            .tr("Default", "默认")
            .to_string();
        self.new_session_color_tag.clear();
        self.new_session_private_key_path.clear();
        self.new_session_use_ssh_agent = true;
        self.new_session_proxy_jump.clear();
        self.new_session_proxy_command.clear();
        self.new_session_local_forwards_text.clear();
        self.new_session_remote_forwards_text.clear();
        self.new_session_dynamic_forwards_text.clear();
        self.new_session_vault = VaultSecretForm::default();
    }

    /// 删除会话
    pub fn delete_session(&mut self, ctx: &egui::Context, session_id: &str) {
        let display = self
            .session_manager
            .get_session(session_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| session_id.to_string());
        for t in &mut self.tabs {
            for pane in t.panes.iter_mut() {
                if pane.session_id == session_id {
                    pane.terminal.disconnect();
                }
            }
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "session.delete", AuditOutcome::Success)
                .with_session(session_id)
                .with_detail(serde_json::json!({ "name": display })),
        );
        self.session_manager.delete_session(session_id);
        self.tabs
            .retain(|t| !t.panes.iter().any(|p| p.session_id == session_id));
        if let Some(idx) = self.active_tab {
            if idx >= self.tabs.len() {
                self.active_tab = self.tabs.len().checked_sub(1);
            }
        }
        if self.selected_session_id.as_ref() == Some(&session_id.to_string()) {
            self.selected_session_id = None;
            if let Some(active) = self.active_tab {
                self.selected_session_id = self.tabs.get(active).map(|t| t.primary_session_id());
            }
        }
        self.status_message = format!(
            "{} {}",
            crate::i18n::tr(ctx, "Session deleted:", "已删除会话："),
            display
        );
    }

    fn open_edit_session_dialog(&mut self, session_id: &str) {
        if let Some(session) = self.session_manager.get_session(session_id).cloned() {
            self.edit_session_id = Some(session.id);
            self.edit_session_name = session.name;
            self.edit_session_host = session.host;
            self.edit_session_port = session.port;
            self.edit_session_port_str = session.port.to_string();
            self.edit_session_username = session.username;
            // FUNCTIONAL_SPEC §1.3.3：不将真实密码填入 UI
            self.edit_session_password = "****".to_string();
            self.edit_session_group = session.group;
            self.edit_session_private_key_path = session.private_key_path;
            self.edit_session_use_ssh_agent = session.use_ssh_agent;
            self.edit_session_color_tag = session.color_tag.clone();
            self.edit_session_keepalive_enabled = session.keepalive_enabled;
            self.edit_session_keepalive_interval_secs = session.keepalive_interval_secs;
            self.edit_session_keepalive_count_max = session.keepalive_count_max;
            self.edit_session_keepalive_auto_reconnect = session.keepalive_auto_reconnect;
            self.edit_session_proxy_jump = session.proxy_jump.clone();
            self.edit_session_proxy_command = session.proxy_command.clone();
            self.edit_session_local_forwards_text = session.local_forwards_text.clone();
            self.edit_session_remote_forwards_text = session.remote_forwards_text.clone();
            self.edit_session_dynamic_forwards_text = session.dynamic_forwards_text.clone();
            self.edit_session_vault = VaultSecretForm::from_backend(
                &session.secret_backend,
                &self.app_settings.vault.default_mount,
            );
            self.show_edit_session_dialog = true;
        }
    }

    fn save_edit_session(&mut self, ctx: &egui::Context) {
        let Some(session_id) = self.edit_session_id.clone() else {
            return;
        };

        if self.edit_session_name.is_empty() || self.edit_session_host.is_empty() {
            self.status_message =
                crate::i18n::tr(ctx, "Session name and host cannot be empty", "会话名称和主机地址不能为空")
                    .to_string();
            return;
        }

        let old_password = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.password.clone())
            .unwrap_or_default();
        let trimmed = self.edit_session_password.trim();
        let password_to_store = if trimmed.is_empty() || trimmed == "****" {
            old_password
        } else {
            self.edit_session_password.clone()
        };

        let updated = self.session_manager.update_session(
            &session_id,
            &self.edit_session_name,
            &self.edit_session_host,
            self.edit_session_port,
            &self.edit_session_username,
            &password_to_store,
            &self.edit_session_group,
            &self.edit_session_private_key_path,
        );

        if updated {
            let color = self.edit_session_color_tag.clone();
            let ka_on = self.edit_session_keepalive_enabled;
            let ka_int = self.edit_session_keepalive_interval_secs;
            let ka_max = self.edit_session_keepalive_count_max;
            let ka_ar = self.edit_session_keepalive_auto_reconnect;
            let backend = self
                .edit_session_vault
                .to_backend(&self.app_settings.vault);
            let _ = self.session_manager.patch_session(&session_id, |s| {
                s.color_tag = color;
                s.keepalive_enabled = ka_on;
                s.keepalive_interval_secs = ka_int;
                s.keepalive_count_max = ka_max;
                s.keepalive_auto_reconnect = ka_ar;
                s.proxy_jump = self.edit_session_proxy_jump.trim().to_string();
                s.proxy_command = self.edit_session_proxy_command.trim().to_string();
                s.use_ssh_agent = self.edit_session_use_ssh_agent;
                s.local_forwards_text = self.edit_session_local_forwards_text.clone();
                s.remote_forwards_text = self.edit_session_remote_forwards_text.clone();
                s.dynamic_forwards_text = self.edit_session_dynamic_forwards_text.clone();
                s.secret_backend = backend.clone();
                if backend.is_vault() {
                    s.password.clear();
                }
            });
            self.audit_logger.record(
                AuditEvent::new(AuditCategory::Session, "session.update", AuditOutcome::Success)
                    .with_session(&session_id)
                    .with_host(&self.edit_session_host),
            );
            self.status_message = format!(
                "{} {}",
                crate::i18n::tr(ctx, "Session updated:", "已更新会话："),
                self.edit_session_name
            );
            if self.selected_session_id.as_deref() == Some(session_id.as_str()) {
                self.select_session(ctx, &session_id);
            }
            self.show_edit_session_dialog = false;
        } else {
            self.status_message = status_message_wrap_error(
                crate::i18n::tr(ctx, "Failed to update session", "更新会话失败").to_string(),
            );
        }
    }

    /// 注册命令片段栏槽位（须在 Central 之前）。实际 UI 见 [`show_fragment_panel_foreground`]。
    fn show_fragment_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        dock_col_w: f32,
    ) {
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let fragment_panel = egui::SidePanel::right(layout_util::FRAGMENT_PANEL_ID)
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .show_separator_line(false)
            // 仅占布局宽；勿在此绘制内容（CentralPanel 后绘会盖住）。内容在 Foreground Area 重绘。
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                crate::ui::chrome::paint_right_dock_left_gap(ui, theme);
                self.fragment_panel_slot_rect = Some(ui.max_rect());
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        if let Some(slot) = self.fragment_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, &mut self.right_dock_outer_left_x);
        } else {
            layout_util::record_right_dock_panel(
                &fragment_panel.response,
                &mut self.right_dock_outer_left_x,
            );
        }
        let _ = theme;
    }

    /// Central 之后绘制命令片段（egui 规定 Central 最后绘制，会盖住同层 SidePanel 的像素）。
    pub(crate) fn show_fragment_panel_foreground(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if !self.show_fragment_panel {
            return;
        }
        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.fragment_panel_slot_rect,
            ctx,
            layout_util::FRAGMENT_PANEL_ID,
            layout_util::SidePanelProfile::Fragment,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_fragment_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_fragment_fg",
            ctx,
            &geom,
            layout_util::SidePanelProfile::Fragment,
            |ui, panel_w| {
                self.show_fragment_panel_contents(ui, theme, panel_w);
            },
        );
    }

    /// 命令片段面板正文（§5 扁平列表 + 四标签）。
    fn show_fragment_panel_contents(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        panel_w: f32,
    ) {
        self.fragment_filter_category = match self.fragment_filter_category.as_str() {
            "常用" | "frequent" => "frequent".to_string(),
            "全部" | "all" => "all".to_string(),
            "Docker" => "Docker".to_string(),
            "K8s" => "K8s".to_string(),
            _ => "all".to_string(),
        };
        ui.set_max_width(panel_w);

        let prev_gap_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        theme.frame_right_dock_header_band().show(ui, |ui| {
            let ctx_ref = ui.ctx().clone();
            if crate::ui::chrome::dock_panel_title_close_only(
                ui,
                theme,
                crate::ui::icons::IconId::Fragment,
                crate::i18n::tr(&ctx_ref, "Command snippets", "命令片段"),
                crate::i18n::tr(&ctx_ref, "Close command snippets sidebar", "关闭命令片段侧栏"),
            ) {
                self.show_fragment_panel = false;
            }
        });
        crate::ui::chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap_y;
        ui.add_space(theme.spacing_dock_section_gap());

        // 这里必须用 `form_singleline_field`（有框）且不要走 `panel_search_row/search_field`：
        // 后两者会引入额外外边距或行高壳层，导致「命令片段」顶部节奏与 SFTP 不一致。
        crate::ui::chrome::form_singleline_field(
            ui,
            theme,
            Self::id_fragment_panel_search(),
            &mut self.fragment_search_query,
            crate::i18n::tr(ui.ctx(), "Search snippets…", "搜索片段…"),
            panel_w,
            false,
        );
        ui.add_space(theme.spacing_dock_control_gap());

        ui.horizontal(|ui| {
            if crate::ui::chrome::panel_action_icon_button(
                ui,
                theme,
                crate::ui::icons::IconId::Plus,
                crate::i18n::tr(ui.ctx(), "New snippet", "新建片段"),
            )
            .clicked()
            {
                self.fragment_library.open = true;
            }
            if crate::ui::chrome::panel_action_icon_button(
                ui,
                theme,
                crate::ui::icons::IconId::Fragment,
                crate::i18n::tr(ui.ctx(), "Analytics", "分析"),
            )
            .clicked()
            {
                self.open_fragment_analytics_dialog();
            }
        });
        ui.add_space(theme.spacing_dock_control_gap());

        if self.team_service.is_configured() && self.team_service.is_logged_in() {
            let ctx_scope = ui.ctx().clone();
            let personal_lbl =
                crate::i18n::tr(&ctx_scope, "Personal", "个人");
            let team_lbl = crate::i18n::tr(&ctx_scope, "Team", "团队");
            let market_lbl = crate::i18n::tr(&ctx_scope, "Market", "市场");
            let scope_key = match self.fragment_list_scope {
                FragmentListScope::Personal => "personal",
                FragmentListScope::Team => "team",
                FragmentListScope::Market => "market",
            };
            let scope_defs: [(&str, &str); 3] = [
                ("personal", personal_lbl),
                ("team", team_lbl),
                ("market", market_lbl),
            ];
            let chip_h = theme.size_panel_filter_chip_h();
            let chip_min = egui::vec2(
                theme.size_panel_header_btn_min_w(),
                chip_h,
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                for (key, label) in &scope_defs {
                    let selected = scope_key == *key;
                    if crate::ui::chrome::filter_chip_button(
                        ui,
                        theme,
                        label,
                        selected,
                        chip_min,
                    )
                    .clicked()
                    {
                        self.fragment_list_scope = match *key {
                            "team" => FragmentListScope::Team,
                            "market" => {
                                self.market_catalog_query_fingerprint = (
                                    self.fragment_filter_category.clone(),
                                    self.fragment_search_query.clone(),
                                );
                                self.start_market_catalog_refresh();
                                FragmentListScope::Market
                            }
                            _ => FragmentListScope::Personal,
                        };
                    }
                }
            });
            ui.add_space(theme.spacing_dock_control_gap());
        }

        if self.fragment_list_scope == FragmentListScope::Market {
            ui.horizontal(|ui| {
                if crate::ui::chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Refresh,
                    crate::i18n::tr(ui.ctx(), "Refresh market catalog", "刷新市场目录"),
                )
                .clicked()
                {
                    self.start_market_catalog_refresh();
                }
            });
            if self.market_catalog_refresh_rx.is_some() {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ui.ctx(),
                        "Refreshing market catalog…",
                        "正在刷新市场目录…",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
            } else if let Some(err) = &self.market_catalog.last_error {
                let hint = if err.starts_with("catalog_not_deployed") {
                    crate::i18n::tr(
                        ui.ctx(),
                        "Market API not deployed on server; showing cached catalog.",
                        "服务端尚未部署市场接口，当前显示本地缓存。",
                    )
                    .to_string()
                } else {
                    err.clone()
                };
                ui.label(
                    egui::RichText::new(hint)
                        .size(theme.font_size_caption())
                        .color(theme.text_tertiary()),
                );
            } else if self.market_catalog.api_available {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ui.ctx(),
                        "Synced from market catalog",
                        "已从市场目录同步",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
            }
            if self.market_catalog.has_more() {
                let loading = self.market_catalog_refresh_rx.is_some()
                    || self.market_catalog.loading_more;
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!loading, egui::Button::new(crate::i18n::tr(
                            ui.ctx(),
                            "Load more",
                            "加载更多",
                        )))
                        .clicked()
                    {
                        self.start_market_catalog_load_more();
                    }
                    if loading {
                        ui.spinner();
                    }
                });
            }
            ui.add_space(theme.spacing_dock_control_gap());
        }

        if self.fragment_list_scope == FragmentListScope::Team
            && self.team_service.is_logged_in()
        {
            let role = self.team_service.state.current_role();
            let role_name = self
                .team_service
                .state
                .current_membership()
                .map(|m| m.role.as_str())
                .unwrap_or("viewer");
            let role_lbl = format!(
                "{}: {role_name}",
                crate::i18n::tr(ui.ctx(), "Role", "角色"),
            );
            ui.label(
                crate::ui::chrome::rich_caption(theme, &role_lbl).weak(),
            );
            if let Some(detail) = &self.team_service.current_team_detail {
                if !detail.description.is_empty() {
                    ui.label(
                        crate::ui::chrome::rich_caption(theme, &detail.description).weak(),
                    );
                }
            }
            let can_edit = role.can_edit();
            let can_delete = role.can_delete();
            ui.horizontal(|ui| {
                if can_edit
                    && crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plus,
                        crate::i18n::tr(ui.ctx(), "New team snippet", "新建团队片段"),
                    )
                    .clicked()
                {
                    open_create_editor(&mut self.team_fragment_editor);
                }
                if can_edit
                    && self.team_fragment_selected_id.is_some()
                    && crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Fragment,
                        crate::i18n::tr(ui.ctx(), "Edit", "编辑"),
                    )
                    .clicked()
                {
                    if let Some(id) = self.team_fragment_selected_id.clone() {
                        if let Some(frag) = self.team_service.find_team_fragment(&id) {
                            open_edit_editor(&mut self.team_fragment_editor, &frag);
                        }
                    }
                }
                if can_delete
                    && self.team_fragment_selected_id.is_some()
                    && crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        crate::i18n::tr(ui.ctx(), "Delete", "删除"),
                    )
                    .clicked()
                {
                    if let Some(id) = self.team_fragment_selected_id.take() {
                        match crate::core::team::delete_team_fragment_blocking(
                            &mut self.team_service,
                            &id,
                        ) {
                            Ok(()) => {
                                self.audit_logger.record(
                                    AuditEvent::new(
                                        AuditCategory::Fragment,
                                        "fragment.delete",
                                        AuditOutcome::Success,
                                    )
                                    .with_resource(&id),
                                );
                                self.status_message = crate::i18n::tr(
                                    ui.ctx(),
                                    "Team snippet deleted",
                                    "已删除团队片段",
                                )
                                .to_string();
                            }
                            Err(e) => self.status_message = e,
                        }
                    }
                }
                if crate::ui::chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    crate::i18n::tr(ui.ctx(), "Sync", "同步"),
                )
                .clicked()
                {
                    self.team_service.spawn_sync_current_team();
                }
            });
            ui.add_space(theme.spacing_dock_control_gap());
        }

        // §5.3：分类筛选 + 右侧排序（与芯片同排，不再单独占「片段列表」行）
        let ctx_owned = ui.ctx().clone();
        let chip_defs: [(&str, &str); 4] = [
            ("frequent", crate::i18n::tr(&ctx_owned, "Pinned", "常用")),
            ("Docker", "Docker"),
            ("K8s", "K8s"),
            ("all", crate::i18n::tr(&ctx_owned, "All", "全部")),
        ];
        let sort_lbl = crate::i18n::fragment_sort_chip_short(&ctx_owned, self.fragment_sort_by);
        let sort_hover = crate::i18n::filter_sort_cycle_hint_fragments(&ctx_owned);
        let chip_row = crate::ui::chrome::filter_chip_row_with_sort(
            ui,
            theme,
            &chip_defs,
            self.fragment_filter_category.as_str(),
            crate::ui::icons::fragment_sort_icon(self.fragment_sort_by),
            sort_lbl,
            sort_hover,
        );
        if let Some(picked) = chip_row.picked {
            self.fragment_filter_category = picked;
        }
        if chip_row.cycle_sort {
            self.fragment_sort_by = match self.fragment_sort_by {
                SortBy::UsageCount => SortBy::SuccessRate,
                SortBy::SuccessRate => SortBy::LastUsed,
                SortBy::LastUsed => SortBy::Name,
                SortBy::Name => SortBy::UsageCount,
            };
            self.fragment_manager.sort(self.fragment_sort_by);
        }
        ui.add_space(theme.spacing_dock_control_gap());

        self.sync_market_catalog_query_fingerprint();

        let search_lower = self.fragment_search_query.to_lowercase();
        let search_match = |f: &FragmentStats| {
            search_lower.is_empty()
                || f.title.to_lowercase().contains(&search_lower)
                || f.command.to_lowercase().contains(&search_lower)
        };

        let source: Vec<FragmentStats> = match self.fragment_list_scope {
            FragmentListScope::Personal => self.fragment_manager.get_all().to_vec(),
            FragmentListScope::Team => self.team_service.team_fragments_as_stats(),
            FragmentListScope::Market => {
                if self.market_catalog_refresh_pending && self.market_catalog_refresh_rx.is_none() {
                    self.start_market_catalog_refresh();
                }
                let mut list = self.market_catalog.to_fragment_stats_list();
                if list.is_empty() {
                    list = self
                        .fragment_manager
                        .get_all()
                        .iter()
                        .filter(|f| {
                            f.tags
                                .iter()
                                .any(|t| t.eq_ignore_ascii_case("market"))
                        })
                        .cloned()
                        .collect();
                }
                list
            }
        };

        match self.fragment_list_scope {
            FragmentListScope::Personal => {
                let mut top: Vec<_> = self
                    .fragment_manager
                    .get_all()
                    .iter()
                    .filter(|f| f.usage_count > 0)
                    .cloned()
                    .collect();
                top.sort_by_key(|f| std::cmp::Reverse(f.usage_count));
                top.truncate(5);
                if !top.is_empty() {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ui.ctx(),
                            "Top snippets (local usage)",
                            "常用片段（本地统计）",
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    for f in &top {
                        ui.label(
                            egui::RichText::new(format!(
                                "· {} — {}×",
                                f.title, f.usage_count
                            ))
                            .size(theme.font_size_small())
                            .color(theme.color_body_text_muted()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());
                }
            }
            FragmentListScope::Team if self.team_service.is_logged_in() => {
                let mut top = self.team_service.team_fragments_as_stats();
                top.retain(|f| f.usage_count > 0);
                top.sort_by_key(|f| std::cmp::Reverse(f.usage_count));
                top.truncate(5);
                if !top.is_empty() {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ui.ctx(),
                            "Team Top snippets",
                            "团队常用片段",
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    for f in &top {
                        ui.label(
                            egui::RichText::new(format!(
                                "· {} — {}×",
                                f.title, f.usage_count
                            ))
                            .size(theme.font_size_small())
                            .color(theme.color_body_text_muted()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());
                }
            }
            _ => {}
        }

        let mut work: Vec<FragmentStats> = source
            .iter()
            .filter(|f| search_match(f))
            .cloned()
            .collect();

        match self.fragment_filter_category.as_str() {
            "Docker" => work.retain(|f| f.category == "Docker"),
            "K8s" => work.retain(|f| f.category == "K8s"),
            "frequent" => {
                work.retain(|f| f.usage_count > 0);
                if work.is_empty() {
                    work = source
                        .iter()
                        .filter(|f| search_match(f))
                        .cloned()
                        .collect();
                }
                work.sort_by_key(|f| std::cmp::Reverse(f.usage_count));
            }
            _ => {
                let sort = self.fragment_sort_by;
                match sort {
                    SortBy::UsageCount => {
                        work.sort_by_key(|f| std::cmp::Reverse(f.usage_count))
                    }
                    SortBy::SuccessRate => work.sort_by(|a, b| {
                        b.success_rate()
                            .partial_cmp(&a.success_rate())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }),
                    SortBy::LastUsed => {
                        work.sort_by_key(|f| std::cmp::Reverse(f.last_used))
                    }
                    SortBy::Name => work.sort_by(|a, b| a.title.cmp(&b.title)),
                }
            }
        }

        let scroll_h = ui.available_height().max(80.0);
        let prev_extreme = ui.visuals().extreme_bg_color;
        ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
        egui::ScrollArea::vertical()
            .id_source("mistterm_fragment_list_scroll")
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .scroll_bar_visibility(
                egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
            )
            .show(ui, |ui| {
                ui.set_max_width(panel_w);
                if work.is_empty() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ui.ctx(),
                                    "No snippets match your filters",
                                    "暂无片段",
                                ))
                                    .size(theme.font_size_panel_title())
                                    .color(theme.text_tertiary()),
                            );
                        }
                        for frag in &work {
                            let stats_line = format_fragment_stats_line(ui.ctx(), frag);
                            let tag_label = frag.tags.first().cloned().unwrap_or_else(|| {
                                if frag.category.is_empty() {
                                    "—".to_string()
                                } else {
                                    frag.category.clone()
                                }
                            });
                            let row_resp = crate::ui::chrome::fragment_list_row(
                                ui,
                                theme,
                                crate::ui::chrome::FragmentListRow {
                                    title: &frag.title,
                                    command: &frag.command,
                                    stats_line: &stats_line,
                                    tag_label: &tag_label,
                                },
                            );
                            let is_team_scope =
                                self.fragment_list_scope == FragmentListScope::Team;
                            let is_market_scope =
                                self.fragment_list_scope == FragmentListScope::Market;
                            let selected = self
                                .team_fragment_selected_id
                                .as_deref()
                                == Some(frag.id.as_str());
                            if is_team_scope && selected {
                                ui.painter().rect_stroke(
                                    row_resp.row.rect,
                                    theme.radius_card(),
                                    egui::Stroke::new(1.5, theme.accent_color()),
                                );
                            }
                            if row_resp.title.clicked() {
                                self.begin_fragment_insert(ui.ctx(), frag);
                            } else if is_team_scope && row_resp.row.clicked() {
                                self.team_fragment_selected_id = Some(frag.id.clone());
                            }
                            if is_market_scope {
                                let frag_clone = frag.clone();
                                row_resp.row.context_menu(|ui| {
                                    crate::ui::chrome::apply_context_menu_style(ui, theme);
                                    if crate::ui::chrome::popup_menu_button(
                                        ui,
                                        theme,
                                        crate::i18n::tr(
                                            ui.ctx(),
                                            "Add to personal library",
                                            "添加到个人库",
                                        ),
                                    )
                                    .clicked()
                                    {
                                        if let Some(item) =
                                            self.market_item_for_stats(&frag_clone)
                                        {
                                            self.install_market_fragment(ui.ctx(), &item);
                                        }
                                        ui.close_menu();
                                    }
                                });
                            }
                            ui.add_space(theme.spacing_list_item_gap());
                        }
                    });
        ui.visuals_mut().extreme_bg_color = prev_extreme;
    }

    /// 从右侧片段列表点击：支持片段库定义的变量、命令里的 `<占位符>`，以及会话字段替换。
    fn begin_fragment_insert(&mut self, egui_ctx: &egui::Context, fragment: &FragmentStats) {
        if self.active_tab.is_none() {
            self.status_message =
                crate::i18n::tr(egui_ctx, "Open a terminal tab first", "请先打开终端标签")
                    .to_string();
            return;
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Fragment, "fragment.insert", AuditOutcome::Success)
                .with_resource(&fragment.id)
                .with_detail(serde_json::json!({ "title": fragment.title })),
        );

        if fragment.has_variables() {
            self.variable_dialog.open = true;
            self.variable_dialog.fragment_id = Some(fragment.id.clone());
            self.variable_dialog.fragment_title = fragment.title.clone();
            self.variable_dialog.values = fragment.variable_defaults();
            self.variable_dialog.command_edit =
                self.build_fragment_command_preview(fragment, &self.variable_dialog.values);
            self.variable_dialog.paste_after_fill = true;
            self.variable_dialog.last_finalize_error = None;
            return;
        }

        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        let rhai_ctx = merge_rhai_context(session, &HashMap::new());
        let after_rhai = match expand_rhai_blocks(&fragment.command, &rhai_ctx) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = status_message_wrap_error(crate::i18n::localize_fragment_expr_error(
                    crate::i18n::language(egui_ctx),
                    &e,
                ));
                return;
            }
        };
        let expanded = expand_command_template(
            &after_rhai,
            session,
            &std::collections::HashMap::new(),
        );

        let needs_user = placeholders_needing_user(&expanded);

        if needs_user.is_empty() {
            self.insert_expanded_fragment_with_stats(egui_ctx, &fragment.id, &expanded);
        } else {
            self.pending_fragment_id = Some(fragment.id.clone());
            self.pending_fragment_name = fragment.title.clone();
            self.pending_fragment_command = expanded;
            self.pending_fragment_vars = needs_user
                .into_iter()
                .map(|k| (k, String::new()))
                .collect();
            self.sync_pending_fragment_command_edit();
            self.fragment_vars_completion = FragmentVarsCompletion::PasteInsertStats;
            self.show_fragment_vars_dialog = true;
        }
    }

    /// 在指定标签页插入片段文本：`record_execution` 记统计，`pending_fragment_insert` 处理连接中空终端。
    fn insert_fragment_at_tab_index(
        &mut self,
        ctx: &egui::Context,
        tab_idx: usize,
        fragment_id: Option<&str>,
        command: &str,
    ) {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            self.status_message =
                crate::i18n::tr(ctx, "Tab index out of range", "标签页不存在").to_string();
            return;
        };
        let start = std::time::Instant::now();
        let Some(pane) = tab.active_pane_mut() else { return };
        match pane.terminal.insert_fragment(command) {
            Ok(_) => {
                let dur_ms = start.elapsed().as_millis().max(1) as u64;
                if let Some(fid) = fragment_id {
                    self.record_fragment_execution(fid, true, dur_ms);
                }
                self.status_message = format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Inserted command:", "插入命令："),
                    command
                );
            }
            Err(e) => {
                if e == TerminalView::ERR_FRAGMENT_NOT_CONNECTED && pane.terminal.is_connecting() {
                    self.pending_fragment_insert = Some((
                        tab_idx,
                        fragment_id.map(|id| id.to_string()),
                        command.to_string(),
                    ));
                    self.status_message = crate::i18n::tr(
                        ctx,
                        "Connecting… fragment will insert when the session is ready",
                        "连接建立中，片段将在连接成功后自动插入",
                    )
                    .to_string();
                } else {
                    let dur_ms = start.elapsed().as_millis().max(1) as u64;
                    if let Some(fid) = fragment_id {
                        self.record_fragment_execution(fid, false, dur_ms);
                    }
                    self.status_message = status_message_wrap_error(format!(
                        "{} {}",
                        crate::i18n::tr(ctx, "Insert failed:", "插入失败："),
                        localize_terminal_insert_fragment_error(ctx, &e)
                    ));
                }
            }
        }
    }

    fn try_flush_pending_fragment_insert(&mut self, ctx: &egui::Context) {
        let Some((idx, fid_opt, cmd)) = self.pending_fragment_insert.take() else {
            return;
        };
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        if !tab
            .active_terminal()
            .map(|t| t.is_connected())
            .unwrap_or(false)
        {
            self.pending_fragment_insert = Some((idx, fid_opt, cmd));
            return;
        }
        self.insert_fragment_at_tab_index(ctx, idx, fid_opt.as_deref(), &cmd);
    }

    fn insert_expanded_fragment_with_stats(&mut self, ctx: &egui::Context, id: &str, expanded: &str) {
        let Some(idx) = self.active_tab else {
            self.status_message =
                crate::i18n::tr(ctx, "Open a terminal tab first", "请先打开终端标签").to_string();
            return;
        };
        self.insert_fragment_at_tab_index(ctx, idx, Some(id), expanded);
    }

    /// 底栏左侧：连接 / 侧栏会话 / 日志等状态信息成组排列（不拉满整行）。
    fn status_bar_info_cluster(&mut self, ui: &mut egui::Ui, theme: &crate::ui::theme::Theme) {
        let bar_ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing = egui::vec2(theme.spacing_sm(), 0.0);

        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get(idx) {
                if let Some(conn) = tab
                    .active_terminal()
                    .and_then(|t| t.connection_status_for_bar(theme))
                {
                    Self::status_connection_chip(ui, &conn, theme);
                }
                if let Some(ssh_id) = tab
                    .active_terminal()
                    .and_then(|t| t.ssh_session_id())
                {
                    let n = self.port_forward_panel.active_count_for(ssh_id);
                    let en = crate::i18n::language(&bar_ctx) == crate::i18n::UiLanguage::En;
                    if let Some(label) = status_bar_summary(n, en) {
                        crate::ui::chrome::status_text_chip(
                            ui,
                            theme,
                            &label,
                            theme.accent_color(),
                        );
                    }
                }
            }
        }

        if let Some(sid) = &self.selected_session_id {
            let active_sid = self
                .active_tab
                .and_then(|i| self.tabs.get(i).map(|t| t.primary_session_id()));
            if active_sid.as_ref() != Some(sid) {
                let label = self
                    .session_manager
                    .get_session(sid)
                    .map(|s| {
                        format!(
                            "{} {}",
                            crate::i18n::tr(&bar_ctx, "Sidebar:", "侧栏："),
                            s.name
                        )
                    })
                    .unwrap_or_else(|| {
                        crate::i18n::tr(&bar_ctx, "Sidebar: unnamed", "侧栏：未命名").to_string()
                    });
                crate::ui::chrome::status_text_chip(ui, theme, &label, theme.text_primary());
            }
        }

        if self.session_log_enabled {
            if let Some(log_label) = self.active_tab_log_status(&bar_ctx) {
                let chip = theme
                    .frame_status_chip()
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(log_label)
                                    .size(theme.font_size_status_bar())
                                    .color(theme.text_primary()),
                            )
                            .sense(egui::Sense::click()),
                        )
                    })
                    .inner
                    .on_hover_text(crate::i18n::tr(
                        &bar_ctx,
                        "Browse local recording of this session's terminal output",
                        "查看本会话的终端输出录制（本地日志文件）",
                    ));
                if chip.clicked() {
                    if let Some(idx) = self.active_tab {
                        let sid = self.tabs.get(idx).map(|t| t.primary_session_id());
                        let name = sid.as_deref().and_then(|id| {
                            self.session_manager.get_session(id).map(|s| s.name.clone())
                        });
                        if let (Some(id), Some(n)) = (sid, name) {
                            self.flush_session_log_buffers_for_session(&id);
                            self.session_log_dialog.open_for(
                                &bar_ctx,
                                &id,
                                &n,
                                &self.session_log_settings,
                            );
                        }
                    }
                }
            }
        }

        if let Some(metrics) = self.monitor_panel.status_bar_metrics_line(&bar_ctx) {
            crate::ui::chrome::status_text_chip(ui, theme, &metrics, theme.text_primary());
        }

        self.paint_team_account_status_chip(ui, theme, &bar_ctx);

        if self.auto_reconnect_enabled {
            crate::ui::chrome::status_icon_chip(
                ui,
                theme,
                crate::ui::icons::IconId::Refresh,
                crate::i18n::tr(&bar_ctx, "Auto-reconnect", "自动重连"),
            );
        }

        if self.sidebar_collapsed
            && crate::ui::chrome::status_restore_chip(
                ui,
                theme,
                crate::i18n::tr(&bar_ctx, "Connections", "连接"),
                self.tabs.len(),
            )
            .on_hover_text(crate::i18n::tr(
                &bar_ctx,
                "Expand connection sidebar",
                "展开左侧连接栏",
            ))
            .clicked()
        {
            self.sidebar_collapsed = false;
            self.sidebar_user_dismissed_responsive = false;
        }

        if !self.status_message.is_empty() {
            crate::ui::chrome::status_text_chip(
                ui,
                theme,
                &truncate_status(status_message_body(&self.status_message), 36),
                status_message_text_color(&self.status_message, theme),
            );
        }
    }

    /// 底栏团队账户：已登录时显示用户与当前团队，点击打开偏好设置。
    fn paint_team_account_status_chip(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        bar_ctx: &egui::Context,
    ) {
        if !self.team_service.is_configured() || !self.team_service.is_logged_in() {
            return;
        }
        let team_name = self.team_service.current_team_name();
        let (display, email) = self
            .team_service
            .state
            .user
            .as_ref()
            .map(|u| {
                let name = if !u.display_name.is_empty() {
                    u.display_name.as_str()
                } else if !u.username.is_empty() {
                    u.username.as_str()
                } else {
                    u.email.as_str()
                };
                (name.to_string(), u.email.clone())
            })
            .unwrap_or_else(|| {
                (
                    crate::i18n::tr(bar_ctx, "Signed in", "已登录").to_string(),
                    String::new(),
                )
            });

        let chip_text = format!(
            "{} · {} @ {}",
            crate::i18n::tr(bar_ctx, "Team", "团队"),
            truncate_status(&display, 14),
            truncate_status(&team_name, 12),
        );
        let hover = if email.is_empty() {
            format!(
                "{}\n{}",
                team_name,
                crate::i18n::tr(
                    bar_ctx,
                    "Click to open team account settings",
                    "点击打开团队账户设置",
                ),
            )
        } else {
            format!(
                "{}\n{}\n{}",
                email,
                team_name,
                crate::i18n::tr(
                    bar_ctx,
                    "Click to open team account settings",
                    "点击打开团队账户设置",
                ),
            )
        };

        let resp = theme.frame_status_chip().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let px = theme.font_size_status_bar();
                let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                crate::ui::icons::paint_icon(
                    ui,
                    r,
                    crate::ui::icons::IconId::Cloud,
                    theme.green_color(),
                    px,
                );
                ui.label(
                    egui::RichText::new(chip_text)
                        .size(theme.font_size_status_bar())
                        .color(theme.green_color()),
                );
            });
        });
        if resp.response.on_hover_text(hover).clicked() {
            self.show_preferences_dialog = true;
        }
    }

    /// 当前标签 SSH 连接状态（主机 + 状态字色），不占用终端 scrollback。
    fn status_connection_chip(
        ui: &mut egui::Ui,
        status: &crate::ui::terminal::ConnectionBarStatus,
        theme: &crate::ui::theme::Theme,
    ) {
        let host = truncate_status(&status.host_line, 40);
        theme.frame_status_chip().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm();
                let sz = theme.font_size_status_bar();
                ui.label(
                    egui::RichText::new(host)
                        .size(sz)
                        .color(theme.text_primary()),
                );
                ui.label(
                    egui::RichText::new("·")
                        .size(sz)
                        .color(theme.text_tertiary()),
                );
                ui.label(
                    egui::RichText::new(&status.state_line)
                        .size(sz)
                        .color(status.state_color),
                );
            });
        });
    }

    /// 底栏：改造后 **32px** 单行状态栏（左复原+连接信息，右工具图标+统计）
    fn show_bottom_chrome(&mut self, ctx: &egui::Context) {
        let theme = self.theme_manager.current_theme().clone();
        let status_h = theme.status_bar_height();
        let fragment_count = self.fragment_manager.get_all().len();
        let total_runs: u32 = self
            .fragment_manager
            .get_all()
            .iter()
            .map(|f| f.usage_count)
            .sum();

        egui::TopBottomPanel::bottom("bottom_chrome")
            .exact_height(status_h)
            .frame(theme.frame_chrome_bar())
            .show(ctx, |ui| {
                let screen = ui.ctx().screen_rect();
                let inner = ui.max_rect();
                let m = theme.margin_chrome_bar();
                let panel_top = inner.min.y - m.top;
                let panel_rect = egui::Rect::from_min_max(
                    egui::pos2(screen.min.x, panel_top),
                    egui::pos2(screen.max.x, inner.max.y + m.bottom),
                );
                ui.painter()
                    .rect_filled(panel_rect, 0.0, theme.chrome_bar_fill());
                let content_clip = egui::Rect::from_min_max(
                    egui::pos2(screen.min.x, panel_top + 2.0),
                    egui::pos2(screen.max.x, inner.max.y),
                );
                ui.set_clip_rect(content_clip);
                let content_h = theme.chrome_bar_content_height(status_h);

                let status_ctx = ui.interact(
                    inner,
                    egui::Id::new("status_bar_context"),
                    egui::Sense::click(),
                );
                status_ctx.context_menu(|ui| {
                    crate::ui::chrome::apply_context_menu_style(ui, &theme);
                    let import_label = format!(
                        "{}…",
                        crate::i18n::menu::labels(crate::i18n::language(ui.ctx())).import_ssh
                    );
                    if crate::ui::chrome::popup_menu_button(ui, &theme, &import_label).clicked() {
                        self.open_ssh_import_dialog(ui.ctx());
                    }
                });

                let row_w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(row_w, content_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(content_h);
                        ui.set_max_height(content_h);
                        ui.spacing_mut().item_spacing =
                            egui::vec2(theme.spacing_status_left_gap(), 0.0);

                        self.status_bar_info_cluster(ui, &theme);

                        let remaining_w = ui.available_width().max(0.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(remaining_w, content_h),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_height(content_h);
                                ui.set_max_height(content_h);
                                ui.spacing_mut().item_spacing =
                                    egui::vec2(theme.spacing_tool_btn_gap(), 0.0);

                                let fragment_chip = match crate::i18n::language(ctx) {
                                    crate::i18n::UiLanguage::En => format!(
                                        "{fragment_count} snippets · {total_runs} runs"
                                    ),
                                    crate::i18n::UiLanguage::Zh => format!(
                                        "{fragment_count} 片段 · {total_runs} 次"
                                    ),
                                };
                                crate::ui::chrome::status_text_chip(
                                    ui,
                                    &theme,
                                    &fragment_chip,
                                    theme.text_primary(),
                                );
                                ui.label(
                                    egui::RichText::new("|")
                                        .size(theme.font_size_status_bar())
                                        .color(theme.color_caption_text()),
                                );
                                ui.add_space(theme.spacing_status_right_gap());

                                let menu = crate::i18n::menu::labels(crate::i18n::language(ctx));
                                if crate::ui::chrome::status_tool_button(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Fragment,
                                    crate::i18n::tr(ctx, "Snippets", "片段"),
                                    &format!(
                                        "{} · {}",
                                        menu.fragment_panel,
                                        crate::platform::accel("K")
                                    ),
                                )
                                .clicked()
                                {
                                    if self.show_fragment_panel {
                                        self.show_fragment_panel = false;
                                    } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                        self.show_fragment_panel = true;
                                    }
                                }
                                if crate::ui::chrome::status_tool_button(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Folder,
                                    crate::i18n::tr(ctx, "Files", "文件"),
                                    crate::i18n::tr(
                                        ctx,
                                        "SFTP files · browse / upload / download",
                                        "SFTP 文件 · 浏览/上传/下载",
                                    ),
                                )
                                .clicked()
                                {
                                    if self.show_sftp_panel {
                                        self.show_sftp_panel = false;
                                    } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                        self.toggle_sftp_panel(ctx);
                                    }
                                }
                                let fwd_count = self
                                    .active_tab
                                    .and_then(|idx| self.tabs.get(idx))
                                    .and_then(|tab| tab.active_terminal())
                                    .and_then(|t| t.ssh_session_id())
                                    .map(|id| self.port_forward_panel.active_count_for(id))
                                    .unwrap_or(0);
                                let fwd_tip = if fwd_count > 0 {
                                    format!(
                                        "{} · {}",
                                        crate::i18n::tr(
                                            ctx,
                                            "Port forwarding · active",
                                            "端口转发 · 运行中",
                                        ),
                                        fwd_count
                                    )
                                } else {
                                    crate::i18n::tr(
                                        ctx,
                                        "Port forwarding · -L / -R / SOCKS",
                                        "端口转发 · 本地/远程/SOCKS",
                                    )
                                    .to_string()
                                };
                                if crate::ui::chrome::status_tool_button(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Network,
                                    crate::i18n::tr(ctx, "Forward", "转发"),
                                    &fwd_tip,
                                )
                                .clicked()
                                {
                                    if self.show_port_forward_panel {
                                        self.show_port_forward_panel = false;
                                        self.port_forward_last_tab = None;
                                    } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                        self.toggle_port_forward_panel(ctx);
                                    }
                                }
                                if crate::ui::chrome::status_tool_button(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Monitor,
                                    crate::i18n::tr(ctx, "Monitor", "监控"),
                                    menu.monitor_panel,
                                )
                                .clicked()
                                {
                                    if self.show_monitor_panel {
                                        self.show_monitor_panel = false;
                                        self.monitor_last_tab = None;
                                    } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                        self.show_monitor_panel = true;
                                        self.sync_monitor_panel_to_active_tab();
                                        self.monitor_last_tab = self.active_tab;
                                    }
                                }
                                if crate::ui::chrome::status_tool_button(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Api,
                                    "AI",
                                    menu.ai_panel,
                                )
                                .clicked()
                                {
                                    self.toggle_ai_panel(ctx);
                                }
                            },
                        );
                    },
                );
            });
    }

    #[cfg(target_os = "macos")]
    fn poll_native_menu_bar(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.native_menu.is_none() {
            let names: Vec<String> = self
                .theme_manager
                .list_themes()
                .iter()
                .map(|t| {
                    crate::i18n::theme_display_name(ctx, &t.name).into_owned()
                })
                .collect();
            let stored: Vec<String> = self
                .theme_manager
                .list_themes()
                .iter()
                .map(|t| t.name.clone())
                .collect();
            self.native_menu = crate::platform::macos_menu::NativeAppMenu::install(
                &names,
                &stored,
                self.app_settings.ui_language,
            )
            .ok();
        }
        if let Some(menu) = &mut self.native_menu {
            menu.sync(
                ctx,
                self.app_settings.ui_language,
                self.ssh_config_path.exists(),
                self.sidebar_collapsed,
                frame.info().window_info.maximized,
                self.show_sftp_panel,
                self.show_fragment_panel,
                self.show_monitor_panel,
                self.theme_manager.current,
            );
            let actions = menu.drain_actions();
            for action in actions {
                self.handle_mac_menu_action(action, ctx, frame);
            }
        }
        // 系统会反复把首项改回可执行文件名 mistterm，每帧纠正为 Mist
        crate::platform::fix_menu_bar_application_title();
    }

    #[cfg(target_os = "macos")]
    fn handle_mac_menu_action(
        &mut self,
        action: crate::platform::macos_menu::MacMenuAction,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
    ) {
        use crate::platform::macos_menu::MacMenuAction;
        match action {
            MacMenuAction::ImportSsh => self.open_ssh_import_dialog(ctx),
            MacMenuAction::NewSession => self.show_new_session_dialog = true,
            MacMenuAction::NewTab => self.open_new_tab_from_selection(ctx),
            MacMenuAction::Preferences => self.show_preferences_dialog = true,
            MacMenuAction::CloseTab => self.request_close_active_tab(),
            MacMenuAction::DisconnectSsh => self.disconnect_ssh_keep_buffer_active(ctx),
            MacMenuAction::ReconnectTab => self.reconnect_active_tab(ctx),
            MacMenuAction::Quit => frame.close(),
            MacMenuAction::CopyTerminal => self.menu_copy_for_context(ctx),
            MacMenuAction::PasteToTerminal => self.menu_paste_for_context(ctx),
            MacMenuAction::SelectAllTerminal => self.menu_select_all_for_context(ctx),
            MacMenuAction::ToggleSidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                if self.sidebar_collapsed {
                    self.sidebar_user_dismissed_responsive = true;
                } else {
                    self.sidebar_user_dismissed_responsive = false;
                }
            }
            MacMenuAction::ToggleMaximize => {
                frame.set_maximized(!frame.info().window_info.maximized);
            }
            MacMenuAction::TerminalSearch => self.toggle_terminal_search(),
            MacMenuAction::ToggleSftp => self.toggle_sftp_panel(ctx),
            MacMenuAction::ToggleFragmentSidebar => self.toggle_fragment_sidebar(ctx),
            MacMenuAction::ToggleMonitorPanel => self.toggle_monitor_panel(ctx),
            MacMenuAction::CommandHistory => self.menu_open_command_history(ctx),
            MacMenuAction::BatchExec => self.menu_open_batch_exec(ctx),
            MacMenuAction::SessionLogBrowser => self.menu_open_session_log_browser(ctx),
            MacMenuAction::Theme(i) => {
                if i < self.theme_manager.list_themes().len() {
                    self.theme_manager.set_theme_index(i);
                    self.theme_manager.save();
                    ctx.request_repaint();
                }
            }
            MacMenuAction::FragmentLibrary => self.fragment_library.open = true,
            MacMenuAction::QuickFragmentSelector => self.quick_selector.open = true,
            MacMenuAction::CredentialPanel => {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.credential_panel.open = true;
                }
            }
            MacMenuAction::TeamAccount => {
                self.show_preferences_dialog = true;
            }
            MacMenuAction::TeamMembers => {
                self.team_members_dialog.open(&mut self.team_service);
            }
            MacMenuAction::CloudSync => {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.cloud_sync_panel.open = true;
                }
            }
            MacMenuAction::HelpUserGuide => {
                self.help_docs_dialog.open_page(HelpPage::QuickStart);
            }
            MacMenuAction::HelpOnlineDocs => {
                if !crate::platform::open_url(crate::platform::DOCS_INDEX_URL) {
                    self.status_message = crate::i18n::tr(
                        ctx,
                        "Failed to open browser",
                        "无法打开浏览器",
                    )
                    .to_string();
                }
            }
            MacMenuAction::HelpShortcuts => {
                self.help_docs_dialog.open_page(HelpPage::Shortcuts);
            }
            MacMenuAction::HelpReportIssue => {
                self.open_report_issue(ctx);
            }
            MacMenuAction::About => self.show_about_dialog = true,
        }
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn uses_native_menu_bar(&self) -> bool {
        self.native_menu.is_some()
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn uses_native_menu_bar(&self) -> bool {
        false
    }

    fn apply_credential_to_new_session_form(&mut self, ctx: &egui::Context, c: Credential) {
        self.audit_logger.record(
            AuditEvent::new(
                AuditCategory::Credential,
                "credential.use_for_connect",
                AuditOutcome::Success,
            )
            .with_resource(&c.id)
            .with_host(&c.host),
        );
        self.show_new_session_dialog = true;
        self.new_session_name = if c.name.is_empty() {
            c.host.clone()
        } else {
            c.name.clone()
        };
        self.new_session_host = c.host.clone();
        self.new_session_port = c.port.max(1);
        self.new_session_port_str = self.new_session_port.to_string();
        self.new_session_username = c.username.clone();
        self.new_session_vault = VaultSecretForm::from_backend(
            &c.secret_backend,
            &self.app_settings.vault.default_mount,
        );
        match c.auth {
            CredentialAuthKind::Password | CredentialAuthKind::Token => {
                self.new_session_password = if c.secret_backend.is_vault() {
                    String::new()
                } else {
                    c.secret.clone()
                };
                self.new_session_private_key_path.clear();
            }
            CredentialAuthKind::SshKey => {
                self.new_session_password.clear();
                if c.secret.contains("BEGIN") {
                    self.new_session_private_key_path.clear();
                } else {
                    self.new_session_private_key_path = c.secret.clone();
                }
            }
        }
        self.status_message = crate::i18n::tr(
            ctx,
            "Credential prefilled into new session — review before connecting.",
            "已从凭证填入新建会话（请检查后连接）",
        )
        .to_string();
    }
}

impl eframe::App for MistTermApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let p = MistTermUiPersist {
            sidebar_width: self.sidebar_width,
            sidebar_collapsed: self.sidebar_collapsed,
            sidebar_user_dismissed_responsive: self.sidebar_user_dismissed_responsive,
            auto_reconnect_enabled: self.auto_reconnect_enabled,
            session_sort_by: self.session_sort_by,
            session_log_enabled: self.session_log_enabled,
            default_keepalive_enabled: self.default_keepalive_enabled,
            default_keepalive_interval_secs: self.default_keepalive_interval_secs,
            default_keepalive_count_max: self.default_keepalive_count_max,
            session_log_retention_days: self.session_log_settings.retention_days,
            session_log_include_ansi: self.session_log_settings.include_ansi,
            ssh_import_banner_dismissed: self.ssh_import_banner_dismissed,
        };
        eframe::set_value(storage, MISTTERM_UI_STORAGE_KEY, &p);
        let _ = self.command_history.save();
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        crate::i18n::set_language(ctx, self.app_settings.ui_language);
        crate::ui::icons::UiIcons::reload_if_ppp_changed(ctx);
        self.apply_current_theme(ctx);
        self.apply_responsive_layout(ctx);
        self.poll_market_catalog_refresh(ctx);
        self.poll_market_catalog_debounce();

        #[cfg(target_os = "macos")]
        self.poll_native_menu_bar(ctx, frame);

        // ⌘⇧J / Ctrl+⇧J：快速片段选择器（FUNCTIONAL_SPEC §7：⌘J 为连接搜索）
        if !self.global_shortcuts_blocked()
            && ctx.input(|i| {
                i.modifiers.shift
                    && i.key_pressed(egui::Key::J)
                    && (i.modifiers.command || i.modifiers.ctrl)
            })
        {
            self.quick_selector.open = true;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F))
            || ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::F))
        {
            self.toggle_terminal_search();
        }
        if self.show_terminal_search {
            let step = ctx.input(|i| {
                if i.key_pressed(egui::Key::F3) {
                    Some(if i.modifiers.shift { -1 } else { 1 })
                } else {
                    None
                }
            });
            if let Some(delta) = step {
                self.terminal_search_step(delta);
            }
        }

        let theme = self.theme_manager.current_theme().clone();

        // 监控：`exec` 由 shell 泵串行执行，在此处轮询结果并驱动自动刷新
        self.monitor_panel.update(ctx, self.show_monitor_panel);
        self.poll_team_service(ctx);
        self.poll_port_forward_panel();
        show_team_fragment_editor_modal(
            ctx,
            &theme,
            &mut self.team_service,
            &mut self.team_fragment_editor,
            &mut self.team_fragment_conflict,
            &self.audit_logger,
        );
        show_team_fragment_conflict_modal(
            ctx,
            &theme,
            &mut self.team_service,
            &mut self.team_fragment_conflict,
            &self.audit_logger,
        );
        let mut analytics_action =
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::None;
        crate::ui::fragment_analytics_dialog::show_fragment_analytics_modal(
            ctx,
            &theme,
            &mut self.show_fragment_analytics_dialog,
            &mut self.fragment_analytics_range,
            &self.fragment_analytics_snapshot,
            &self.fragment_recommendations,
            &mut analytics_action,
        );
        match analytics_action {
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::Refresh => {
                self.refresh_fragment_analytics_dashboard();
            }
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::ExportJson => {
                self.export_fragment_analytics_json(ctx);
            }
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::ExportEfficiencyReport => {
                self.export_efficiency_report(ctx);
            }
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::ExportEfficiencyReportPdf => {
                self.export_efficiency_report_pdf(ctx);
            }
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::AddRecommendation(i) => {
                self.add_fragment_from_recommendation(ctx, i);
            }
            crate::ui::fragment_analytics_dialog::FragmentAnalyticsUiAction::None => {}
        }
        let batch_targets = self.build_batch_targets(
            ctx,
            self.batch_exec_dialog.include_team_servers,
        );
        let batch_action = self.batch_exec_dialog.show_modal(
            ctx,
            &theme,
            &batch_targets,
            self.batch_exec_rx.as_ref(),
        );
        match batch_action {
            BatchExecUiAction::Run => self.start_batch_exec(ctx),
            BatchExecUiAction::CopyResults => {
                let text = crate::core::batch_exec::format_batch_results_for_clipboard(
                    &self.batch_exec_dialog.results,
                );
                ctx.copy_text(text);
            }
            BatchExecUiAction::None => {}
        }
        self.team_members_dialog.show_modal(ctx, &theme, &mut self.team_service);
        self.try_flush_pending_fragment_insert(ctx);
        if self.command_history.poll_background_load() {
            ctx.request_repaint();
        }
        self.poll_command_history_from_active_tab();
        self.poll_connect_audit_from_tabs();
        self.poll_session_log_commands();
        self.append_terminal_output_logs();

        if let Some(ti) = self.active_tab {
            if let Some(pane) = self.tabs.get_mut(ti).and_then(|t| t.active_pane_mut()) {
                for p in pane.terminal.take_drop_upload_paths() {
                    self.enqueue_upload_for_active_tab(ctx, p);
                }
            }
        }

        let now = Instant::now();
        use crate::core::{
            schedule_after_unexpected_disconnect, TabReconnectSchedule,
            DEFAULT_MAX_RECONNECT_ATTEMPTS,
        };
        let schedules: Vec<TabReconnectSchedule> = self
            .tabs
            .iter()
            .map(|t| {
                let p = t.active_pane();
                TabReconnectSchedule {
                    next_fire: p.and_then(|p| p.ssh_auto_reconnect_next),
                    attempts: p.map(|p| p.ssh_auto_reconnect_attempts).unwrap_or(0),
                }
            })
            .collect();
        let due: Vec<usize> = schedules
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if !self.tab_auto_reconnect_enabled(&self.tabs[i].primary_session_id()) {
                    return None;
                }
                s.next_fire.filter(|t| now >= *t).map(|_| i)
            })
            .collect();
        for i in due {
            if let Some(pane) = self.tabs.get_mut(i).and_then(|t| t.active_pane_mut()) {
                pane.ssh_auto_reconnect_next = None;
            }
            self.reconnect_tab_at(ctx, i);
        }
        for i in 0..self.tabs.len() {
            let sid = self.tabs[i].primary_session_id();
            if !self.tab_auto_reconnect_enabled(&sid) {
                if let Some(pane) = self.tabs.get_mut(i).and_then(|t| t.active_pane_mut()) {
                    let _ = pane.terminal.take_unexpected_disconnect_notified();
                }
                continue;
            }
            let notified = self
                .tabs
                .get_mut(i)
                .and_then(|t| t.active_pane_mut())
                .map(|p| p.terminal.take_unexpected_disconnect_notified());
            if notified == Some(true) {
                let pane = self.tabs[i].active_pane();
                let sched = TabReconnectSchedule {
                    next_fire: pane.and_then(|p| p.ssh_auto_reconnect_next),
                    attempts: pane.map(|p| p.ssh_auto_reconnect_attempts).unwrap_or(0),
                };
                let (new_sched, status) = schedule_after_unexpected_disconnect(
                    sched,
                    DEFAULT_MAX_RECONNECT_ATTEMPTS,
                    now,
                );
                if let Some(pane) = self.tabs.get_mut(i).and_then(|t| t.active_pane_mut()) {
                    pane.ssh_auto_reconnect_next = new_sched.next_fire;
                    pane.ssh_auto_reconnect_attempts = new_sched.attempts;
                }
                if let Some(s) = status {
                    self.status_message = Self::format_reconnect_status(ctx, s);
                }
            }
        }

        // FUNCTIONAL_SPEC §2.4：非当前标签/窗格仍消费 SSH 输出。
        let active_tab = self.active_tab;
        let mut inactive_tab_vte_dirty = false;
        for (ti, tab) in self.tabs.iter_mut().enumerate() {
            for (pi, pane) in tab.panes.iter_mut().enumerate() {
                let focused =
                    active_tab == Some(ti) && tab.active_pane == pi;
                if !focused && pane.terminal.pump_ssh_only(&theme) {
                    inactive_tab_vte_dirty = true;
                }
            }
        }
        if inactive_tab_vte_dirty {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        // SCP 直传结果（`TerminalView::start_upload` 后台线程）
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
            if let Some(res) = pane.terminal.poll_upload_result() {
                match res {
                    Ok(path) => {
                        self.status_message = format!(
                            "{}{}",
                            crate::i18n::tr(ctx, "File upload finished: ", "文件上传完成："),
                            path
                        );
                    }
                    Err(e) => {
                        self.status_message = status_message_wrap_error(format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "File upload failed: ", "文件上传失败："),
                            e
                        ));
                    }
                }
                break;
            }
            }
        }

        // 检查是否有终端等待 rz 上传文件（ZMODEM：`start_rz_upload`，非 SCP `start_upload`）
        if let Some(terminal) = self.current_terminal() {
            if terminal.pending_rz_upload {
                if let Some(t) = self.current_terminal_mut() {
                    t.pending_rz_upload = false;
                }
                if let Some(path) = FileDialog::new()
                    .set_title(crate::i18n::tr(
                        ctx,
                        "Choose file for remote upload (rz)",
                        "选择要上传到远端（rz）的文件",
                    ))
                    .pick_file()
                {
                    self.status_message = format!(
                        "{} {}",
                        crate::i18n::tr(ctx, "ZMODEM upload:", "ZMODEM 上传："),
                        path.display()
                    );
                    if let Some(t) = self.current_terminal_mut() {
                        match t.start_rz_upload(path.as_path()) {
                            Ok(()) => {
                                self.status_message = format!(
                                    "{} {}",
                                    crate::i18n::tr(ctx, "ZMODEM started:", "ZMODEM 已启动:"),
                                    path.display()
                                );
                            }
                            Err(e) => {
                                t.end_rz_handshake_capture();
                                self.status_message = status_message_wrap_error(format!(
                                    "{} {}",
                                    crate::i18n::tr(ctx, "ZMODEM launch failed:", "ZMODEM 启动失败："),
                                    e
                                ));
                            }
                        }
                    }
                } else {
                    self.status_message = crate::i18n::tr(ctx, "rz upload cancelled", "rz 上传已取消")
                        .to_string();
                    if let Some(t) = self.current_terminal_mut() {
                        t.end_rz_handshake_capture();
                        t.clear_rz_control_mode();
                    }
                }
            }
        }

        if !self.global_shortcuts_blocked() {
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::N)) {
                self.show_new_session_dialog = true;
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::T)) {
                self.open_new_tab_from_selection(ctx);
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::J)) {
                self.focus_sidebar_connection_search(ctx);
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::K)) {
                self.focus_fragment_panel_search(ctx);
            }
            if ctx.input(|i| {
                Self::input_primary_mod(i) && i.modifiers.shift && i.key_pressed(egui::Key::A)
            }) {
                self.toggle_ai_panel(ctx);
            }
            if ctx.input(|i| {
                Self::input_primary_mod(i) && i.modifiers.shift && i.key_pressed(egui::Key::L)
            }) {
                self.send_terminal_selection_to_ai(ctx);
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::W)) {
                self.request_close_active_tab();
            }
            if ctx.input(|i| {
                i.modifiers.alt
                    && !i.modifiers.ctrl
                    && !i.modifiers.command
                    && !i.modifiers.shift
                    && (i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowRight))
            }) {
                if let Some(idx) = self.active_tab {
                    if self.tabs.get(idx).is_some_and(|t| t.is_split()) {
                        self.tabs[idx].cycle_active_pane();
                        ctx.request_repaint();
                    }
                }
            }
            // 分屏：⌘⇧D / Ctrl+Shift+D 左右，⌘⇧U / Ctrl+Shift+U 上下
            if ctx.input(|i| {
                Self::input_primary_mod(i)
                    && i.modifiers.shift
                    && i.key_pressed(egui::Key::D)
            }) {
                if let Some(idx) = self.active_tab {
                    self.split_tab_at(ctx, idx, crate::ui::tab_pane::TabLayout::SplitHorizontal);
                }
            }
            if ctx.input(|i| {
                Self::input_primary_mod(i)
                    && i.modifiers.shift
                    && i.key_pressed(egui::Key::U)
            }) {
                if let Some(idx) = self.active_tab {
                    self.split_tab_at(ctx, idx, crate::ui::tab_pane::TabLayout::SplitVertical);
                }
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::E)) {
                if let Some(ref sid) = self.selected_session_id.clone() {
                    self.open_edit_session_dialog(sid);
                } else {
                    let accel = crate::platform::accel("E");
                    self.status_message = match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => format!(
                            "Select a connection on the left first ({accel} edits the profile)."
                        ),
                        crate::i18n::UiLanguage::Zh => format!(
                            "请先在左侧选择一个连接（{} 编辑会话配置）",
                            accel,
                        ),
                    };
                }
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::H)) {
                self.show_about_dialog = true;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::R) && i.modifiers.ctrl && !i.modifiers.command) {
                if self.current_terminal().map(|t| t.is_connected()).unwrap_or(false) {
                    if self.command_history_overlay.open {
                        let n = self
                            .command_history
                            .search(&self.command_history_overlay.query, true)
                            .len();
                        self.command_history_overlay.cycle_match(n);
                    } else {
                        self.command_history_overlay.open_new();
                    }
                } else {
                    let a = crate::platform::terminal_history_accel();
                    self.status_message = match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => format!(
                            "Connect first, then use {a} to search command history",
                        ),
                        crate::i18n::UiLanguage::Zh => {
                            format!("请先连接终端后再使用 {} 搜索命令历史", a)
                        }
                    };
                }
            }
            // egui 0.23 无 Key::Comma；⌘/Ctrl+, 常表现为 Text(",") + 主修饰键
            let prefs_shortcut = ctx.input_mut(|i| {
                if !Self::input_primary_mod(i) {
                    return false;
                }
                let mut hit = false;
                i.events.retain(|e| {
                    if let egui::Event::Text(t) = e {
                        if t.as_str() == "," {
                            hit = true;
                            return false;
                        }
                    }
                    true
                });
                hit
            });
            if prefs_shortcut {
                self.show_preferences_dialog = true;
            }
            let primary_tab_cycle = ctx.input(|i| {
                if Self::input_primary_mod(i) && i.key_pressed(egui::Key::Tab) {
                    Some(i.modifiers.shift)
                } else {
                    None
                }
            });
            match primary_tab_cycle {
                Some(true) => self.switch_to_prev_tab(),
                Some(false) => self.switch_to_next_tab(),
                None => {}
            }
            for n in 1u8..=9u8 {
                let key = match n {
                    1 => egui::Key::Num1,
                    2 => egui::Key::Num2,
                    3 => egui::Key::Num3,
                    4 => egui::Key::Num4,
                    5 => egui::Key::Num5,
                    6 => egui::Key::Num6,
                    7 => egui::Key::Num7,
                    8 => egui::Key::Num8,
                    9 => egui::Key::Num9,
                    _ => continue,
                };
                if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(key)) {
                    let idx = (n - 1) as usize;
                    self.switch_tab_to_index(idx);
                    break;
                }
            }
        }

        let terminal_wants_delete_for_pty = self
            .active_tab
            .and_then(|i| self.tabs.get(i))
            .and_then(|t| t.active_terminal().map(|term| term.is_terminal_focused()))
            .unwrap_or(false);

        if !self.global_shortcuts_blocked()
            && !terminal_wants_delete_for_pty
            && self.delete_session_confirm.is_none()
            && ctx.input(|i| {
                i.key_pressed(egui::Key::Delete)
                    && !i.modifiers.command
                    && !i.modifiers.ctrl
                    && !i.modifiers.alt
            })
        {
            if let Some(ref sid) = self.selected_session_id.clone() {
                if let Some(s) = self.session_manager.get_session(sid) {
                    self.delete_session_confirm = Some((sid.clone(), s.name.clone()));
                }
            }
        }

        self.render_workspace_shell(ctx, frame, &theme);
        self.process_ai_bridge(ctx);
    }
}

impl MistTermApp {
    /// 执行命令片段（⌘J 快速选择）：会话占位符展开；片段库变量与 `<自定义>` 占位符弹窗填写。
    fn execute_fragment(&mut self, ctx: &egui::Context, fragment: &FragmentStats) {
        if self.selected_session_id.is_none() {
            self.status_message =
                crate::i18n::tr(ctx, "Select a session on the left first", "请先选择左侧会话")
                    .to_string();
            return;
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Fragment, "fragment.execute", AuditOutcome::Success)
                .with_resource(&fragment.id)
                .with_detail(serde_json::json!({ "title": fragment.title })),
        );

        if fragment.has_variables() {
            self.variable_dialog.open = true;
            self.variable_dialog.fragment_id = Some(fragment.id.clone());
            self.variable_dialog.fragment_title = fragment.title.clone();
            self.variable_dialog.values = fragment.variable_defaults();
            self.variable_dialog.command_edit =
                self.build_fragment_command_preview(fragment, &self.variable_dialog.values);
            self.variable_dialog.paste_after_fill = false;
            self.variable_dialog.last_finalize_error = None;
            return;
        }

        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        let rhai_ctx = merge_rhai_context(session, &HashMap::new());
        let after_rhai = match expand_rhai_blocks(&fragment.command, &rhai_ctx) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = status_message_wrap_error(crate::i18n::localize_fragment_expr_error(
                    crate::i18n::language(ctx),
                    &e,
                ));
                return;
            }
        };
        let expanded = expand_command_template(
            &after_rhai,
            session,
            &std::collections::HashMap::new(),
        );
        let needs = placeholders_needing_user(&expanded);

        if needs.is_empty() {
            let start = std::time::Instant::now();
            if let Some(session_id) = &self.selected_session_id {
                let idx = self
                    .active_tab
                    .filter(|&i| {
                        i < self.tabs.len() && self.tabs[i].primary_session_id() == *session_id
                    })
                    .or_else(|| {
                        self
                            .tabs
                            .iter()
                            .position(|t| t.primary_session_id() == *session_id)
                    });
                if let Some(idx) = idx {
                    if self
                        .tabs[idx]
                        .active_terminal()
                        .map(|t| t.is_connected())
                        .unwrap_or(false)
                    {
                        if self.send_audited_command_at(ctx, idx, &expanded)
                            != CommandSendResult::Sent
                        {
                            return;
                        }
                        let dur_ms = start.elapsed().as_millis().max(1) as u64;
                        self.record_fragment_execution(fragment.id.as_str(), true, dur_ms);
                        self.status_message = format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "Executed snippet:", "已执行片段："),
                            fragment.title
                        );
                    } else {
                        self.insert_fragment_at_tab_index(
                            ctx,
                            idx,
                            Some(fragment.id.as_str()),
                            &expanded,
                        );
                    }
                } else {
                    self.status_message = crate::i18n::tr(
                        ctx,
                        "Open a terminal tab for this session",
                        "请为当前会话打开终端标签",
                    )
                    .to_string();
                }
            }
            self.quick_selector.open = false;
            return;
        }

        self.pending_fragment_id = Some(fragment.id.clone());
        self.pending_fragment_name = fragment.title.clone();
        self.pending_fragment_command = expanded;
        self.pending_fragment_vars = needs
            .into_iter()
            .map(|k| (k, String::new()))
            .collect();
        self.sync_pending_fragment_command_edit();
        self.fragment_vars_completion = FragmentVarsCompletion::QuickExecuteSend;
        self.show_fragment_vars_dialog = true;
    }

    /// 显示欢迎界面
    fn show_welcome(&self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let icon_px = self.theme_manager.current_theme().size_icon_glyph();
        let s = ui.available_size();
        if s.x.is_finite() && s.y.is_finite() && s.x > 0.0 && s.y > 0.0 {
            ui.set_min_size(s);
        }
        ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            ui.heading(crate::i18n::tr(&ctx, "Welcome to Mist", "欢迎使用 Mist"));
            ui.separator();
            let accent = ui.style().visuals.selection.bg_fill;
            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Rocket,
                crate::i18n::tr(&ctx, "Quick start", "快速开始"),
                icon_px,
                8.0,
                move |t| t.color(accent),
            );
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(&ctx, "1. Click on the sidebar", "1. 点击左侧"));
                let px = icon_px;
                let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                crate::ui::icons::paint_icon(
                    ui,
                    r,
                    crate::ui::icons::IconId::Plus,
                    ui.visuals().text_color(),
                    px,
                );
                ui.label(crate::i18n::tr(&ctx, "to create a session", "创建新会话"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(&ctx, "2. Select a session", "2. 选择会话"));
                let px = icon_px;
                let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                crate::ui::icons::paint_icon(
                    ui,
                    r,
                    crate::ui::icons::IconId::Plug,
                    ui.visuals().text_color(),
                    px,
                );
                ui.label(crate::i18n::tr(&ctx, "and connect", "建立连接"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(&ctx, "3. Use", "3. 使用"));
                ui.label("rz/sz");
                ui.label(crate::i18n::tr(&ctx, "for file transfer", "进行文件传输"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(
                    &ctx,
                    "Custom snippets: Tools → Fragment Library, or New in the right sidebar",
                    "自建命令片段：菜单「工具 → 命令片段库」或右侧栏「新建」",
                ));
            });
            ui.separator();
            ui.small(crate::i18n::tr(
                &ctx,
                "Tip: double-click the sidebar to collapse or expand",
                "提示：双击侧边栏可以折叠/展开",
            ));
        });
    }
}

/// 主窗口布局 shell（`docs/product/LAYOUT.md`）
#[path = "workspace.rs"]
mod workspace;

#[path = "preferences_dialog.rs"]
mod preferences_dialog;

/// 应用菜单（终端 / 编辑 / 视图 / 工具 / 帮助）— 子模块可访问 `MistTermApp` 私有字段
mod menu {
    use super::*;

    impl MistTermApp {
        pub(crate) fn show_application_menu_bar(
            &mut self,
            ui: &mut egui::Ui,
            ctx: &egui::Context,
            theme: &crate::ui::theme::Theme,
            frame: &mut eframe::Frame,
        ) {
            if self.uses_native_menu_bar() {
                return;
            }
            let label = |text: &str| {
                egui::RichText::new(text)
                    .size(theme.font_size_menu_item())
                    .color(theme.text_secondary())
            };
            let ssh_import_enabled = self.ssh_config_path.exists();
            let l = crate::i18n::menu::labels(crate::i18n::language(ctx));

            egui::menu::menu_button(ui, label(l.terminal_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if crate::ui::chrome::popup_menu_button(ui, theme, l.new_session).clicked()
                {
                    self.show_new_session_dialog = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.new_tab).clicked()
                {
                    self.open_new_tab_from_selection(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button_enabled(
                    ui,
                    theme,
                    l.import_ssh,
                    ssh_import_enabled,
                )
                .clicked()
                {
                    self.open_ssh_import_dialog(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.close_tab).clicked()
                {
                    self.request_close_active_tab();
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.disconnect).clicked()
                {
                    self.disconnect_ssh_keep_buffer_active(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.reconnect).clicked() {
                    self.reconnect_active_tab(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.preferences).clicked()
                {
                    self.show_preferences_dialog = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Quit", "退出"),
                )
                .clicked()
                {
                    frame.close();
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label(l.edit_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if crate::ui::chrome::popup_menu_button(ui, theme, l.copy).clicked()
                {
                    self.menu_copy_for_context(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.paste).clicked()
                {
                    self.menu_paste_for_context(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.select_all).clicked()
                {
                    self.menu_select_all_for_context(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.find_in_terminal).clicked()
                {
                    self.toggle_terminal_search();
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label(l.view_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if crate::ui::chrome::popup_menu_button(
                    ui,
                    theme,
                    if self.sidebar_collapsed {
                        l.expand_sidebar
                    } else {
                        l.collapse_sidebar
                    },
                )
                .clicked()
                {
                    self.sidebar_collapsed = !self.sidebar_collapsed;
                    self.sidebar_user_dismissed_responsive = self.sidebar_collapsed;
                    ui.close_menu();
                }
                let maximized = frame.info().window_info.maximized;
                if crate::ui::chrome::popup_menu_button(
                    ui,
                    theme,
                    if maximized {
                        l.restore_window
                    } else {
                        l.maximize_window
                    },
                )
                .clicked()
                {
                    frame.set_maximized(!maximized);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_sftp_panel,
                    l.sftp_panel,
                )
                .clicked()
                {
                    self.toggle_sftp_panel(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_port_forward_panel,
                    crate::i18n::tr(ctx, "Port Forwarding", "端口转发"),
                )
                .clicked()
                {
                    self.toggle_port_forward_panel(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_fragment_panel,
                    l.fragment_panel,
                )
                .clicked()
                {
                    self.toggle_fragment_sidebar(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_monitor_panel,
                    l.monitor_panel,
                )
                .clicked()
                {
                    self.toggle_monitor_panel(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_ai_panel,
                    l.ai_panel,
                )
                .clicked()
                {
                    self.toggle_ai_panel(ctx);
                    ui.close_menu();
                }
                ui.separator();
                ui.menu_button(label(l.theme_menu), |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    let current_idx = self.theme_manager.current;
                    let theme_labels: Vec<String> = self
                        .theme_manager
                        .list_themes()
                        .iter()
                        .map(|t| crate::i18n::theme_display_name(ctx, &t.name).into_owned())
                        .collect();
                    for (i, label) in theme_labels.iter().enumerate() {
                        let selected = i == current_idx;
                        if crate::ui::chrome::menu_theme_item(ui, theme, selected, label)
                            .clicked()
                        {
                            self.theme_manager.set_theme_index(i);
                            self.theme_manager.save();
                            ui.ctx().request_repaint();
                            ui.close_menu();
                        }
                    }
                });
            });
            egui::menu::menu_button(ui, label(l.tools_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if crate::ui::chrome::popup_menu_button(ui, theme, l.ai_settings).clicked() {
                    self.show_ai_settings_dialog = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.fragment_library).clicked() {
                    self.fragment_library.open = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.quick_fragments).clicked()
                {
                    self.quick_selector.open = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.command_history).clicked()
                {
                    self.menu_open_command_history(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.batch_exec).clicked() {
                    self.menu_open_batch_exec(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.credentials).clicked() {
                    if self.ensure_right_dock_allowed_or_warn(ctx) {
                        self.credential_panel.open = true;
                    }
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.team_account).clicked() {
                    self.show_preferences_dialog = true;
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.cloud_sync).clicked() {
                    if self.ensure_right_dock_allowed_or_warn(ctx) {
                        self.cloud_sync_panel.open = true;
                    }
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.session_logs).clicked() {
                    self.menu_open_session_log_browser(ctx);
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label(l.help_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if crate::ui::chrome::popup_menu_button(ui, theme, l.help_guide).clicked() {
                    self.help_docs_dialog.open_page(HelpPage::QuickStart);
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.help_shortcuts).clicked() {
                    self.help_docs_dialog.open_page(HelpPage::Shortcuts);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.help_online_docs).clicked() {
                    if !crate::platform::open_url(crate::platform::DOCS_INDEX_URL) {
                        self.status_message = crate::i18n::tr(
                            ctx,
                            "Failed to open browser",
                            "无法打开浏览器",
                        )
                        .to_string();
                    }
                    ui.close_menu();
                }
                if crate::ui::chrome::popup_menu_button(ui, theme, l.help_report_issue).clicked() {
                    self.open_report_issue(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if crate::ui::chrome::popup_menu_button(ui, theme, l.help_about).clicked() {
                    self.show_about_dialog = true;
                    ui.close_menu();
                }
            });
        }
    }
}

fn terminal_command_status_message(ctx: &egui::Context, cmd: &str) -> String {
    use crate::i18n::{UiLanguage, language};
    let lines: Vec<&str> = cmd.lines().filter(|l| !l.trim().is_empty()).collect();
    let first = lines.first().map(|l| l.trim()).unwrap_or("");
    let preview = if first.chars().count() > 56 {
        let head: String = first.chars().take(56).collect();
        format!("{head}…")
    } else {
        first.to_string()
    };
    match language(ctx) {
        UiLanguage::En if lines.len() > 1 => {
            format!("Sent to terminal ({} lines): {preview}", lines.len())
        }
        UiLanguage::En => format!("Sent to terminal: {preview}"),
        UiLanguage::Zh if lines.len() > 1 => {
            format!("已发送到终端（{} 行）：{preview}", lines.len())
        }
        UiLanguage::Zh => format!("已发送到终端：{preview}"),
    }
}

#[cfg(test)]
mod responsive_layout_tests {
    use super::MistTermApp;
    use crate::ui::layout_util::{terminal_column_width, work_area_inner_rect};

    #[test]
    fn work_area_inner_rect_terminal_width_respects_pad() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 800.0));
        let inner = work_area_inner_rect(work, 8.0);
        let col_left = inner.min.x + 200.0;
        let w = terminal_column_width(col_left, inner.max.x, None);
        assert!(col_left + w <= inner.max.x + 0.01);
        assert_eq!(inner.width(), work.width() - 16.0);
    }

    #[test]
    fn right_dock_open_allowed_respects_wide_min() {
        assert!(MistTermApp::right_dock_open_allowed(1200.0));
        assert!(MistTermApp::right_dock_open_allowed(2000.0));
        assert!(!MistTermApp::right_dock_open_allowed(1199.0));
        assert!(!MistTermApp::right_dock_open_allowed(800.0));
        assert!(!MistTermApp::right_dock_open_allowed(f32::NAN));
        assert!(!MistTermApp::right_dock_open_allowed(-1.0));
    }
}
