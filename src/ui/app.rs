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
    AppSettings, AuditCategory, AuditEvent, AuditLogger, AuditOutcome, CommandHistory, Credential,
    CredentialAuthKind, SecretResolver, SessionLogSettings, SessionLogWriter, SecretBackend,
    TempKeyFile, spawn_cleanup_old_logs, DEFAULT_RETENTION_DAYS,
    SessionSortBy, SshConfigCandidate, command_preview, expand_command_template,
    expand_fragment_command_stages, expand_rhai_blocks, list_placeholder_keys, merge_rhai_context,
    FragmentManager, FragmentStats, SessionConfig, SessionManager, SortBy, SESSION_COLOR_TAGS,
};
use crate::ui::command_history_overlay::{CommandHistoryAction, CommandHistoryOverlay};
use crate::ui::help_docs_dialog::{HelpDocsDialog, HelpPage};
use crate::ui::session_log_dialog::SessionLogDialog;
use crate::ui::ssh_config_import_dialog::SshConfigImportDialog;
use crate::ui::sidebar::Sidebar;
use crate::ui::terminal::TerminalView;
use crate::ui::git_sync::GitSyncPanel;
use crate::ui::monitor_panel::MonitorPanel;
use crate::ui::sftp_panel::SftpPanel;
use crate::ui::theme::ThemeManager;
use crate::ui::fragment_library::FragmentLibraryState;
use crate::ui::credential_panel::{CredentialPanel, CredentialPanelAction};
use crate::ui::cloud_sync_panel::{CloudSyncPanel, CloudSyncDeps};
use crate::ui::layout_util;

/// eframe 自定义持久化键（RON）；与 egui 自带的窗口几何持久化并存（FUNCTIONAL_SPEC §8.1）
const MISTTERM_UI_STORAGE_KEY: &str = "mistterm_ui_v1";

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

/// FUNCTIONAL_SPEC §7 快捷键单一真源（关于页与帮助共用；随平台显示 ⌘ 或 Ctrl）。
pub(crate) fn mistterm_functional_spec_shortcuts() -> String {
    use crate::platform::shortcuts as s;
    format!(
        "键盘快捷键（主修饰键：{}）\n\
         {}\n\
         {}\n\
         {}\n\
         {}\n\
         {}1–9 — 切换第 N 个标签\n\
         {}Tab — 下一标签；加 Shift 为上一标签\n\
         {}\n\
         {}\n\
         {}\n\
         {}F — 终端内搜索\n\
         {}, — 偏好设置\n\
         {}H — 关于与本说明\n\
         {} — 命令历史（终端内）",
        s::primary_modifier_label(),
        s::help_line("N", "新建会话"),
        s::help_line("E", "编辑所选会话"),
        s::help_line("T", "新终端标签"),
        s::help_line("W", "关闭当前标签"),
        s::primary_modifier_label(),
        s::primary_modifier_label(),
        s::help_line("J", "聚焦连接搜索"),
        s::help_line("K", "聚焦片段搜索"),
        s::accel_shift("J") + " — 快速片段选择器",
        s::primary_modifier_label(),
        s::primary_modifier_label(),
        s::primary_modifier_label(),
        s::terminal_history_accel(),
    )
}

/// 底栏 / 提示文案颜色：错误类用主题红，其余用弱文字色（避免顶栏大块告警色）
fn status_message_text_color(msg: &str, theme: &crate::ui::theme::Theme) -> egui::Color32 {
    if msg.starts_with("表达式错误")
        || msg.starts_with("插入失败")
        || msg.starts_with("上传失败")
        || msg.starts_with("文件上传失败")
        || (msg.starts_with("ZMODEM") && msg.contains("失败"))
    {
        theme.red_color()
    } else {
        theme.fg_low_color()
    }
}

/// 设计文档 §5.4：`{次数}次 · {成功率}%成功 · {耗时}s`
fn format_fragment_stats_line(frag: &FragmentStats) -> String {
    if frag.usage_count == 0 {
        return "未使用".to_string();
    }
    let rate = (frag.success_count as f32 / frag.usage_count as f32) * 100.0;
    let avg_s = frag.total_time_ms as f64 / frag.usage_count as f64 / 1000.0;
    format!(
        "{}次 · {:.0}%成功 · {:.1}s",
        frag.usage_count, rate, avg_s
    )
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
            !SESSION_PLACEHOLDER_KEYS
                .iter()
                .any(|&sk| sk == k.as_str())
        })
        .collect()
}

struct TerminalTab {
    session_id: String,
    title: String,
    terminal: TerminalView,
    /// 自动重连：计划执行时间；与 `ssh_auto_reconnect_attempts` 配合（§1.4 可选行为）
    ssh_auto_reconnect_next: Option<Instant>,
    ssh_auto_reconnect_attempts: u8,
    /// Vault/内存 PEM 临时私钥，断开或关闭标签时删除
    ssh_temp_key: Option<TempKeyFile>,
    log_writer: Option<SessionLogWriter>,
    last_term_rect: egui::Rect,
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
    /// 上一帧的 §8 布局档位；变化时写入 `status_message` 便于察觉
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
    show_git_sync_panel: bool,  // Git 同步面板
    show_monitor_panel: bool,   // 监控面板
    /// 终端视口搜索（当前屏缓冲，不含卷动历史）
    show_terminal_search: bool,
    /// 打开查找条后首帧聚焦输入框
    terminal_search_pending_focus: bool,
    terminal_search_query: String,
    terminal_search_ignore_case: bool,
    terminal_search_hits: Vec<crate::terminal::SearchHit>,
    terminal_search_cur: usize,
    show_sftp_panel: bool,       // SFTP 文件浏览器
    /// 上次已同步 SFTP 列表的终端标签索引（切换标签时重置远端浏览状态）
    sftp_last_tab: Option<usize>,
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
    new_session_private_key_path: String,
    new_session_secret_backend: SecretBackend,

    edit_session_id: Option<String>,
    edit_session_name: String,
    edit_session_host: String,
    edit_session_port: u16,
    edit_session_port_str: String,
    edit_session_username: String,
    edit_session_password: String,
    edit_session_group: String,
    edit_session_private_key_path: String,
    edit_session_color_tag: String,
    edit_session_keepalive_enabled: bool,
    edit_session_keepalive_interval_secs: u32,
    edit_session_keepalive_count_max: u8,
    edit_session_keepalive_auto_reconnect: bool,
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

    git_sync_panel: GitSyncPanel,
    monitor_panel: MonitorPanel,
    sftp_panel: SftpPanel,
    fragment_library: FragmentLibraryState,
    credential_panel: CredentialPanel,
    cloud_sync_panel: CloudSyncPanel,

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
    help_docs_dialog: HelpDocsDialog,
    session_log_enabled: bool,
    default_keepalive_enabled: bool,
    default_keepalive_interval_secs: u32,
    default_keepalive_count_max: u8,

    /// FUNCTIONAL_SPEC §1.3.4：Delete 删除会话前的确认 `(session_id, display_name)`
    delete_session_confirm: Option<(String, String)>,
    /// §2.3.5：关闭仍连接/握手中的标签前确认
    close_tab_confirm_idx: Option<usize>,
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
            egui::Id::new(id),
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
            egui::Id::new(id),
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
        self.show_git_sync_panel = false;
        self.show_monitor_panel = false;
        self.show_sftp_panel = false;
        self.credential_panel.open = false;
        self.cloud_sync_panel.open = false;
        self.monitor_last_tab = None;
        self.sftp_last_tab = None;
    }

    /// FUNCTIONAL_SPEC §8.2：按窗口宽度收折左栏与右侧 dock
    fn apply_responsive_layout(&mut self, ctx: &egui::Context) {
        let w = Self::layout_window_width(ctx);
        let Some(band) = Self::layout_band_from_width(w) else {
            return;
        };
        let prev = self.last_responsive_layout_band;
        let band_changed = prev != Some(band);

        if w < Self::RESP_LAYOUT_NARROW_LT_PX {
            self.sidebar_collapsed = true;
            self.close_all_right_dock_panels();
        } else if w < Self::RESP_LAYOUT_WIDE_MIN_PX {
            self.close_all_right_dock_panels();
        } else if !self.sidebar_user_dismissed_responsive {
            self.sidebar_collapsed = false;
        }

        if band_changed {
            self.last_responsive_layout_band = Some(band);
            let w_txt = format!("{:.0}", w);
            self.status_message = match band {
                ResponsiveLayoutBand::Narrow => format!(
                    "§8 窄屏（{}px）：已自动收起左侧连接栏与右侧侧栏；拉宽到 ≥800 可恢复左栏",
                    w_txt
                ),
                ResponsiveLayoutBand::Medium => format!(
                    "§8 中宽（{}px）：右侧侧栏已自动关闭；拉宽到 ≥1200 可再打开片段/Git/SFTP 等",
                    w_txt
                ),
                ResponsiveLayoutBand::Wide => {
                    if self.sidebar_user_dismissed_responsive {
                        format!(
                            "§8 宽屏（{}px）：左侧栏因您曾手动折叠而未自动展开；点「展开侧边栏」可恢复",
                            w_txt
                        )
                    } else {
                        format!(
                            "§8 宽屏（{}px）：左侧栏已展开，可打开右侧侧栏",
                            w_txt
                        )
                    }
                }
            };
        }
    }

    /// 打开任意右侧 dock 前调用；不允许时写状态栏并返回 false
    fn ensure_right_dock_allowed_or_warn(&mut self, ctx: &egui::Context) -> bool {
        let w = Self::layout_window_width(ctx);
        if Self::right_dock_open_allowed(w) {
            true
        } else {
            self.status_message = format!(
                "当前窗口约 {:.0}px，§8 要求宽度 ≥ {:.0}px 才能打开右侧侧栏；请先拉宽窗口",
                w,
                Self::RESP_LAYOUT_WIDE_MIN_PX
            );
            false
        }
    }

    /// 创建新的应用实例
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let app_settings = AppSettings::load();
        let audit_logger = AuditLogger::new(app_settings.audit.clone());
        let mut session_manager = SessionManager::new();
        let boot_diagnostics = session_manager.take_load_diagnostics().join("；");
        let sessions = session_manager.list_sessions();
        
        // 自动选择第一个会话
        let selected_session_id = sessions.first().map(|s| s.id.clone());

        let mut app = Self {
            session_manager,
            fragment_manager: FragmentManager::load(&FragmentManager::default_config_path())
                .unwrap_or_else(|_| FragmentManager::new()),
            selected_session_id,
            sidebar_collapsed: false,
            sidebar_width: layout_util::default_sidebar_width(&cc.egui_ctx),
            sidebar_user_dismissed_responsive: false,
            last_responsive_layout_band: None,
            tabs: Vec::new(),
            active_tab: None,
            status_message: {
                let mut msg = if boot_diagnostics.is_empty() {
                    "就绪".to_string()
                } else {
                    boot_diagnostics
                };
                if !crate::platform::cjk_font_loaded() {
                    let warn = "未加载中文字体，界面中文可能显示为方框";
                    if msg.is_empty() || msg == "就绪" {
                        msg = warn.to_string();
                    } else {
                        msg = format!("{msg}；{warn}");
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
            show_git_sync_panel: false,
            show_monitor_panel: false,
            show_terminal_search: false,
            terminal_search_pending_focus: false,
            terminal_search_query: String::new(),
            terminal_search_ignore_case: true,
            terminal_search_hits: Vec::new(),
            terminal_search_cur: 0,
            show_sftp_panel: false,
            sftp_last_tab: None,
            monitor_last_tab: None,
            git_sync_panel: GitSyncPanel::new(),
            monitor_panel: MonitorPanel::new(),
            sftp_panel: SftpPanel::new(),
            fragment_library: FragmentLibraryState::new(),
            credential_panel: CredentialPanel::new(),
            cloud_sync_panel: CloudSyncPanel::new(),
            pending_fragment_id: None,
            pending_fragment_name: String::new(),
            pending_fragment_command: String::new(),
            pending_fragment_command_edit: String::new(),
            pending_fragment_vars: Vec::new(),
            show_fragment_vars_dialog: false,
            fragment_filter_category: "全部".to_string(),
            pending_fragment_insert: None,
            new_session_name: String::new(),
            new_session_host: String::new(),
            new_session_port: 22,
            new_session_port_str: "22".to_string(),
            new_session_username: String::new(),
            new_session_password: String::new(),
            new_session_group: "默认".to_string(),
            new_session_private_key_path: String::new(),
            new_session_secret_backend: SecretBackend::default(),
            edit_session_id: None,
            edit_session_name: String::new(),
            edit_session_host: String::new(),
            edit_session_port: 22,
            edit_session_port_str: "22".to_string(),
            edit_session_username: String::new(),
            edit_session_password: String::new(),
            edit_session_group: "默认".to_string(),
            edit_session_private_key_path: String::new(),
            edit_session_color_tag: String::new(),
            edit_session_keepalive_enabled: true,
            edit_session_keepalive_interval_secs: 30,
            edit_session_keepalive_count_max: 3,
            edit_session_keepalive_auto_reconnect: true,
            sidebar_search_query: String::new(),
            sidebar_filter: "全部".to_string(),
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

        app
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

    fn open_ssh_import_dialog(&mut self) {
        if !self.ssh_config_path.exists() {
            self.status_message = format!(
                "未找到 SSH 配置文件：{}",
                self.ssh_config_path.display()
            );
            return;
        }
        let parse = parse_ssh_config_file(&self.ssh_config_path).unwrap_or(SshConfigParseResult {
            candidates: Vec::new(),
            warnings: vec!["无法读取 SSH 配置文件".to_string()],
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
            self.status_message = "所有可导入的 SSH 配置已存在".to_string();
        }
        self.ssh_import_dialog.set_candidates(
            parse.candidates,
            already_imported,
            parse.warnings,
        );
    }

    fn import_ssh_indices(&mut self, indices: &[usize]) {
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
            self.status_message = format!("已导入 {} 个 SSH 配置", added);
            self.refresh_ssh_config_candidates();
        }
    }

    fn poll_connect_audit_from_tabs(&mut self) {
        for tab in &mut self.tabs {
            if let Some((ok, host)) = tab.terminal.take_connect_audit() {
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
                        .with_session(&tab.session_id),
                );
            }
        }
    }

    fn terminal_connect_session(
        &mut self,
        terminal: &mut TerminalView,
        session: &SessionConfig,
        temp_key: &mut Option<TempKeyFile>,
    ) {
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "connect.start", AuditOutcome::Success)
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
                self.status_message = format!("解析凭据失败: {e}");
                return;
            }
        };
        *temp_key = resolved.temp_key_file;
        let theme = self.theme_manager.current_theme();
        let (ka_on, ka_int, ka_max) = Self::session_keepalive_params(session);
        terminal.connect(
            theme,
            &session.host,
            session.port,
            &session.username,
            &resolved.password,
            &resolved.private_key_path,
            ka_on,
            ka_int,
            ka_max,
        );
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
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        if tab.log_writer.is_some() {
            return;
        }
        let sid = tab.session_id.clone();
        let (name, host_line) = self
            .session_manager
            .get_session(&sid)
            .map(|s| {
                (
                    s.name.clone(),
                    format!("{}@{}:{}", s.username, s.host, s.port),
                )
            })
            .unwrap_or_else(|| (tab.title.clone(), String::new()));
        let settings = self.session_log_settings.clone();
        if let Some(tab) = self.tabs.get_mut(tab_idx) {
            let mut writer = SessionLogWriter::new(sid, name, host_line, settings);
            writer.write_connected();
            tab.log_writer = Some(writer);
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
            let cmd = tab.terminal.take_submitted_line();
            let sid = tab.session_id.clone();
            let sname = self
                .session_manager
                .get_session(&sid)
                .map(|s| s.name.clone());
            (sid, sname, cmd)
        };
        if let Some(command) = cmd {
            let preview = command_preview(&command, 120);
            self.audit_logger.record(
                AuditEvent::new(AuditCategory::Command, "command.submit", AuditOutcome::Success)
                    .with_session(&sid)
                    .with_detail(serde_json::json!({
                        "preview": preview,
                        "len": command.len(),
                    })),
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
            if let Some(writer) = tab.log_writer.as_mut() {
                while let Some(command) = tab.terminal.take_pending_log_command() {
                    writer.write_prompt_marker(&command);
                }
            }
        }
    }

    fn append_terminal_output_logs(&mut self) {
        if !self.session_log_enabled {
            return;
        }
        for tab in &mut self.tabs {
            if let Some(writer) = tab.log_writer.as_mut() {
                while let Some(chunk) = tab.terminal.take_pending_log_output() {
                    writer.append_output(&chunk);
                }
            }
        }
    }

    fn active_tab_log_status(&self) -> Option<String> {
        let idx = self.active_tab?;
        let tab = self.tabs.get(idx)?;
        tab.log_writer
            .as_ref()
            .map(|w| w.status_label())
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
            || self.variable_dialog.open
            || self.fragment_library.open
            || self.show_terminal_search
            || self.git_sync_panel.is_clone_dialog_open()
            || self.delete_session_confirm.is_some()
            || self.close_tab_confirm_idx.is_some()
            || self.quick_selector.open
            || self.large_upload_pending_path.is_some()
            || self.ssh_import_dialog.open
            || self.command_history_overlay.open
            || self.session_log_dialog.open
            || self.help_docs_dialog.open
    }

    /// 是否将键盘输入交给 PTY（弹窗打开或终端未聚焦时不抢键）
    fn should_capture_pty_keyboard(&self) -> bool {
        if self.global_shortcuts_blocked() {
            return false;
        }
        self.active_tab
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.terminal.is_terminal_focused())
            .unwrap_or(false)
    }

    /// 编辑菜单 ⌘C/⌘V/全选 是否应发给远端 PTY（否则发给当前焦点控件）
    fn route_edit_shortcuts_to_terminal(&self) -> bool {
        !self.global_shortcuts_blocked()
            && self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .map(|t| t.terminal.is_terminal_focused())
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
        self.status_message =
            format!("已聚焦连接搜索框（{}）", crate::platform::accel("J"));
    }

    fn focus_fragment_panel_search(&mut self, ctx: &egui::Context) {
        if !Self::right_dock_open_allowed(Self::layout_window_width(ctx)) {
            let w = Self::layout_window_width(ctx);
            self.status_message = format!(
                "当前窗口约 {:.0}px，§8 需 ≥ {:.0}px 才能打开命令片段侧栏以使用 {}",
                w,
                Self::RESP_LAYOUT_WIDE_MIN_PX,
                crate::platform::accel("K"),
            );
            return;
        }
        self.show_fragment_panel = true;
        self.show_sftp_panel = false;
        ctx.memory_mut(|m| m.request_focus(Self::id_fragment_panel_search()));
        self.status_message =
            format!("已聚焦片段搜索框（{}）", crate::platform::accel("K"));
    }

    fn switch_tab_to_index(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = Some(idx);
            self.selected_session_id = Some(self.tabs[idx].session_id.clone());
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
        if let Some(w) = self.tabs[idx].log_writer.as_mut() {
            w.stop_log();
        }
        self.tabs[idx].terminal.disconnect();
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
            .map(|t| t.session_id.clone());
    }

    fn request_close_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let need_confirm = self.tabs[idx].terminal.is_connected()
            || self.tabs[idx].terminal.is_connecting();
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
    fn disconnect_ssh_keep_buffer_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        if let Some(w) = self.tabs[idx].log_writer.as_mut() {
            w.stop_log();
        }
        self.tabs[idx].terminal.disconnect_ssh_keep_buffer();
        self.sync_monitor_panel_to_active_tab();
        self.status_message = "已断开 SSH（本标签输出已保留，可重连或关闭标签）".to_string();
    }

    fn disconnect_ssh_keep_buffer_active(&mut self) {
        let Some(idx) = self.active_tab else {
            self.status_message = "当前没有打开的终端标签".to_string();
            return;
        };
        self.disconnect_ssh_keep_buffer_at(idx);
    }

    fn reconnect_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        self.tabs[idx].ssh_auto_reconnect_next = None;
        self.tabs[idx].ssh_auto_reconnect_attempts = 0;
        let sid = self.tabs[idx].session_id.clone();
        let Some(session) = self.session_manager.get_session(&sid).cloned() else {
            self.status_message = "未找到会话配置，无法重连".to_string();
            return;
        };
        let offline = self.tabs[idx].terminal.offline_input_snapshot();
        self.tabs[idx].terminal.disconnect();
        let mut temp_key = None;
        let mut terminal = std::mem::replace(&mut self.tabs[idx].terminal, TerminalView::new());
        self.terminal_connect_session(&mut terminal, &session, &mut temp_key);
        self.tabs[idx].terminal = terminal;
        self.tabs[idx].ssh_temp_key = temp_key;
        self.tabs[idx]
            .terminal
            .restore_offline_input_snapshot(offline.0, offline.1);
        if let Some(t) = self.tabs.get_mut(idx) {
            t.title = session.name.clone();
        }
        self.session_manager.mark_session_connected(&sid);
        self.sync_monitor_panel_to_active_tab();
        self.status_message = format!("正在重连：{}", session.name);
    }

    fn reconnect_active_tab(&mut self) {
        let Some(idx) = self.active_tab else {
            self.status_message = "当前没有打开的终端标签".to_string();
            return;
        };
        self.reconnect_tab_at(idx);
    }

    /// 活动标签：SCP 直传或弹出 ≥10MB 选择（与拖放共用，FUNCTIONAL_SPEC §4.3）
    fn enqueue_upload_for_active_tab(&mut self, path: std::path::PathBuf) {
        use crate::core::{decide_upload_dispatch, format_bytes_short, UploadDispatch};

        match decide_upload_dispatch(path.as_path(), self.active_tab.is_some()) {
            UploadDispatch::NoActiveTab => {
                self.status_message = "没有活动的终端标签，无法上传".to_string();
            }
            UploadDispatch::PromptLargeFile { size_bytes } => {
                let disp = path.display().to_string();
                self.large_upload_pending_path = Some(path);
                self.status_message = format!(
                    "请选择上传方式（≥10MB）：{}（{}）",
                    disp,
                    format_bytes_short(size_bytes)
                );
            }
            UploadDispatch::ScpDirect { size_bytes } => {
                if let Some(terminal) = self.current_terminal_mut() {
                    match terminal.start_upload(path.as_path()) {
                        Ok(_) => {
                            self.status_message = format!(
                                "开始 SCP 上传: {} · {}",
                                path.display(),
                                format_bytes_short(size_bytes)
                            );
                        }
                        Err(e) => {
                            self.status_message = format!("上传失败: {}", e);
                        }
                    }
                }
            }
        }
    }

    fn modal_header(ui: &mut egui::Ui, theme: &crate::ui::theme::Theme, title: &str, should_close: &mut bool) {
        if crate::ui::chrome::modal_header(ui, theme, title, theme.font_size_fragment_dialog_body()) {
            *should_close = true;
        }
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
        self.tabs.get_mut(idx).map(|t| &mut t.terminal)
    }

    fn current_terminal(&self) -> Option<&TerminalView> {
        let idx = self.active_tab?;
        self.tabs.get(idx).map(|t| &t.terminal)
    }

    /// 监控侧栏跟随当前标签：重新绑定 SSH 会话上的 exec；未连接则清空展示。
    fn sync_monitor_panel_to_active_tab(&mut self) {
        let Some(tab) = self.active_tab.and_then(|i| self.tabs.get(i)) else {
            self.monitor_panel.clear();
            return;
        };
        if tab.terminal.is_connected() {
            if let (Some(h), Some(mgr)) = (
                tab.terminal.ssh_session_handle(),
                tab.terminal.ssh_manager_clone(),
            ) {
                self.monitor_panel.init(h, mgr);
                return;
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
    fn push_tab_connecting(&mut self, session: &SessionConfig) {
        let mut terminal = TerminalView::new();
        let mut temp_key = None;
        self.terminal_connect_session(&mut terminal, session, &mut temp_key);
        self.tabs.push(TerminalTab {
            session_id: session.id.clone(),
            title: session.name.clone(),
            terminal,
            ssh_auto_reconnect_next: None,
            ssh_auto_reconnect_attempts: 0,
            ssh_temp_key: temp_key,
            log_writer: None,
            last_term_rect: egui::Rect::NOTHING,
        });
        let idx = self.tabs.len() - 1;
        self.ensure_tab_log_writer(idx);
        self.active_tab = Some(idx);
        self.session_manager.mark_session_connected(&session.id);
        self.status_message = format!("正在连接：{}", session.name);
    }

    /// ⌘T / Ctrl+T：为左侧当前选中会话新开标签；未选中时提示（与 ⌘N 新建配置区分）
    fn open_new_tab_from_selection(&mut self) {
        let Some(ref sid) = self.selected_session_id else {
            self.status_message =
                format!(
                    "请先在左侧选择一个连接，再按 {} 新开标签；{} 为新建会话配置",
                    crate::platform::accel("T"),
                    crate::platform::accel("N"),
                );
            return;
        };
        let Some(session) = self.session_manager.get_session(sid).cloned() else {
            self.status_message = "未找到所选会话".to_string();
            return;
        };
        self.selected_session_id = Some(session.id.clone());
        self.push_tab_connecting(&session);
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

        let detail = if self.current_terminal().is_none() {
            "请先打开终端标签".to_string()
        } else if self.terminal_search_query.is_empty() {
                "匹配终端缓冲（含 scrollback）".to_string()
            } else if self.terminal_search_hits.is_empty() {
                "无匹配".to_string()
            } else {
                let hit = self.terminal_search_hits[self.terminal_search_cur];
                format!(
                    "第 {}/{} · 行{} 列{}",
                    self.terminal_search_cur + 1,
                    self.terminal_search_hits.len(),
                    hit.line.0,
                    hit.column + 1
                )
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
                    RichText::new("查找")
                        .size(theme.font_size_panel_title())
                        .color(theme.fg_medium_color()),
                );
                let search_id = egui::Id::new("mistterm_terminal_search_input");
                let input_w = (ui.available_width() * 0.22).clamp(96.0, 200.0);
                let resp = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    search_id,
                    &mut self.terminal_search_query,
                    "关键词…",
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
                    .on_hover_text("忽略大小写")
                    .changed()
                {
                    self.rebuild_terminal_search_matches();
                }
                if crate::ui::chrome::chrome_small_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronLeft,
                )
                .on_hover_text("上一个 (Shift+F3)")
                .clicked()
                {
                    self.terminal_search_step(-1);
                }
                if crate::ui::chrome::chrome_small_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronRight,
                )
                .on_hover_text("下一个 (F3 / Enter)")
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
                            .color(theme.fg_low_color()),
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

    pub(crate) fn menu_open_command_history(&mut self) {
        if self
            .current_terminal()
            .map(|t| t.is_connected())
            .unwrap_or(false)
        {
            self.command_history_overlay.open_new();
        } else {
            self.status_message = "请先连接终端后再使用命令历史".to_string();
        }
    }

    pub(crate) fn menu_copy_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = "请先打开终端标签".to_string();
            return;
        };
        let Some(tab) = self.tabs.get_mut(idx) else {
            return;
        };
        if tab.terminal.menu_copy_to_clipboard() {
            self.status_message = "已复制到剪贴板".to_string();
        } else {
            self.status_message = "终端无内容可复制".to_string();
        }
        ctx.request_repaint();
    }

    pub(crate) fn menu_paste_to_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            self.status_message = "请先打开终端标签".to_string();
            return;
        };
        let Some(tab) = self.tabs.get_mut(idx) else {
            return;
        };
        tab.terminal.menu_paste_from_clipboard(ctx);
    }

    pub(crate) fn menu_select_all_terminal(&mut self, ctx: &egui::Context) {
        let Some(idx) = self.active_tab else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(idx) {
            tab.terminal.menu_select_all();
            ctx.request_repaint();
        }
    }

    pub(crate) fn menu_open_session_log_browser(&mut self) {
        let Some(idx) = self.active_tab else {
            self.status_message = "请先打开一个会话标签".to_string();
            return;
        };
        let session_id = self.tabs[idx].session_id.clone();
        let name = self
            .session_manager
            .get_session(&session_id)
            .map(|s| s.name.clone())
            .unwrap_or(session_id.clone());
        self.session_log_dialog
            .open_for(&session_id, &name, &self.session_log_settings);
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
    pub fn select_session(&mut self, session_id: &str) {
        self.selected_session_id = Some(session_id.to_string());
        self.status_message = format!("已选择会话：{}", session_id);

        if let Some(idx) = self.tabs.iter().position(|t| t.session_id == session_id) {
            self.active_tab = Some(idx);
            return;
        }

        if let Some(session) = self.session_manager.get_session(session_id).cloned() {
            self.push_tab_connecting(&session);
        }
    }

    /// 创建并连接会话
    fn create_and_connect_session(&mut self) {
        if self.new_session_name.is_empty() || self.new_session_host.is_empty() {
            self.status_message = "请填写会话名称和主机地址".to_string();
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
        let backend = self.new_session_secret_backend.clone();
        if !matches!(backend, SecretBackend::LocalEncrypted) {
            self.session_manager.patch_session(&sid, |s| {
                s.secret_backend = backend.clone();
                if backend.is_vault() {
                    s.password.clear();
                }
            });
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "session.create", AuditOutcome::Success)
                .with_session(&sid)
                .with_host(&session.host),
        );

        // 选择会话
        self.selected_session_id = Some(sid.clone());
        self.push_tab_connecting(&session);
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
        self.new_session_group = "默认".to_string();
        self.new_session_private_key_path.clear();
        self.new_session_secret_backend = SecretBackend::default();
    }

    /// 删除会话
    pub fn delete_session(&mut self, session_id: &str) {
        let display = self
            .session_manager
            .get_session(session_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| session_id.to_string());
        for t in &mut self.tabs {
            if t.session_id == session_id {
                t.terminal.disconnect();
            }
        }
        self.audit_logger.record(
            AuditEvent::new(AuditCategory::Session, "session.delete", AuditOutcome::Success)
                .with_session(session_id)
                .with_detail(serde_json::json!({ "name": display })),
        );
        self.session_manager.delete_session(session_id);
        self.tabs.retain(|t| t.session_id != session_id);
        if let Some(idx) = self.active_tab {
            if idx >= self.tabs.len() {
                self.active_tab = self.tabs.len().checked_sub(1);
            }
        }
        if self.selected_session_id.as_ref() == Some(&session_id.to_string()) {
            self.selected_session_id = None;
            if let Some(active) = self.active_tab {
                self.selected_session_id = self.tabs.get(active).map(|t| t.session_id.clone());
            }
        }
        self.status_message = format!("已删除会话：{}", display);
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
            self.edit_session_color_tag = session.color_tag.clone();
            self.edit_session_keepalive_enabled = session.keepalive_enabled;
            self.edit_session_keepalive_interval_secs = session.keepalive_interval_secs;
            self.edit_session_keepalive_count_max = session.keepalive_count_max;
            self.edit_session_keepalive_auto_reconnect = session.keepalive_auto_reconnect;
            self.show_edit_session_dialog = true;
        }
    }

    fn save_edit_session(&mut self) {
        let Some(session_id) = self.edit_session_id.clone() else {
            return;
        };

        if self.edit_session_name.is_empty() || self.edit_session_host.is_empty() {
            self.status_message = "会话名称和主机地址不能为空".to_string();
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
            let _ = self.session_manager.patch_session(&session_id, |s| {
                s.color_tag = color;
                s.keepalive_enabled = ka_on;
                s.keepalive_interval_secs = ka_int;
                s.keepalive_count_max = ka_max;
                s.keepalive_auto_reconnect = ka_ar;
            });
            self.status_message = format!("已更新会话：{}", self.edit_session_name);
            if self.selected_session_id.as_deref() == Some(session_id.as_str()) {
                self.select_session(&session_id);
            }
            self.show_edit_session_dialog = false;
        } else {
            self.status_message = "更新会话失败".to_string();
        }
    }

    /// 注册命令片段栏槽位（须在 Central 之前）。实际 UI 见 [`show_fragment_panel_foreground`]。
    fn show_fragment_panel(&mut self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        let (frag_def, frag_min, frag_max) =
            layout_util::side_panel_widths(ctx, layout_util::SidePanelProfile::Fragment);
        let fragment_panel = egui::SidePanel::right(layout_util::FRAGMENT_PANEL_ID)
            .default_width(frag_def)
            .min_width(frag_min)
            .max_width(frag_max)
            .resizable(true)
            .show_separator_line(false)
            // 仅占布局宽；勿在此绘制内容（CentralPanel 后绘会盖住）。内容在 Foreground Area 重绘。
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                self.fragment_panel_slot_rect = Some(ui.max_rect());
                let w = layout_util::dock_panel_content_width(ui, frag_min, frag_max);
                let h = ui.available_height().max(1.0);
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
        if !matches!(
            self.fragment_filter_category.as_str(),
            "常用" | "Docker" | "K8s" | "全部"
        ) {
            self.fragment_filter_category = "全部".to_string();
        }
        let title_style = theme.color_section_title();
        ui.set_max_width(panel_w);

        // SPEC §3.2 / §5：面板标题区 padding 9px 10px
        egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(
                theme.spacing_panel_title_pad_x(),
                theme.spacing_panel_title_pad_y(),
            ))
            .show(ui, |ui| {
                ui.set_max_width(panel_w);
                let sort_icon = crate::ui::icons::fragment_sort_icon(self.fragment_sort_by);
                let sort_label = match self.fragment_sort_by {
                    SortBy::UsageCount => "次数",
                    SortBy::SuccessRate => "成功率",
                    SortBy::LastUsed => "最近",
                    SortBy::Name => "名称",
                };
                let header = crate::ui::chrome::dock_panel_title_bar(
                    ui,
                    theme,
                    "命令片段",
                    title_style,
                    sort_icon,
                    sort_label,
                    "新建",
                    "关闭命令片段侧栏",
                );
                if header.closed {
                    self.show_fragment_panel = false;
                }
                if header.new_fragment {
                    self.fragment_library.open = true;
                }
                if header.cycle_sort {
                    self.fragment_sort_by = match self.fragment_sort_by {
                        SortBy::UsageCount => SortBy::SuccessRate,
                        SortBy::SuccessRate => SortBy::LastUsed,
                        SortBy::LastUsed => SortBy::Name,
                        SortBy::Name => SortBy::UsageCount,
                    };
                    self.fragment_manager.sort(self.fragment_sort_by);
                }
            });
        ui.separator();

        // SPEC §3.3：搜索框区域 padding 上 4、左右沿用面板 8、下 6
        ui.add_space(theme.spacing_sm());
        let field_w = ui.available_width().max(72.0);
        crate::ui::chrome::form_singleline_field(
            ui,
            theme,
            Self::id_fragment_panel_search(),
            &mut self.fragment_search_query,
            "搜索片段…",
            field_w,
            false,
        );
        ui.add_space(theme.spacing_panel_gap());

        // §5.3：常用 │ Docker │ K8s │ 全部
        if let Some(picked) = crate::ui::chrome::filter_chip_row(
            ui,
            theme,
            &["常用", "Docker", "K8s", "全部"],
            self.fragment_filter_category.as_str(),
            panel_w,
        ) {
            self.fragment_filter_category = picked;
        }

        ui.add_space(theme.spacing_md());

        let search_lower = self.fragment_search_query.to_lowercase();
        let search_match = |f: &FragmentStats| {
            search_lower.is_empty()
                || f.title.to_lowercase().contains(&search_lower)
                || f.command.to_lowercase().contains(&search_lower)
        };

        let mut work: Vec<FragmentStats> = self
            .fragment_manager
            .get_all()
            .iter()
            .filter(|f| search_match(f))
            .cloned()
            .collect();

        match self.fragment_filter_category.as_str() {
            "Docker" => work.retain(|f| f.category == "Docker"),
            "K8s" => work.retain(|f| f.category == "K8s"),
            "常用" => {
                work.retain(|f| f.usage_count > 0);
                if work.is_empty() {
                    work = self
                        .fragment_manager
                        .get_all()
                        .iter()
                        .filter(|f| search_match(f))
                        .cloned()
                        .collect();
                }
                work.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));
            }
            _ => {
                let sort = self.fragment_sort_by;
                match sort {
                    SortBy::UsageCount => {
                        work.sort_by(|a, b| b.usage_count.cmp(&a.usage_count))
                    }
                    SortBy::SuccessRate => work.sort_by(|a, b| {
                        b.success_rate()
                            .partial_cmp(&a.success_rate())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }),
                    SortBy::LastUsed => {
                        work.sort_by(|a, b| b.last_used.cmp(&a.last_used))
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
                                egui::RichText::new("暂无片段")
                                    .size(theme.font_size_panel_title())
                                    .color(theme.fg_low_color()),
                            );
                        }
                        for frag in &work {
                            let stats_line = format_fragment_stats_line(frag);
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
                            if row_resp.title.clicked() {
                                self.begin_fragment_insert(frag);
                            }
                            ui.add_space(theme.spacing_list_item_gap());
                        }
                    });
        ui.visuals_mut().extreme_bg_color = prev_extreme;
    }

    /// 从右侧片段列表点击：支持片段库定义的变量、命令里的 `<占位符>`，以及会话字段替换。
    fn begin_fragment_insert(&mut self, fragment: &FragmentStats) {
        if self.active_tab.is_none() {
            self.status_message = "没有活动的终端标签页".to_string();
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
        let ctx = merge_rhai_context(session, &HashMap::new());
        let after_rhai = match expand_rhai_blocks(&fragment.command, &ctx) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = e;
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
            self.insert_expanded_fragment_with_stats(&fragment.id, &expanded);
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
        tab_idx: usize,
        fragment_id: Option<&str>,
        command: &str,
    ) {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            self.status_message = "标签页不存在".to_string();
            return;
        };
        let start = std::time::Instant::now();
        match tab.terminal.insert_fragment(command) {
            Ok(_) => {
                let dur_ms = start.elapsed().as_millis().max(1) as u64;
                if let Some(fid) = fragment_id {
                    self.fragment_manager.record_execution(fid, true, dur_ms);
                }
                let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                self.status_message = format!("插入命令：{}", command);
            }
            Err(e) => {
                if e == "终端未连接" && tab.terminal.is_connecting() {
                    self.pending_fragment_insert = Some((
                        tab_idx,
                        fragment_id.map(|id| id.to_string()),
                        command.to_string(),
                    ));
                    self.status_message = "连接建立中，片段将在连接成功后自动插入".to_string();
                } else {
                    let dur_ms = start.elapsed().as_millis().max(1) as u64;
                    if let Some(fid) = fragment_id {
                        self.fragment_manager.record_execution(fid, false, dur_ms);
                    }
                    let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                    self.status_message = format!("插入失败：{}", e);
                }
            }
        }
    }

    fn try_flush_pending_fragment_insert(&mut self) {
        let Some((idx, fid_opt, cmd)) = self.pending_fragment_insert.take() else {
            return;
        };
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        if !tab.terminal.is_connected() {
            self.pending_fragment_insert = Some((idx, fid_opt, cmd));
            return;
        }
        self.insert_fragment_at_tab_index(idx, fid_opt.as_deref(), &cmd);
    }

    fn insert_expanded_fragment_with_stats(&mut self, id: &str, expanded: &str) {
        let Some(idx) = self.active_tab else {
            self.status_message = "没有活动的终端标签页".to_string();
            return;
        };
        self.insert_fragment_at_tab_index(idx, Some(id), expanded);
    }

    /// 显示 Git 同步面板
    fn show_git_sync_panel(&mut self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        let (g_def, g_min, g_max) =
            layout_util::side_panel_widths(ctx, layout_util::SidePanelProfile::GitSync);
        let git_panel = egui::SidePanel::right("git_sync_panel")
            .default_width(g_def)
            .min_width(g_min)
            .max_width(g_max)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_panel_frame(theme))
            .show(ctx, |ui| {
                let panel_w = layout_util::dock_panel_content_width(ui, g_min, g_max);
                ui.set_max_width(panel_w);
                let mut close_git = false;
                self.git_sync_panel.show(ui, theme, &mut close_git);
                if close_git {
                    self.show_git_sync_panel = false;
                }
            });
        layout_util::record_right_dock_panel(&git_panel.response, &mut self.right_dock_outer_left_x);
    }

    #[allow(dead_code)]
    fn title_bar_connection(&self) -> Option<crate::ui::chrome::TitleBarConnection> {
        let terminal = self.current_terminal()?;
        let online = terminal.is_connected();
        let connecting = terminal.is_connecting();
        let status_label = if let Some(err) = terminal.connection_error_text() {
            truncate_status(err, 24)
        } else if online {
            "在线".to_string()
        } else if connecting {
            "连接中…".to_string()
        } else {
            "已断开".to_string()
        };
        Some(crate::ui::chrome::TitleBarConnection {
            server_text: terminal.connection_server_text(),
            status_label,
            online,
            connecting,
        })
    }

    /// README §2.4 状态徽章：淡色底，内边距 2px 8px，圆角 4px，11px 高对比字色
    fn status_chip(ui: &mut egui::Ui, text: &str, theme: &crate::ui::theme::Theme) {
        theme.frame_status_chip().show(ui, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(theme.font_size_status_bar())
                    .color(theme.fg_high_color()),
            );
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
                let outer = ui.max_rect();
                ui.painter()
                    .rect_filled(outer, 0.0, theme.chrome_bar_fill());
                ui.painter().hline(
                    outer.x_range(),
                    outer.top(),
                    egui::Stroke::new(1.0, theme.border_divider_color()),
                );
                let content_h = ui
                    .available_height()
                    .min(theme.chrome_bar_content_height(status_h));
                let bar_w = ui.available_width();
                let right_w = 168.0;
                let status_ctx = ui.interact(
                    ui.max_rect(),
                    egui::Id::new("status_bar_context"),
                    egui::Sense::click(),
                );
                status_ctx.context_menu(|ui| {
                    crate::ui::chrome::apply_context_menu_style(ui, &theme);
                    if ui.button("导入 SSH 配置…").clicked() {
                        self.open_ssh_import_dialog();
                    }
                });
                ui.horizontal(|ui| {
                    ui.set_min_height(content_h);
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_status_left_gap(), 0.0);

                    let left_w = (bar_w - right_w).max(120.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_w, content_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            if let Some(metrics) = self.monitor_panel.status_bar_metrics_line() {
                                Self::status_chip(ui, &metrics, &theme);
                            }
                            if self.auto_reconnect_enabled {
                                ui.add_space(theme.spacing_sm());
                                crate::ui::chrome::status_icon_chip(
                                    ui,
                                    &theme,
                                    crate::ui::icons::IconId::Refresh,
                                    "自动重连",
                                );
                            }
                            if self.session_log_enabled {
                                if let Some(log_label) = self.active_tab_log_status() {
                                    ui.add_space(theme.spacing_sm());
                                    let chip = ui
                                        .add(
                                            egui::Label::new(
                                                egui::RichText::new(log_label)
                                                    .size(theme.font_size_status_bar_stats())
                                                    .color(theme.fg_low_color()),
                                            )
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_text("查看本会话的终端输出录制（本地日志文件）");
                                    if chip.clicked() {
                                        if let Some(idx) = self.active_tab {
                                            let sid =
                                                self.tabs.get(idx).map(|t| t.session_id.clone());
                                            let name = sid.as_deref().and_then(|id| {
                                                self.session_manager
                                                    .get_session(id)
                                                    .map(|s| s.name.clone())
                                            });
                                            if let (Some(id), Some(n)) = (sid, name) {
                                                self.session_log_dialog.open_for(
                                                    &id,
                                                    &n,
                                                    &self.session_log_settings,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            if self.sidebar_collapsed {
                                ui.add_space(theme.spacing_sm());
                                if crate::ui::chrome::status_restore_chip(
                                    ui,
                                    &theme,
                                    "连接",
                                    self.tabs.len(),
                                )
                                .on_hover_text("展开左侧连接栏")
                                .clicked()
                                {
                                    self.sidebar_collapsed = false;
                                    self.sidebar_user_dismissed_responsive = false;
                                }
                            }
                            if !self.status_message.is_empty() {
                                ui.add_space(theme.spacing_sm());
                                let msg_w = ui.available_width().max(40.0);
                                ui.add_sized(
                                    [msg_w, content_h],
                                    egui::Label::new(
                                        egui::RichText::new(truncate_status(
                                            &self.status_message,
                                            40,
                                        ))
                                        .size(theme.font_size_status_bar_stats())
                                        .color(status_message_text_color(
                                            &self.status_message,
                                            &theme,
                                        )),
                                    )
                                    .truncate(true),
                                );
                            }
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2(right_w, content_h),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                        ui.spacing_mut().item_spacing =
                            egui::vec2(theme.spacing_tool_btn_gap(), 0.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "{}片段 · {}次",
                                fragment_count, total_runs
                            ))
                            .size(theme.font_size_status_bar_stats())
                            .color(theme.fg_low_color()),
                        );
                        ui.add_space(theme.spacing_status_right_gap());
                        ui.label(
                            egui::RichText::new("|")
                                .size(theme.font_size_status_bar_stats())
                                .color(theme.color_caption_text()),
                        );

                        if crate::ui::chrome::status_tool_icon(
                            ui,
                            &theme,
                            crate::ui::icons::IconId::Fragment,
                        )
                            .on_hover_text(format!(
                                "命令片段 · {}",
                                crate::platform::accel("K")
                            ))
                            .clicked()
                        {
                            if self.show_fragment_panel {
                                self.show_fragment_panel = false;
                            } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                self.show_fragment_panel = true;
                            }
                        }
                        if crate::ui::chrome::status_tool_icon(
                            ui,
                            &theme,
                            crate::ui::icons::IconId::Folder,
                        )
                            .on_hover_text("SFTP 文件 · 浏览/上传/下载")
                            .clicked()
                        {
                            if self.show_sftp_panel {
                                self.show_sftp_panel = false;
                            } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                self.toggle_sftp_panel(ctx);
                            }
                        }
                        if crate::ui::chrome::status_tool_icon(
                            ui,
                            &theme,
                            crate::ui::icons::IconId::Monitor,
                        )
                            .on_hover_text("系统监控")
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
                        },
                    );
                });
            });
    }

    #[cfg(target_os = "macos")]
    fn poll_native_menu_bar(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.native_menu.is_none() {
            let names: Vec<String> = self
                .theme_manager
                .list_themes()
                .iter()
                .map(|t| t.name.clone())
                .collect();
            self.native_menu =
                crate::platform::macos_menu::NativeAppMenu::install(&names).ok();
        }
        if let Some(menu) = &mut self.native_menu {
            menu.sync(
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
            MacMenuAction::ImportSsh => self.open_ssh_import_dialog(),
            MacMenuAction::NewSession => self.show_new_session_dialog = true,
            MacMenuAction::NewTab => self.open_new_tab_from_selection(),
            MacMenuAction::Preferences => self.show_preferences_dialog = true,
            MacMenuAction::CloseTab => self.request_close_active_tab(),
            MacMenuAction::DisconnectSsh => self.disconnect_ssh_keep_buffer_active(),
            MacMenuAction::ReconnectTab => self.reconnect_active_tab(),
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
            MacMenuAction::CommandHistory => self.menu_open_command_history(),
            MacMenuAction::SessionLogBrowser => self.menu_open_session_log_browser(),
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
            MacMenuAction::CloudSync => {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.cloud_sync_panel.open = true;
                }
            }
            MacMenuAction::HelpUserGuide => {
                self.help_docs_dialog.open_page(HelpPage::QuickStart);
            }
            MacMenuAction::HelpFunctionalSpec => {
                match HelpDocsDialog::open_markdown_in_system("product/FUNCTIONAL_SPEC.md") {
                    Ok(()) => self.status_message = "已在系统默认应用中打开功能规格".to_string(),
                    Err(e) => self.status_message = e,
                }
            }
            MacMenuAction::HelpShortcuts => {
                self.help_docs_dialog.open_page(HelpPage::Shortcuts);
            }
            MacMenuAction::HelpRevealDocsFolder => {
                if crate::platform::docs::reveal_docs_directory() {
                    self.status_message = "已在 Finder 中打开文档文件夹".to_string();
                } else {
                    self.status_message = "未找到 docs 目录（开发构建时位于仓库根目录）".to_string();
                }
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

    fn apply_credential_to_new_session_form(&mut self, c: Credential) {
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
        self.new_session_secret_backend = c.secret_backend.clone();
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
        self.status_message = "已从凭证填入新建会话（请检查后连接）".to_string();
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
        crate::ui::icons::UiIcons::reload_if_ppp_changed(ctx);
        self.apply_current_theme(ctx);
        self.apply_responsive_layout(ctx);

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
        self.try_flush_pending_fragment_insert();
        if self.command_history.poll_background_load() {
            ctx.request_repaint();
        }
        self.poll_command_history_from_active_tab();
        self.poll_connect_audit_from_tabs();
        self.poll_session_log_commands();
        self.append_terminal_output_logs();

        if let Some(ti) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(ti) {
                for p in tab.terminal.take_drop_upload_paths() {
                    self.enqueue_upload_for_active_tab(p);
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
            .map(|t| TabReconnectSchedule {
                next_fire: t.ssh_auto_reconnect_next,
                attempts: t.ssh_auto_reconnect_attempts,
            })
            .collect();
        let due: Vec<usize> = schedules
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if !self.tab_auto_reconnect_enabled(&self.tabs[i].session_id) {
                    return None;
                }
                s.next_fire.filter(|t| now >= *t).map(|_| i)
            })
            .collect();
        for i in due {
            self.tabs[i].ssh_auto_reconnect_next = None;
            self.reconnect_tab_at(i);
        }
        for i in 0..self.tabs.len() {
            let sid = self.tabs[i].session_id.clone();
            if !self.tab_auto_reconnect_enabled(&sid) {
                let _ = self.tabs[i].terminal.take_unexpected_disconnect_notified();
                continue;
            }
            if self.tabs[i].terminal.take_unexpected_disconnect_notified() {
                let sched = TabReconnectSchedule {
                    next_fire: self.tabs[i].ssh_auto_reconnect_next,
                    attempts: self.tabs[i].ssh_auto_reconnect_attempts,
                };
                let (new_sched, status) = schedule_after_unexpected_disconnect(
                    sched,
                    DEFAULT_MAX_RECONNECT_ATTEMPTS,
                    now,
                );
                self.tabs[i].ssh_auto_reconnect_next = new_sched.next_fire;
                self.tabs[i].ssh_auto_reconnect_attempts = new_sched.attempts;
                if let Some(s) = status {
                    self.status_message = s.message;
                }
            }
        }

        // FUNCTIONAL_SPEC §2.4：非当前标签仍消费 SSH 输出；有 VTE 更新时用低频重绘，避免与活动 Tab 抢同一帧节奏。
        let active = self.active_tab;
        let mut inactive_tab_vte_dirty = false;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if Some(i) != active && tab.terminal.pump_ssh_only(&theme) {
                inactive_tab_vte_dirty = true;
            }
        }
        if inactive_tab_vte_dirty {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        // SCP 直传结果（`TerminalView::start_upload` 后台线程）
        for tab in &mut self.tabs {
            if let Some(res) = tab.terminal.poll_upload_result() {
                match res {
                    Ok(path) => {
                        self.status_message = format!("文件上传完成：{}", path);
                    }
                    Err(e) => {
                        self.status_message = format!("文件上传失败：{}", e);
                    }
                }
                break;
            }
        }

        // 检查是否有终端等待 rz 上传文件（ZMODEM：`start_rz_upload`，非 SCP `start_upload`）
        if let Some(terminal) = self.current_terminal() {
            if terminal.pending_rz_upload {
                if let Some(t) = self.current_terminal_mut() {
                    t.pending_rz_upload = false;
                }
                if let Some(path) = FileDialog::new()
                    .set_title("选择要上传到远端（rz）的文件")
                    .pick_file()
                {
                    self.status_message = format!("ZMODEM 上传: {}", path.display());
                    if let Some(t) = self.current_terminal_mut() {
                        match t.start_rz_upload(path.as_path()) {
                            Ok(()) => {
                                self.status_message =
                                    format!("ZMODEM 已启动: {}", path.display());
                            }
                            Err(e) => {
                                t.end_rz_handshake_capture();
                                self.status_message = format!("ZMODEM 启动失败: {}", e);
                            }
                        }
                    }
                } else {
                    self.status_message = "rz 上传已取消".to_string();
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
                self.open_new_tab_from_selection();
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::J)) {
                self.focus_sidebar_connection_search(ctx);
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::K)) {
                self.focus_fragment_panel_search(ctx);
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::W)) {
                self.request_close_active_tab();
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::E)) {
                if let Some(ref sid) = self.selected_session_id.clone() {
                    self.open_edit_session_dialog(sid);
                } else {
                    self.status_message =
                        format!(
                            "请先在左侧选择一个连接（{} 编辑会话配置）",
                            crate::platform::accel("E"),
                        );
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
                    self.status_message = format!(
                        "请先连接终端后再使用 {} 搜索命令历史",
                        crate::platform::terminal_history_accel()
                    );
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
            .map(|t| t.terminal.is_terminal_focused())
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
    }
}

impl MistTermApp {
    /// 执行命令片段（⌘J 快速选择）：会话占位符展开；片段库变量与 `<自定义>` 占位符弹窗填写。
    fn execute_fragment(&mut self, fragment: &FragmentStats) {
        if self.selected_session_id.is_none() {
            self.status_message = "请先选择左侧会话".to_string();
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
        let ctx = merge_rhai_context(session, &HashMap::new());
        let after_rhai = match expand_rhai_blocks(&fragment.command, &ctx) {
            Ok(s) => s,
            Err(e) => {
                self.status_message = e;
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
                    .filter(|&i| i < self.tabs.len() && self.tabs[i].session_id == *session_id)
                    .or_else(|| self.tabs.iter().position(|t| t.session_id == *session_id));
                if let Some(idx) = idx {
                    if self.tabs[idx].terminal.is_connected() {
                        self.tabs[idx].terminal.send_command(&expanded);
                        let dur_ms = start.elapsed().as_millis().max(1) as u64;
                        self.fragment_manager
                            .record_execution(fragment.id.as_str(), true, dur_ms);
                        let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                        self.status_message = format!("已执行片段：{}", fragment.title);
                    } else {
                        self.insert_fragment_at_tab_index(idx, Some(fragment.id.as_str()), &expanded);
                    }
                } else {
                    self.status_message = "请为当前会话打开终端标签".to_string();
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
        let icon_px = self.theme_manager.current_theme().size_icon_glyph();
        let s = ui.available_size();
        if s.x.is_finite() && s.y.is_finite() && s.x > 0.0 && s.y > 0.0 {
            ui.set_min_size(s);
        }
        ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            ui.heading("欢迎使用 Mist");
            ui.separator();
            let accent = ui.style().visuals.selection.bg_fill;
            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Rocket,
                "快速开始",
                icon_px,
                8.0,
                move |t| t.color(accent),
            );
            ui.horizontal(|ui| {
                ui.label("1. 点击左侧");
                let px = icon_px;
                let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                crate::ui::icons::paint_icon(
                    ui,
                    r,
                    crate::ui::icons::IconId::Plus,
                    ui.visuals().text_color(),
                    px,
                );
                ui.label("创建新会话");
            });
            ui.horizontal(|ui| {
                ui.label("2. 选择会话");
                let px = icon_px;
                let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                crate::ui::icons::paint_icon(
                    ui,
                    r,
                    crate::ui::icons::IconId::Plug,
                    ui.visuals().text_color(),
                    px,
                );
                ui.label("建立连接");
            });
            ui.horizontal(|ui| {
                ui.label("3. 使用");
                ui.label("rz/sz");
                ui.label("进行文件传输");
            });
            ui.horizontal(|ui| {
                ui.label("自建命令片段：菜单「工具 → 命令片段库」或右侧栏「新建」");
            });
            ui.separator();
            ui.small("提示：双击侧边栏可以折叠/展开");
        });
    }
}

/// 主窗口布局 shell（`docs/product/LAYOUT.md`）
#[path = "workspace.rs"]
mod workspace;

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
                    .color(theme.fg_medium_color())
            };
            let ssh_import_enabled = self.ssh_config_path.exists();

            egui::menu::menu_button(ui, label("终端"), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(theme, "新建会话", "N"))
                    .clicked()
                {
                    self.show_new_session_dialog = true;
                    ui.close_menu();
                }
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(theme, "新建标签", "T"))
                    .clicked()
                {
                    self.open_new_tab_from_selection();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        ssh_import_enabled,
                        egui::Button::new(crate::ui::chrome::menu_item_label(
                            theme,
                            "导入 SSH 配置",
                            None,
                        )),
                    )
                    .clicked()
                {
                    self.open_ssh_import_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(format!(
                        "关闭标签 {}",
                        crate::platform::accel("W")
                    ))
                    .clicked()
                {
                    self.request_close_active_tab();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("断开 SSH（保留输出）").clicked() {
                    self.disconnect_ssh_keep_buffer_active();
                    ui.close_menu();
                }
                if ui.button("重连当前标签").clicked() {
                    self.reconnect_active_tab();
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(format!(
                        "偏好设置 {}",
                        crate::platform::accel(",")
                    ))
                    .clicked()
                {
                    self.show_preferences_dialog = true;
                    ui.close_menu();
                }
                if ui.button("退出").clicked() {
                    frame.close();
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label("编辑"), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(theme, "复制", "C"))
                    .clicked()
                {
                    self.menu_copy_for_context(ctx);
                    ui.close_menu();
                }
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(theme, "粘贴", "V"))
                    .clicked()
                {
                    self.menu_paste_for_context(ctx);
                    ui.close_menu();
                }
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(theme, "全选", "A"))
                    .clicked()
                {
                    self.menu_select_all_for_context(ctx);
                    ui.close_menu();
                }
                ui.separator();
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel(
                        theme,
                        "在终端中搜索",
                        "F",
                    ))
                    .clicked()
                {
                    self.toggle_terminal_search();
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label("视图"), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if ui
                    .button(
                        self.sidebar_collapsed
                            .then_some("展开侧边栏")
                            .unwrap_or("折叠侧边栏"),
                    )
                    .clicked()
                {
                    self.sidebar_collapsed = !self.sidebar_collapsed;
                    if self.sidebar_collapsed {
                        self.sidebar_user_dismissed_responsive = true;
                    } else {
                        self.sidebar_user_dismissed_responsive = false;
                    }
                    ui.close_menu();
                }
                let maximized = frame.info().window_info.maximized;
                if ui
                    .button(if maximized {
                        "还原窗口大小"
                    } else {
                        "最大化窗口"
                    })
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
                    "SFTP 文件",
                )
                .clicked()
                {
                    self.toggle_sftp_panel(ctx);
                    ui.close_menu();
                }
                if crate::ui::chrome::menu_toggle_item(
                    ui,
                    theme,
                    self.show_fragment_panel,
                    "命令片段侧栏",
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
                    "系统监控",
                )
                .clicked()
                {
                    self.toggle_monitor_panel(ctx);
                    ui.close_menu();
                }
                ui.separator();
                ui.menu_button(label("主题"), |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    let current_idx = self.theme_manager.current;
                    let names: Vec<String> = self
                        .theme_manager
                        .list_themes()
                        .iter()
                        .map(|t| t.name.clone())
                        .collect();
                    for (i, name) in names.iter().enumerate() {
                        let selected = i == current_idx;
                        if crate::ui::chrome::menu_theme_item(ui, theme, selected, name).clicked()
                        {
                            self.theme_manager.set_theme_index(i);
                            self.theme_manager.save();
                            ui.ctx().request_repaint();
                            ui.close_menu();
                        }
                    }
                });
            });
            egui::menu::menu_button(ui, label("工具"), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if ui.button("命令片段库…").clicked() {
                    self.fragment_library.open = true;
                    ui.close_menu();
                }
                if ui
                    .button(crate::ui::chrome::menu_item_label_accel_shift(
                        theme,
                        "快速片段选择器",
                        "J",
                    ))
                    .clicked()
                {
                    self.quick_selector.open = true;
                    ui.close_menu();
                }
                if ui
                    .button(crate::ui::chrome::menu_item_label(
                        theme,
                        "命令历史…",
                        Some(crate::platform::terminal_history_accel()),
                    ))
                    .clicked()
                {
                    self.menu_open_command_history();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("凭证管理").clicked() {
                    if self.ensure_right_dock_allowed_or_warn(ctx) {
                        self.credential_panel.open = true;
                    }
                    ui.close_menu();
                }
                if ui.button("云端同步").clicked() {
                    if self.ensure_right_dock_allowed_or_warn(ctx) {
                        self.cloud_sync_panel.open = true;
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("浏览会话日志…").clicked() {
                    self.menu_open_session_log_browser();
                    ui.close_menu();
                }
            });
            egui::menu::menu_button(ui, label("帮助"), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                if ui.button("快速入门…").clicked() {
                    self.help_docs_dialog.open_page(HelpPage::QuickStart);
                    ui.close_menu();
                }
                if ui.button("功能规格（系统打开）").clicked() {
                    match HelpDocsDialog::open_markdown_in_system("product/FUNCTIONAL_SPEC.md") {
                        Ok(()) => self.status_message = "已在系统默认应用中打开功能规格".to_string(),
                        Err(e) => self.status_message = e,
                    }
                    ui.close_menu();
                }
                if ui.button("键盘快捷键…").clicked() {
                    self.help_docs_dialog.open_page(HelpPage::Shortcuts);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("打开文档文件夹").clicked() {
                    if crate::platform::docs::reveal_docs_directory() {
                        self.status_message = "已打开文档文件夹".to_string();
                    } else {
                        self.status_message = "未找到 docs 目录".to_string();
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("关于 Mist").clicked() {
                    self.show_about_dialog = true;
                    ui.close_menu();
                }
            });
        }
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
