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
    Credential, CredentialAuthKind, expand_command_template, expand_fragment_command_stages,
    expand_rhai_blocks, list_placeholder_keys, merge_rhai_context,
    FragmentManager, FragmentStats, SessionConfig, SessionManager, SortBy,
};
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

/// FUNCTIONAL_SPEC §7 快捷键单一真源（关于页与帮助共用）
fn mistterm_functional_spec_shortcuts() -> &'static str {
    "FUNCTIONAL_SPEC §7 摘录（Mac 用 ⌘，Win/Linux 用 Ctrl）\n\
     ⌘N / Ctrl+N — 新建会话\n\
     ⌘E / Ctrl+E — 编辑所选会话\n\
     ⌘T / Ctrl+T — 新终端标签\n\
     ⌘W / Ctrl+W — 关闭当前标签\n\
     ⌘1–9 / Ctrl+1–9 — 切换第 N 个标签\n\
     ⌘Tab / Ctrl+Tab — 下一标签；加 Shift 为上一标签\n\
     ⌘J / Ctrl+J — 聚焦连接搜索\n\
     ⌘K / Ctrl+K — 聚焦片段搜索\n\
     ⌘⇧J / Ctrl+⇧J — 快速片段选择器\n\
     ⌘F / Ctrl+F — 终端内搜索\n\
     ⌘, / Ctrl+, — 偏好设置\n\
     ⌘H / Ctrl+H — 关于与本说明"
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

fn placeholders_needing_user(template: &str) -> Vec<String> {
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
    /// 本帧任意右侧 dock（片段/SFTP/监控等）与主区交界的最左 **屏幕 x**（多栏时取 min，即贴主区的那条边）
    right_dock_outer_left_x: Option<f32>,
    show_git_sync_panel: bool,  // Git 同步面板
    show_monitor_panel: bool,   // 监控面板
    /// 终端视口搜索（当前屏缓冲，不含卷动历史）
    show_terminal_search: bool,
    terminal_search_query: String,
    terminal_search_ignore_case: bool,
    terminal_search_matches: Vec<(usize, usize)>,
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
    new_session_username: String,
    new_session_password: String,
    new_session_group: String,
    new_session_private_key_path: String,

    edit_session_id: Option<String>,
    edit_session_name: String,
    edit_session_host: String,
    edit_session_port: u16,
    edit_session_username: String,
    edit_session_password: String,
    edit_session_group: String,
    edit_session_private_key_path: String,
    sidebar_search_query: String,
    sidebar_filter: String,
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

    /// 网络断开后是否自动重连（偏好设置，§1.4）
    auto_reconnect_enabled: bool,
    /// ≥10MB 上传：待用户选择 SCP 或 ZMODEM 的本地路径
    large_upload_pending_path: Option<std::path::PathBuf>,

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
    const FRAG_VARS_BODY_PX: f32 = 12.0;
    /// 占位符名、辅助说明
    const FRAG_VARS_CAPTION_PX: f32 = 11.0;
    /// 命令预览等宽区
    const FRAG_VARS_MONO_PX: f32 = 12.0;

    /// 应用当前主题（由 ThemeManager 统一管理）
    fn apply_current_theme(&self, ctx: &egui::Context) {
        self.theme_manager.apply_theme(ctx);
    }

    // ── 通用 UI 辅助函数（统一字体大小和间距，按设计规范固定值） ──

    /// 表单字段标签：统一使用 11px 字体 + 加粗（设计规范 §0.2: font_size_panel_title）
    fn ui_field_label(ui: &mut egui::Ui, text: &str, label_color: egui::Color32) {
        ui.label(
            egui::RichText::new(text)
                .size(11.0)
                .strong()
                .color(label_color),
        );
    }

    /// 输入框统一边距：左右10px，上下7px，圆角4px（设计规范 §7/§8）
    fn ui_input_frame(
        ui: &mut egui::Ui,
        input_fill: egui::Color32,
        input_stroke: egui::Stroke,
        add_content: &mut dyn FnMut(&mut egui::Ui),
    ) {
        egui::Frame::none()
            .fill(input_fill)
            .stroke(input_stroke)
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| add_content(ui));
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
            sidebar_width: 200.0,
            sidebar_user_dismissed_responsive: false,
            last_responsive_layout_band: None,
            tabs: Vec::new(),
            active_tab: None,
            status_message: if boot_diagnostics.is_empty() {
                "就绪".to_string()
            } else {
                boot_diagnostics
            },
            show_new_session_dialog: false,
            show_edit_session_dialog: false,
            show_about_dialog: false,
            show_preferences_dialog: false,
            show_fragments_dialog: false,
            show_fragment_panel: false,
            right_dock_outer_left_x: None,
            show_git_sync_panel: false,
            show_monitor_panel: false,
            show_terminal_search: false,
            terminal_search_query: String::new(),
            terminal_search_ignore_case: true,
            terminal_search_matches: Vec::new(),
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
            new_session_username: String::new(),
            new_session_password: String::new(),
            new_session_group: "默认".to_string(),
            new_session_private_key_path: String::new(),
            edit_session_id: None,
            edit_session_name: String::new(),
            edit_session_host: String::new(),
            edit_session_port: 22,
            edit_session_username: String::new(),
            edit_session_password: String::new(),
            edit_session_group: "默认".to_string(),
            edit_session_private_key_path: String::new(),
            sidebar_search_query: String::new(),
            sidebar_filter: "全部".to_string(),
            fragment_search_query: String::new(),
            fragment_sort_by: SortBy::UsageCount,
            variable_dialog: FragmentVariableDialog::default(),
            fragment_vars_completion: FragmentVarsCompletion::default(),
            quick_selector: FragmentQuickSelector::default(),
            theme_manager: ThemeManager::load(),
            delete_session_confirm: None,
            close_tab_confirm_idx: None,
            auto_reconnect_enabled: false,
            large_upload_pending_path: None,
        };

        if let Some(storage) = cc.storage {
            if let Some(p) =
                eframe::get_value::<MistTermUiPersist>(storage, MISTTERM_UI_STORAGE_KEY)
            {
                app.sidebar_width = p.sidebar_width.clamp(160.0, 520.0);
                app.sidebar_collapsed = p.sidebar_collapsed;
                app.sidebar_user_dismissed_responsive = p.sidebar_user_dismissed_responsive;
                app.auto_reconnect_enabled = p.auto_reconnect_enabled;
            }
        }

        app
    }

    fn id_sidebar_connection_search() -> egui::Id {
        egui::Id::new("mistterm_sidebar_connection_search")
    }

    fn id_fragment_panel_search() -> egui::Id {
        egui::Id::new("mistterm_fragment_panel_search")
    }

    /// 阻塞全局快捷键（避免与模态、快速选择器抢键）
    fn global_shortcuts_blocked(&self) -> bool {
        self.show_new_session_dialog
            || self.show_edit_session_dialog
            || self.show_about_dialog
            || self.show_preferences_dialog
            || self.show_fragment_vars_dialog
            || self.delete_session_confirm.is_some()
            || self.close_tab_confirm_idx.is_some()
            || self.quick_selector.open
            || self.large_upload_pending_path.is_some()
    }

    fn focus_sidebar_connection_search(&mut self, ctx: &egui::Context) {
        if self.sidebar_collapsed {
            self.sidebar_collapsed = false;
            self.sidebar_user_dismissed_responsive = false;
        }
        ctx.memory_mut(|m| m.request_focus(Self::id_sidebar_connection_search()));
        self.status_message = "已聚焦连接搜索框（⌘J / Ctrl+J）".to_string();
    }

    fn focus_fragment_panel_search(&mut self, ctx: &egui::Context) {
        if !Self::right_dock_open_allowed(Self::layout_window_width(ctx)) {
            let w = Self::layout_window_width(ctx);
            self.status_message = format!(
                "当前窗口约 {:.0}px，§8 需 ≥ {:.0}px 才能打开命令片段侧栏以使用 ⌘K / Ctrl+K",
                w,
                Self::RESP_LAYOUT_WIDE_MIN_PX
            );
            return;
        }
        self.show_fragment_panel = true;
        self.show_sftp_panel = false;
        ctx.memory_mut(|m| m.request_focus(Self::id_fragment_panel_search()));
        self.status_message = "已聚焦片段搜索框（⌘K / Ctrl+K）".to_string();
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
        self.tabs[idx].terminal.connect(
            &session.host,
            session.port,
            &session.username,
            &session.password,
            &session.private_key_path,
        );
        self.tabs[idx]
            .terminal
            .restore_offline_input_snapshot(offline.0, offline.1);
        if let Some(t) = self.tabs.get_mut(idx) {
            t.title = format!("{}@{}", session.username, session.host);
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
        const TEN_MB: u64 = 10 * 1024 * 1024;
        if self.active_tab.is_none() {
            self.status_message = "没有活动的终端标签，无法上传".to_string();
            return;
        }
        let sz = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if sz >= TEN_MB {
            let disp = path.display().to_string();
            self.large_upload_pending_path = Some(path);
            self.status_message = format!(
                "请选择上传方式（≥10MB）：{}（{}）",
                disp,
                Self::format_bytes_short(sz)
            );
            return;
        }
        if let Some(terminal) = self.current_terminal_mut() {
            match terminal.start_upload(path.as_path()) {
                Ok(_) => {
                    self.status_message = format!(
                        "开始 SCP 上传: {} · {}",
                        path.display(),
                        Self::format_bytes_short(sz)
                    );
                }
                Err(e) => {
                    self.status_message = format!("上传失败: {}", e);
                }
            }
        }
    }

    fn modal_window_frame() -> egui::Frame {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(19, 19, 28))
            .stroke(egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 15),
            ))
            .rounding(10.0)
            .inner_margin(egui::Margin::same(0.0))
    }

    fn modal_content_frame() -> egui::Frame {
        // 设计规范 §8: terminal_pad_x = 16px
        egui::Frame::none().inner_margin(egui::Margin::symmetric(16.0, 14.0))
    }

    fn modal_header(ui: &mut egui::Ui, title: &str, should_close: &mut bool) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(title)
                    .size(Self::FRAG_VARS_BODY_PX)
                    .strong()
                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("×")
                                .size(18.0)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76)),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .frame(false),
                    )
                    .clicked()
                {
                    *should_close = true;
                }
            });
        });
        ui.add_space(8.0);  // 设计规范 §8: spacing_md
        ui.separator();
        ui.add_space(12.0);
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
        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        expand_fragment_command_stages(&fragment.command, session, values).unwrap_or_else(
            |_| {
                let after = fragment.apply_variables(values);
                let ctx = merge_rhai_context(session, values);
                expand_rhai_blocks(&after, &ctx)
                    .map(|rh| expand_command_template(&rh, session, values))
                    .unwrap_or_else(|_| expand_command_template(&after, session, values))
            },
        )
    }

    fn finalize_fragment_command_text(
        &self,
        text: &str,
        values: &HashMap<String, String>,
    ) -> Result<String, String> {
        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        // 与 `expand_fragment_command_stages` 一致：`{{ md5(<user>) }}` 依赖 Rhai 块内再将 `<>` 转成引号字面量
        expand_fragment_command_stages(text, session, values)
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

    /// 上传/提示用简短体积文案
    fn format_bytes_short(n: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        if n >= MB {
            format!("{:.1} MB", n as f64 / MB as f64)
        } else if n >= KB {
            format!("{:.1} KB", n as f64 / KB as f64)
        } else {
            format!("{} B", n)
        }
    }

    /// 为给定会话配置追加一个新终端标签并发起连接（不检查是否已有同会话标签）
    fn push_tab_connecting(&mut self, session: &SessionConfig) {
        let mut terminal = TerminalView::new();
        terminal.connect(
            &session.host,
            session.port,
            &session.username,
            &session.password,
            &session.private_key_path,
        );
        self.tabs.push(TerminalTab {
            session_id: session.id.clone(),
            title: format!("{}@{}", session.username, session.host),
            terminal,
            ssh_auto_reconnect_next: None,
            ssh_auto_reconnect_attempts: 0,
        });
        self.active_tab = Some(self.tabs.len() - 1);
        self.session_manager.mark_session_connected(&session.id);
        self.status_message = format!("正在连接：{}", session.name);
    }

    /// ⌘T / Ctrl+T：为左侧当前选中会话新开标签；未选中时提示（与 ⌘N 新建配置区分）
    fn open_new_tab_from_selection(&mut self) {
        let Some(ref sid) = self.selected_session_id else {
            self.status_message =
                "请先在左侧选择一个连接，再按 ⌘T / Ctrl+T 新开标签；⌘N 为新建会话配置".to_string();
            return;
        };
        let Some(session) = self.session_manager.get_session(sid).cloned() else {
            self.status_message = "未找到所选会话".to_string();
            return;
        };
        self.selected_session_id = Some(session.id.clone());
        self.push_tab_connecting(&session);
    }

    fn rebuild_terminal_search_matches(&mut self) {
        self.terminal_search_matches.clear();
        let Some(t) = self.current_terminal() else {
            self.terminal_search_cur = 0;
            return;
        };
        if self.terminal_search_query.is_empty() {
            self.terminal_search_cur = 0;
            return;
        }
        self.terminal_search_matches =
            t.search_viewport(&self.terminal_search_query, self.terminal_search_ignore_case);
        if self.terminal_search_matches.is_empty() {
            self.terminal_search_cur = 0;
        } else {
            self.terminal_search_cur = self
                .terminal_search_cur
                .min(self.terminal_search_matches.len() - 1);
        }
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

        // 选择会话
        self.selected_session_id = Some(session.id.clone());
        self.push_tab_connecting(&session);
        self.reset_new_session_form();
    }

    /// 重置新建会话表单
    fn reset_new_session_form(&mut self) {
        self.new_session_name.clear();
        self.new_session_host.clear();
        self.new_session_port = 22;
        self.new_session_username.clear();
        self.new_session_password.clear();
        self.new_session_group = "默认".to_string();
        self.new_session_private_key_path.clear();
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
            self.edit_session_username = session.username;
            // FUNCTIONAL_SPEC §1.3.3：不将真实密码填入 UI
            self.edit_session_password = "****".to_string();
            self.edit_session_group = session.group;
            self.edit_session_private_key_path = session.private_key_path;
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
            self.status_message = format!("已更新会话：{}", self.edit_session_name);
            if self.selected_session_id.as_deref() == Some(session_id.as_str()) {
                self.select_session(&session_id);
            }
            self.show_edit_session_dialog = false;
        } else {
            self.status_message = "更新会话失败".to_string();
        }
    }

    /// 显示命令片段面板（带统计信息）
    ///
    /// 布局对齐 `docs/product/SPECIFICATION_DETAILED.md` §5：扁平列表 + 四标签，卡片三行（标题 / 命令截断 / 统计）。
    fn show_fragment_panel(&mut self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        if !matches!(
            self.fragment_filter_category.as_str(),
            "常用" | "Docker" | "K8s" | "全部"
        ) {
            self.fragment_filter_category = "全部".to_string();
        }
        let stats_num = {
            let c = theme.fg_high_color();
            let [r, g, b, _] = c.to_array();
            egui::Color32::from_rgba_unmultiplied(r, g, b, 64)
        };
        let title_style = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51);
        let (frag_def, frag_min, frag_max) =
            layout_util::side_panel_widths(ctx, layout_util::SidePanelProfile::Fragment);
        egui::SidePanel::right("fragment_panel")
            .default_width(frag_def)
            .min_width(frag_min)
            .max_width(frag_max)
            .resizable(true)
            // 默认可见分隔线画在 Frame 左缘；与中央区叠层时偶发错位。关闭后仍可在左缘拖拽改宽度。
            .show_separator_line(false)
            // 与 SFTP 等默认 SidePanel 一致：外侧不用圆角，避免左缘（靠终端一侧）抗锯齿与底色错位像「灰条」；
            // 卡片行内仍保留圆角（见下方片段 Frame）。
            .frame(
                egui::Frame::none()
                    .fill(theme.bg_window_color())
                    .rounding(0.0)
                    .inner_margin(egui::Margin::same(theme.spacing_panel_content_x())),
            )
            .show(ctx, |ui| {
                // 与下方 `.inner_margin(egui::Margin::same(theme.spacing_panel_content_x()))` 左侧一致
                layout_util::record_right_dock_outer_left(ui, 8.0, &mut self.right_dock_outer_left_x);

                // 某些帧上 `available_width` 会为 ∞，与 max 组合仍为 ∞，会把整个侧栏撑满屏
                let mut aw = ui.available_width();
                if !aw.is_finite() || aw > 10_000.0 {
                    aw = ui.max_rect().width();
                }
                if !aw.is_finite() || aw < 1.0 {
                    aw = frag_def;
                }
                // 勿 `ui.set_width`：会干扰 SidePanel 实际宽度，叠层时中央终端易「压」在右栏上
                let panel_w = aw.clamp(frag_min, frag_max);
                // SPEC §3.2 / §5：面板标题区 padding 9px 10px
                egui::Frame::none()
                    .inner_margin(egui::Margin::symmetric(theme.spacing_panel_title_pad_x(), theme.spacing_panel_title_pad_y()))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("命令片段")
                                    .size(theme.font_size_small())
                                    .strong()
                                    .color(title_style),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("−")
                                                    .size(theme.font_size_large())
                                                    .color(title_style),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::NONE)
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.show_fragment_panel = false;
                                    }
                                    let hdr_btn_h = 22.0;
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("➕ 新建…").size(theme.font_size_panel_title()),
                                            )
                                            .min_size(egui::vec2(0.0, hdr_btn_h))
                                            .rounding(theme.radius_list_item()),
                                        )
                                        .on_hover_text(
                                            "打开片段库：自建命令、`<变量>`、会话占位符（host/user/port）等",
                                        )
                                        .clicked()
                                    {
                                        self.fragment_library.open = true;
                                    }
                                    let sort_label = match self.fragment_sort_by {
                                        SortBy::UsageCount => "🔢 次数",
                                        SortBy::SuccessRate => "✅ 成功率",
                                        SortBy::LastUsed => "🕐 最近",
                                        SortBy::Name => "🔤 名称",
                                    };
                                    if ui
                                        .add(
                                            egui::Button::new(egui::RichText::new(sort_label).size(theme.font_size_tool_btn()))
                                                .min_size(egui::vec2(0.0, hdr_btn_h))
                                                .rounding(theme.radius_list_item()),
                                        )
                                        .clicked()
                                    {
                                        self.fragment_sort_by =
                                            match self.fragment_sort_by {
                                                SortBy::UsageCount => SortBy::SuccessRate,
                                                SortBy::SuccessRate => SortBy::LastUsed,
                                                SortBy::LastUsed => SortBy::Name,
                                                SortBy::Name => SortBy::UsageCount,
                                            };
                                        self.fragment_manager.sort(self.fragment_sort_by);
                                    }
                                },
                            );
                        });
                    });
                ui.separator();

                // SPEC §3.3：搜索框区域 padding 上 4、左右沿用面板 8、下 6
                ui.add_space(theme.spacing_sm());
                ui.add(
                    egui::TextEdit::singleline(&mut self.fragment_search_query)
                        .id(Self::id_fragment_panel_search())
                        .hint_text("搜索片段…")
                        .desired_width((panel_w - 14.0).max(72.0)),
                );
                ui.add_space(theme.spacing_panel_gap());

                // §5.3：常用 │ Docker │ K8s │ 全部（与左侧「全部·在线·离线」同等分样式）
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let item_w = ((panel_w - 8.0) / 4.0).max(34.0);
                    for label in ["常用", "Docker", "K8s", "全部"] {
                        let active = self.fragment_filter_category == label;
                        let text_color = if active {
                            egui::Color32::from_rgba_unmultiplied(102, 126, 234, 128)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 46)
                        };
                        let fill = if active {
                            egui::Color32::from_rgba_unmultiplied(102, 126, 234, 128)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let resp = ui.add(
                            egui::Button::new(
                                egui::RichText::new(label).size(theme.font_size_small()).color(text_color),
                            )
                            .fill(fill)
                            .stroke(egui::Stroke::NONE)
                            .rounding(theme.radius_status_btn())
                            .min_size(egui::vec2(item_w, 20.0)),
                        );
                        if resp.clicked() {
                            self.fragment_filter_category = label.to_string();
                        }
                    }
                });

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
                                .filter(|f| search_match(&f))
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
                // 滑道/占位条用面板同色，避免默认 extreme_bg 比 bg_window 浅时像一条竖灰带（SFTP 默认 Scroll 未隐藏条，不明显）。
                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = theme.bg_window_color();
                egui::ScrollArea::vertical()
                    .id_source("mistterm_fragment_list_scroll")
                    .auto_shrink([false, false])
                    .max_height(scroll_h)
                    // egui：clip 偏窄时会把纵向滚动条整体左移以免被裁掉，会压到片段文字上。
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                    )
                    .show(ui, |ui| {
                        // 不要用 set_width(panel_w)：滚动条会占宽度，强设 full 宽会导致内容超出可视区被裁切
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

                            let tag_col = (panel_w * 0.17).clamp(40.0, 56.0);
                            let mid_gap = 4.0_f32;

                            // SPEC §5.4：片段卡片 padding 7px × 8px
                            egui::Frame::none()
                                .inner_margin(egui::Margin::symmetric(theme.spacing_panel_content_x(), theme.spacing_card_y()))
                                .rounding(theme.radius_list_item())
                                .show(ui, |ui| {
                                    let full_w = ui.available_width();
                                    let main_w = (full_w - tag_col - mid_gap).max(52.0);

                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.set_max_width(main_w);
                                            ui.spacing_mut().item_spacing.y = 2.0;

                                            let title_resp = ui.link(
                                                egui::RichText::new(&frag.title)
                                                    .size(theme.font_size_normal())
                                                    .color(theme.fg_medium_color()),
                                            );
                                            if title_resp.clicked() {
                                                self.begin_fragment_insert(frag);
                                            }
                                            title_resp.on_hover_text(&frag.command);

                                            let cmd_trim = frag.command.trim();
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(cmd_trim)
                                                        .size(theme.font_size_small())
                                                        .monospace()
                                                        .color(theme.fg_low_color()),
                                                )
                                                .truncate(true),
                                            )
                                            .on_hover_text(cmd_trim);

                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&stats_line)
                                                        .size(theme.font_size_small())
                                                        .color(stats_num),
                                                )
                                                .truncate(true),
                                            );
                                        });

                                        ui.add_space(mid_gap);
                                        ui.vertical(|ui| {
                                            ui.set_width(tag_col);
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::TOP),
                                                |ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(&tag_label)
                                                                .size(theme.font_size_tag())
                                                                .color(
                                                                    egui::Color32::from_rgba_unmultiplied(
                                                                        102, 126, 234, 115,
                                                                    ),
                                                                ),
                                                        )
                                                        .truncate(true),
                                                    )
                                                    .on_hover_text(&tag_label);
                                                },
                                            );
                                        });
                                    });
                                });

                            ui.add_space(1.0);
                            ui.separator();
                            ui.add_space(1.0);
                        }
                    });
                ui.visuals_mut().extreme_bg_color = prev_extreme;
            });
    }

    /// 从右侧片段列表点击：支持片段库定义的变量、命令里的 `<占位符>`，以及会话字段替换。
    fn begin_fragment_insert(&mut self, fragment: &FragmentStats) {
        if self.active_tab.is_none() {
            self.status_message = "没有活动的终端标签页".to_string();
            return;
        }

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
        egui::SidePanel::right("git_sync_panel")
            .default_width(g_def)
            .min_width(g_min)
            .max_width(g_max)
            .resizable(true)
            .show(ctx, |ui| {
                layout_util::record_right_dock_outer_left(
                    ui,
                    layout_util::EGUI_SIDE_PANEL_FRAME_MARGIN_X,
                    &mut self.right_dock_outer_left_x,
                );
                self.git_sync_panel.show(ui, theme);
            });
    }

    /// README §2.4 状态徽章：淡色底，内边距 2px 8px，圆角 4px，11px 高对比字色
    fn status_chip(ui: &mut egui::Ui, text: &str, theme: &crate::ui::theme::Theme) {
        let c = theme.fg_high_color();
        let [r, g, b, _] = c.to_array();
        let fill = egui::Color32::from_rgba_unmultiplied(r, g, b, 51);
        egui::Frame::none()
            .fill(fill)
            .rounding(egui::Rounding::same(theme.radius_list_item()))
            .inner_margin(egui::Margin::symmetric(theme.spacing_list_item_x(), 3.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(theme.font_size_panel_title())
                        .color(theme.fg_high_color()),
                );
            });
    }

    /// 底栏：上行 **44px** 快捷文字按钮（易见）；下行 SPEC §6 **28px** 状态区。单行 28px 时易换行被裁切且纯图标过淡。
    fn show_bottom_chrome(&mut self, ctx: &egui::Context) {
        const STATUS_H: f32 = 28.0;
        const QUICK_H: f32 = 44.0;
        /// 快捷栏与状态栏之间缝隙（对齐 SPEC：避免两行贴死）
        const QUICK_STATUS_GAP: f32 = 3.0;
        const BOTTOM_CHROME_H: f32 = QUICK_H + QUICK_STATUS_GAP + STATUS_H;

        let theme = self.theme_manager.current_theme().clone();
        let bar_fill = theme.bg_window_color();
        let btn_idle = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 20);
        let btn_primary = theme.accent_color();
        let status_bar_bg = theme.bg_tab_bar_color();
        let h_btn = 32.0;

        let total_runs: u32 = self
            .fragment_manager
            .get_all()
            .iter()
            .map(|f| f.usage_count)
            .sum();

        egui::TopBottomPanel::bottom("bottom_chrome")
            .exact_height(BOTTOM_CHROME_H)
            .frame(
                egui::Frame::none()
                    .inner_margin(egui::Margin::ZERO)
                    .outer_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                // 上：快捷操作栏
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), QUICK_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0.0, bar_fill);
                        ui.painter().hline(
                            rect.x_range(),
                            rect.top(),
                            egui::Stroke::new(1.0, theme.border_color()),
                        );
                        ui.add_space(theme.spacing_list_item_x());
                        ui.spacing_mut().item_spacing = egui::vec2(theme.spacing_md(), 0.0);
                        ui.horizontal(|ui| {
                            let mk = |label: &str, fill: egui::Color32, w: f32| {
                                egui::Button::new(
                                    egui::RichText::new(label)
                                        .size(theme.font_size_normal())
                                        .color(theme.fg_high_color()),
                                )
                                .fill(fill)
                                .rounding(theme.radius_status_btn())
                                .min_size(egui::vec2(w, h_btn))
                            };

                            if ui
                                .add(mk("📋 命令片段", btn_primary, 108.0))
                                .on_hover_text(
                                    "切换右侧片段列表 · ⌘J · 菜单「工具→命令片段库」\n§8：窗口宽 <1200px 时无法打开右侧侧栏，底栏会显示「§8中/窄」提示",
                                )
                                .clicked()
                            {
                                if self.show_fragment_panel {
                                    self.show_fragment_panel = false;
                                } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                    self.show_fragment_panel = true;
                                }
                            }
                            if ui.add(mk("📤 上传", btn_idle, 88.0))
                                .on_hover_text(
                                    "上传至远端当前目录（<10MB 默认 SCP；≥10MB 可选 ZMODEM）。拖放文件到终端区亦可（§4.3）。",
                                )
                                .clicked()
                            {
                                if let Some(path) = FileDialog::new().pick_file() {
                                    self.enqueue_upload_for_active_tab(path);
                                }
                            }
                            if ui.add(mk("🔍 搜索", btn_idle, 88.0))
                                .on_hover_text("在当前终端视口搜索 · ⌘F / Ctrl+F")
                                .clicked()
                            {
                                self.show_terminal_search = !self.show_terminal_search;
                                if self.show_terminal_search {
                                    self.rebuild_terminal_search_matches();
                                }
                            }
                            if ui.add(mk("⚙️ 设置", btn_idle, 88.0))
                                .on_hover_text("偏好设置（主题、云端同步入口）· 同 ⌘,")
                                .clicked()
                            {
                                self.show_preferences_dialog = true;
                            }
                            if ui.add(mk("🔀 Git", btn_idle, 88.0)).on_hover_text("Git 同步面板").clicked()
                            {
                                if self.show_git_sync_panel {
                                    self.show_git_sync_panel = false;
                                } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                    self.show_git_sync_panel = true;
                                }
                            }
                            if ui.add(mk("📊 监控", btn_idle, 88.0)).on_hover_text("系统监控面板").clicked()
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
                            if ui
                                .add(mk("🔐 凭证", btn_idle, 88.0))
                                .on_hover_text("加密凭证库")
                                .clicked()
                            {
                                if self.credential_panel.open {
                                    self.credential_panel.open = false;
                                } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                    self.credential_panel.open = true;
                                }
                            }
                            if ui
                                .add(mk("☁️ 同步", btn_idle, 88.0))
                                .on_hover_text("云端同步 / 导出包")
                                .clicked()
                            {
                                if self.cloud_sync_panel.open {
                                    self.cloud_sync_panel.open = false;
                                } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                    self.cloud_sync_panel.open = true;
                                }
                            }
                            if ui
                                .add(mk("📂 SFTP", btn_idle, 104.0))
                                .on_hover_text(
                                    "切换远端 SFTP 侧栏。ZMODEM 下载目录可在连接后于状态栏查看。",
                                )
                                .clicked()
                            {
                                if self.show_sftp_panel {
                                    self.show_sftp_panel = false;
                                } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                                    self.show_sftp_panel = true;
                                    self.sftp_last_tab = None;
                                    self.sftp_panel.request_list_on_open();
                                    if let Some(terminal) = self.current_terminal() {
                                        self.status_message = format!(
                                            "SFTP 侧栏已打开；本机 ZMODEM 目录 {}",
                                            terminal.download_dir()
                                        );
                                    }
                                }
                            }
                        });
                    },
                );

                ui.add_space(QUICK_STATUS_GAP);

                // 下：状态栏（28px，单行，避免嵌套 horizontal + 右对齐再次换行被裁掉）
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), STATUS_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0.0, status_bar_bg);
                        ui.painter().hline(
                            rect.x_range(),
                            rect.top(),
                            egui::Stroke::new(1.0, theme.subtle_line_color()),
                        );
                        ui.add_space(theme.spacing_status_bar_x());

                        let session_count = self.tabs.len();
                        let fragment_count = self.fragment_manager.get_all().len();

                        let mut server_line = "⚡ 未选择会话".to_string();
                        let mut font_px = "14px".to_string();
                        let mut duration_chip = "—".to_string();

                        if let Some(terminal) = self.current_terminal() {
                            server_line = format!("⚡ {}", terminal.connection_server_text());
                            font_px = format!("{:.0}px", terminal.font_size());
                            duration_chip = if let Some(err) = terminal.connection_error_text() {
                                truncate_status(err, 28)
                            } else if terminal.is_connected() {
                                terminal.connection_duration_text()
                            } else {
                                "连接中…".to_string()
                            };
                        }

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(theme.spacing_md(), 0.0);
                            // §8 档位放最前，避免被长 host 行挤出可视区
                            let lw = Self::layout_window_width(ctx);
                            if let Some(b) = Self::layout_band_from_width(lw) {
                                let band_chip = match b {
                                    ResponsiveLayoutBand::Narrow => format!("§8窄 {:.0}px", lw),
                                    ResponsiveLayoutBand::Medium => format!("§8中 {:.0}px", lw),
                                    ResponsiveLayoutBand::Wide => format!("§8宽 {:.0}px", lw),
                                };
                                Self::status_chip(ui, &band_chip, &theme);
                            }
                            if self.sidebar_collapsed {
                                let restore = egui::Button::new(
                                    egui::RichText::new(format!("▸ 连接 · {}", session_count))
                                        .size(theme.font_size_small())
                                        .color(egui::Color32::from_rgba_unmultiplied(
                                            102, 126, 234, 64,
                                        )),
                                )
                                .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 10))
                                .rounding(theme.radius_status_btn())
                                .min_size(egui::vec2(56.0, 18.0));
                                if ui.add(restore).clicked() {
                                    self.sidebar_collapsed = false;
                                    self.sidebar_user_dismissed_responsive = false;
                                }
                            }
                            if !self.show_fragment_panel {
                                let restore = egui::Button::new(
                                    egui::RichText::new(format!("▸ 命令片段 · {}", fragment_count))
                                        .size(theme.font_size_small())
                                        .color(egui::Color32::from_rgba_unmultiplied(
                                            102, 126, 234, 64,
                                        )),
                                )
                                .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 10))
                                .rounding(theme.radius_status_btn())
                                .min_size(egui::vec2(56.0, 18.0));
                                if ui.add(restore).clicked() {
                                    if self.ensure_right_dock_allowed_or_warn(ctx) {
                                        self.show_fragment_panel = true;
                                    }
                                }
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&server_line)
                                        .monospace()
                                        .size(theme.font_size_panel_title())
                                        .color(theme.fg_medium_color()),
                                )
                                .truncate(true),
                            );
                            ui.add_space(theme.spacing_sm());
                            Self::status_chip(ui, "UTF-8", &theme);
                            Self::status_chip(ui, &font_px, &theme);
                            Self::status_chip(ui, &duration_chip, &theme);
                            Self::status_chip(ui, &format!("累计{}次", total_runs), &theme);

                            if !self.status_message.is_empty() {
                                ui.add_space(theme.spacing_list_item_x());
                                let hint_w = (ui.available_width() - 8.0).clamp(160.0, 560.0);
                                ui.add_sized(
                                    [hint_w, STATUS_H],
                                    egui::Label::new(
                                        egui::RichText::new(truncate_status(
                                            &self.status_message,
                                            140,
                                        ))
                                        .size(theme.font_size_small())
                                        .color(status_message_text_color(
                                            &self.status_message,
                                            &theme,
                                        )),
                                    )
                                    .truncate(true),
                                );
                            }
                        });
                    },
                );
            });
    }

    fn apply_credential_to_new_session_form(&mut self, c: Credential) {
        self.show_new_session_dialog = true;
        self.new_session_name = if c.name.is_empty() {
            c.host.clone()
        } else {
            c.name.clone()
        };
        self.new_session_host = c.host.clone();
        self.new_session_port = c.port.max(1);
        self.new_session_username = c.username.clone();
        self.new_session_password = if matches!(c.auth, CredentialAuthKind::Password) {
            c.secret.clone()
        } else {
            String::new()
        };
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
        };
        eframe::set_value(storage, MISTTERM_UI_STORAGE_KEY, &p);
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.apply_current_theme(ctx);
        self.apply_responsive_layout(ctx);

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
            self.show_terminal_search = !self.show_terminal_search;
            if self.show_terminal_search {
                self.rebuild_terminal_search_matches();
            }
        }

        let theme = self.theme_manager.current_theme().clone();

        // 监控：`exec` 由 shell 泵串行执行，在此处轮询结果并驱动自动刷新
        self.monitor_panel.update(ctx, self.show_monitor_panel);
        self.try_flush_pending_fragment_insert();

        if let Some(ti) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(ti) {
                for p in tab.terminal.take_drop_upload_paths() {
                    self.enqueue_upload_for_active_tab(p);
                }
            }
        }

        let now = Instant::now();
        let mut reconnect_fire: Vec<usize> = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            if let Some(t) = tab.ssh_auto_reconnect_next {
                if self.auto_reconnect_enabled && now >= t {
                    reconnect_fire.push(i);
                }
            }
        }
        for i in reconnect_fire {
            self.tabs[i].ssh_auto_reconnect_next = None;
            self.reconnect_tab_at(i);
        }
        for tab in &mut self.tabs {
            if !self.auto_reconnect_enabled {
                let _ = tab.terminal.take_unexpected_disconnect_notified();
                continue;
            }
            if tab.terminal.take_unexpected_disconnect_notified() {
                if tab.ssh_auto_reconnect_attempts < 5 {
                    let exp = tab.ssh_auto_reconnect_attempts.min(4);
                    let delay = Duration::from_secs(1u64 << exp);
                    tab.ssh_auto_reconnect_next = Some(now + delay);
                    tab.ssh_auto_reconnect_attempts += 1;
                    self.status_message = format!(
                        "连接已断开，{} 秒后将自动重连（{}/5）",
                        delay.as_secs(),
                        tab.ssh_auto_reconnect_attempts
                    );
                } else {
                    self.status_message = "连接已断开；自动重连已达 5 次上限".to_string();
                }
            }
        }

        // FUNCTIONAL_SPEC §2.4：非当前标签仍消费 SSH 输出；有 VTE 更新时用低频重绘，避免与活动 Tab 抢同一帧节奏。
        let active = self.active_tab;
        let mut inactive_tab_vte_dirty = false;
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if Some(i) != active && tab.terminal.pump_ssh_only() {
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
                        "请先在左侧选择一个连接（⌘E / Ctrl+E 编辑会话配置）".to_string();
                }
            }
            if ctx.input(|i| Self::input_primary_mod(i) && i.key_pressed(egui::Key::H)) {
                self.show_about_dialog = true;
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

        // SPEC §2：标题栏 36px，padding 10×16；底部分割线 §2.1
        egui::TopBottomPanel::top("title_bar")
            .exact_height(36.0)
            .frame(
                egui::Frame::none()
                    .fill(theme.bg_tab_bar_color())
                    .inner_margin(egui::Margin::ZERO)
                    .outer_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
            let title_panel_rect = ui.max_rect();
            ui.horizontal(|ui| {
                ui.menu_button("文件", |ui| {
                    if ui.button("新建会话 ⌘N").clicked() {
                        self.show_new_session_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("偏好设置 ⌘,").clicked() {
                        self.show_preferences_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("关闭标签 ⌘W").clicked() {
                        self.request_close_active_tab();
                        ui.close_menu();
                    }
                    if ui.button("断开 SSH（保留输出）").clicked() {
                        self.disconnect_ssh_keep_buffer_active();
                        ui.close_menu();
                    }
                    if ui.button("重连当前标签").clicked() {
                        self.reconnect_active_tab();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        frame.close();
                        ui.close_menu();
                    }
                });
                ui.menu_button("视图", |ui| {
                    if ui.button(self.sidebar_collapsed.then(|| "展开侧边栏").unwrap_or("折叠侧边栏")).clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                        if self.sidebar_collapsed {
                            self.sidebar_user_dismissed_responsive = true;
                        } else {
                            self.sidebar_user_dismissed_responsive = false;
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    let sftp_menu = if self.show_sftp_panel {
                        "✓ SFTP 文件面板"
                    } else {
                        "SFTP 文件面板"
                    };
                    if ui.button(sftp_menu).clicked() {
                        if self.show_sftp_panel {
                            self.show_sftp_panel = false;
                        } else if self.ensure_right_dock_allowed_or_warn(ctx) {
                            self.show_sftp_panel = true;
                            self.sftp_last_tab = None;
                            self.sftp_panel.request_list_on_open();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button("主题", |ui| {
                        let theme_names: Vec<String> = self
                            .theme_manager
                            .list_themes()
                            .iter()
                            .map(|t| t.name.clone())
                            .collect();
                        let current_idx = self.theme_manager.current;
                        for (i, name) in theme_names.into_iter().enumerate() {
                            let label = if i == current_idx {
                                format!("✓ {}", name)
                            } else {
                                name
                            };
                            if ui.button(label).clicked() {
                                self.theme_manager.set_theme_index(i);
                                self.theme_manager.save();
                                ui.close_menu();
                            }
                        }
                    });
                });
                ui.menu_button("工具", |ui| {
                    if ui.button("命令片段库…").clicked() {
                        self.fragment_library.open = true;
                        ui.close_menu();
                    }
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
                });
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        self.show_about_dialog = true;
                        ui.close_menu();
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let title = self
                        .current_terminal()
                        .map(|t| format!("MistTerm - {}", t.connection_server_text()))
                        .unwrap_or_else(|| "MistTerm".to_string());
                    // README §2.4 标题栏：13px
                    ui.label(
                        egui::RichText::new(title)
                            .size(theme.font_size_title_bar())
                            .color(theme.fg_low_color()),
                    );
                });
            });
            ui.painter().hline(
                title_panel_rect.x_range(),
                title_panel_rect.bottom() - 1.0,
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                ),
            );
        });

        // 右侧 dock：须先于底栏与 Central 注册（见下方 show_bottom_chrome 注释）
        self.right_dock_outer_left_x = None;

        if self.show_fragment_panel {
            self.show_fragment_panel(ctx, &theme);
        }

        // Git 同步面板
        if self.show_git_sync_panel {
            self.show_git_sync_panel(ctx, &theme);
        }

        let mut cred_action: Option<CredentialPanelAction> = None;
        if self.credential_panel.open {
            if self
                .credential_panel
                .show_side_panel(ctx, &theme, &mut cred_action, &mut self.right_dock_outer_left_x)
            {
                self.credential_panel.open = false;
            }
        }

        let fragments_export_path = FragmentManager::default_config_path();
        let sessions_export_path = self.session_manager.storage_path().clone();
        let theme_export_path = ThemeManager::config_path();
        let mut deps = CloudSyncDeps {
            fragments_path: &fragments_export_path,
            sessions_path: &sessions_export_path,
            theme_path: &theme_export_path,
            fragment_manager: &mut self.fragment_manager,
            theme_manager: &mut self.theme_manager,
            session_manager: &mut self.session_manager,
            credential_panel: &mut self.credential_panel,
        };
        self.cloud_sync_panel
            .show(ctx, &theme, &mut deps, &mut self.right_dock_outer_left_x);

        if let Some(CredentialPanelAction::UseForQuickConnect(c)) = cred_action {
            self.apply_credential_to_new_session_form(c);
        }

        // SFTP（右侧面板；切换终端标签时重置远端路径并重新拉列表）
        let mut close_sftp_panel = false;
        if self.show_sftp_panel {
            if self.sftp_last_tab != self.active_tab {
                self.sftp_last_tab = self.active_tab;
                self.sftp_panel.reset();
                self.sftp_panel.request_list_on_open();
            }
            let current_terminal_ref = self
                .active_tab
                .and_then(|idx| self.tabs.get(idx).map(|t| &t.terminal));
            self.sftp_panel.show_side_panel(
                ctx,
                &theme,
                current_terminal_ref,
                &mut close_sftp_panel,
                &mut self.right_dock_outer_left_x,
            );
        }
        if close_sftp_panel {
            self.show_sftp_panel = false;
        }

        // 系统监控：切换终端标签时改为采集当前 SSH 会话（与 SFTP 侧栏一致）
        if self.show_monitor_panel {
            if self.monitor_last_tab != self.active_tab {
                self.monitor_last_tab = self.active_tab;
                self.sync_monitor_panel_to_active_tab();
            }
            self.monitor_panel.show_side_panel(
                ctx,
                &theme,
                &mut self.show_monitor_panel,
                &mut self.right_dock_outer_left_x,
            );
        }

        // egui：须先完成所有左右 SidePanel，再注册底栏，最后 CentralPanel；否则右栏与主区叠绘错位（点击片段后像「全屏花屏」）
        self.show_bottom_chrome(ctx);

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme.bg_body_color())
                    .inner_margin(egui::Margin::ZERO)
                    .outer_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                // egui：CentralPanel 内层 Ui 的 clip_rect 默认同 `screen_rect`，与右侧 `SidePanel`
                // 同属 background 层时，后绘的中央区会把先绘的右栏盖住（侧栏变窄后更像「终端压住片段」）。
                let central_rect = ui.max_rect();
                let mut clip_rect = central_rect.intersect(ui.clip_rect());
                // 多右侧 dock 时：`ui.max_rect()` 偶发比 `Context::available_rect()` 更「宽」，主区会多铺一段 bg_body；
                // 与各侧栏回调里推算的槽位左缘取 **三者最小**，保证裁剪最紧。
                let ctx_avail_right = ctx.available_rect().max.x;
                clip_rect.max.x = clip_rect.max.x.min(ctx_avail_right);
                if let Some(left_x) = self.right_dock_outer_left_x {
                    clip_rect.max.x = clip_rect.max.x.min(left_x);
                }
                ui.set_clip_rect(clip_rect);
                // inner_margin：任何非零都会在侧栏｜终端｜顶/底外侧露出 Central 的 bg_body，看起来像「整块终端缩小」；
                // 侧栏会话卡片自己有圆角内边距，无需再在此处垫一圈。
                // layout_h 必须在此 Frame 的子 Ui 内读取（见前文 full_h / layout_h 说明）。
                egui::Frame::none()
                    .inner_margin(egui::Margin::ZERO)
                    .show(ui, |ui| {
                let layout_h = ui.available_height();
                ui.horizontal(|ui| {
                    // 侧边栏与终端列紧贴分割条即可；item_spacing.x=6 会在侧栏｜拖拽条｜终端之间叠出宽约 16px 的 bg_body「灰竖条」
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    ui.set_height(layout_h);
                    if !self.sidebar_collapsed {
                        let connected_sessions: HashSet<String> = self
                            .tabs
                            .iter()
                            .filter(|t| t.terminal.is_connected())
                            .map(|t| t.session_id.clone())
                            .collect();

                        ui.allocate_ui_with_layout(
                            egui::vec2(self.sidebar_width, layout_h),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                egui::Frame::none()
                                    .fill(theme.bg_window_color())
                                    .rounding(0.0)
                                    .stroke(egui::Stroke::NONE)
                                    .inner_margin(egui::Margin::ZERO)
                                    .show(ui, |ui| {
                                        ui.set_width(self.sidebar_width);
                                        egui::Frame::none()
                                            .fill(theme.border_color())
                                            .rounding(0.0)
                                            .inner_margin(egui::Margin::ZERO)
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.sidebar_search_query)
                                                        .id(Self::id_sidebar_connection_search())
                                                        .hint_text("搜索连接…")
                                                        .text_color(theme.fg_high_color())
                                                        .desired_width(
                                                            layout_util::finite_content_width_inset(
                                                                ui,
                                                                4.0,
                                                                120.0,
                                                                (self.sidebar_width - 28.0).max(96.0),
                                                            ),
                                                        ),
                                                );
                                            });
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                            let tab_row_w = (self.sidebar_width - 24.0).max(96.0);
                                            let item_w = (tab_row_w / 3.0).max(48.0);
                                            for label in ["全部", "在线", "离线"] {
                                                let active = self.sidebar_filter == label;
                                                let text_color = if active {
                                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 200)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 46)
                                                };
                                                let fill = if active {
                                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 128)
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                };
                                                let resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(label).size(theme.font_size_small()).color(text_color),
                                                    )
                                                    .fill(fill)
                                                    .stroke(egui::Stroke::NONE)
                                                    .rounding(theme.radius_status_btn())
                                                    .min_size(egui::vec2(item_w, 20.0)),
                                                );
                                                if resp.clicked() {
                                                    self.sidebar_filter = label.to_string();
                                                }
                                            }
                                        });
                                        let sidebar_output = Sidebar::show(
                                            ui,
                                            &self.session_manager,
                                            &self.selected_session_id,
                                            &self.sidebar_search_query,
                                            &self.sidebar_filter,
                                            &connected_sessions,
                                            &theme,
                                        );

                                        if sidebar_output.create_session_clicked {
                                            self.show_new_session_dialog = true;
                                        }
                                        if sidebar_output.collapse_clicked {
                                            self.sidebar_collapsed = true;
                                            self.sidebar_user_dismissed_responsive = true;
                                        }
                                        if let Some(session_id) = sidebar_output.selected_session_id {
                                            self.select_session(&session_id);
                                        }
                                        if let Some(session_id) = sidebar_output.delete_session_id {
                                            if let Some(s) = self.session_manager.get_session(&session_id) {
                                                self.delete_session_confirm =
                                                    Some((session_id, s.name.clone()));
                                            }
                                        }
                                        if let Some(session_id) = sidebar_output.edit_session_id {
                                            self.open_edit_session_dialog(&session_id);
                                        }
                                        if sidebar_output.response.double_clicked() {
                                            self.sidebar_collapsed = true;
                                            self.sidebar_user_dismissed_responsive = true;
                                        }
                                    });
                            },
                        );
                    } else if ui.button("☰").clicked() {
                        self.sidebar_collapsed = false;
                        self.sidebar_user_dismissed_responsive = false;
                    }

                    if !self.sidebar_collapsed {
                        // 宽 0 无法拖拽；压到 1px 尽量不占位
                        let (drag_rect, drag_resp) = ui.allocate_exact_size(
                            egui::vec2(1.0, layout_h),
                            egui::Sense::drag(),
                        );
                        // 空闲态与终端左缘同色，避免侧栏右侧出现一条「分界线」似的灰竖条；拖拽时仍以高亮色提示
                        let color = if drag_resp.hovered() || drag_resp.dragged() {
                            theme.accent_dim_color()
                        } else {
                            theme.bg_terminal_color()
                        };
                        ui.painter().rect_filled(drag_rect, 0.0, color);
                        if drag_resp.dragged() {
                            let (lo, hi) = layout_util::left_sidebar_drag_clamp(ctx);
                            self.sidebar_width =
                                (self.sidebar_width + drag_resp.drag_delta().x).clamp(lo, hi);
                        }
                    }

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), layout_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            // `top_down` 默认 item_spacing.y 会在标签栏与终端之间露出 Central 的 bg_body，形成「灰条」并易撑出滚动条
                            let saved_col_item_spacing = ui.spacing().item_spacing;
                            ui.spacing_mut().item_spacing.y = 0.0;
                            // README §2.4 标签栏
                            egui::Frame::none()
                                .fill(theme.bg_tab_bar_color())
                                .stroke(egui::Stroke::NONE)
                                .inner_margin(egui::Margin::symmetric(4.0, 0.0))
                                .show(ui, |ui| {
                                    // Frame 背景只画在 content min_rect 外扩 inner_margin 上；不拉满宽整行会露出 bg_body，像标签栏下一条灰
                                    // 勿固定 min_height=36：会在 Tab 行下方垫一行空白，终端顶上像「多一条缝」
                                    ui.set_min_width(ui.available_width());
                                    let prev_padding = ui.spacing().button_padding;
                                    let prev_item_spacing = ui.spacing().item_spacing;
                                    // SPEC §4.3 / §8：Tab 内边距与 Tab 间距（终端区勿动此项）
                                    ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
                                    // `horizontal` 会在剩余高度里纵向居中 Tab，易在 Tab 行上方留出一条 tab_bar 色带
                                    ui.horizontal_top(|ui| {
                                        let mut to_close = None;
                                        let mut close_others = None;
                                        let mut close_right = None;
                                        let mut disconnect_ssh_idx = None;
                                        let mut reconnect_idx = None;
                                        for (idx, tab) in self.tabs.iter().enumerate() {
                                            let active = self.active_tab == Some(idx);
                                            let tab_label = tab.title.clone();
                                            ui.horizontal(|ui| {
                                                let tab_resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(&tab_label).size(theme.font_size_tab_label()).color(
                                                            if active {
                                                                theme.fg_high_color()
                                                            } else {
                                                                theme.fg_low_color()
                                                            },
                                                        ),
                                                    )
                                                    .fill(if active {
                                                        theme.bg_terminal_color()
                                                    } else {
                                                        theme.bg_tab_bar_color()
                                                    })
                                                    .stroke(egui::Stroke::new(1.0, theme.border_color()))
                                                    .rounding(theme.radius_list_item())
                                                    .min_size(egui::vec2(146.0, 28.0)),
                                                );
                                                let dot_color = if tab.terminal.is_connected() {
                                                    egui::Color32::from_rgb(76, 175, 80)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)
                                                };
                                                let dot_pos = egui::pos2(
                                                    tab_resp.rect.left() + 11.0,
                                                    tab_resp.rect.center().y,
                                                );
                                                ui.painter().circle_filled(dot_pos, 2.5, dot_color);
                                                if tab_resp.clicked() {
                                                    self.active_tab = Some(idx);
                                                    self.selected_session_id = Some(tab.session_id.clone());
                                                }
                                                let tab_hovered = tab_resp.hovered();
                                                tab_resp.context_menu(|ui| {
                                                    if tab.terminal.is_connected()
                                                        || tab.terminal.is_connecting()
                                                    {
                                                        if ui.button("断开 SSH（保留输出）").clicked() {
                                                            disconnect_ssh_idx = Some(idx);
                                                            ui.close_menu();
                                                        }
                                                    }
                                                    if ui.button("重连此标签").clicked() {
                                                        reconnect_idx = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                    ui.separator();
                                                    if ui.button("关闭其他标签").clicked() {
                                                        close_others = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("关闭右侧标签").clicked() {
                                                        close_right = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                });
                                                if tab_hovered
                                                    && ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(theme.font_size_title_bar())
                                                                .color(theme.fg_low_color()),
                                                        )
                                                        .fill(egui::Color32::TRANSPARENT)
                                                        .frame(false),
                                                    )
                                                    .on_hover_text("关闭标签")
                                                    .clicked()
                                                {
                                                    to_close = Some(idx);
                                                }
                                            });
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("+")
                                                        .size(theme.font_size_large())
                                                        .color(theme.fg_low_color()),
                                                )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .frame(false),
                                            )
                                            .on_hover_text(
                                                "新标签：左侧选中连接后点此或 ⌘T；无选中时打开新建会话配置",
                                            )
                                            .clicked()
                                        {
                                            if self.selected_session_id.is_some() {
                                                self.open_new_tab_from_selection();
                                            } else {
                                                self.show_new_session_dialog = true;
                                            }
                                        }
                                        if let Some(idx) = to_close {
                                            self.request_close_tab_at(idx);
                                        }
                                        if let Some(idx) = disconnect_ssh_idx {
                                            self.disconnect_ssh_keep_buffer_at(idx);
                                        }
                                        if let Some(idx) = reconnect_idx {
                                            self.reconnect_tab_at(idx);
                                        }
                                        if let Some(idx) = close_others {
                                            if idx < self.tabs.len() {
                                                let kept = self.tabs.remove(idx);
                                                for t in self.tabs.iter_mut() {
                                                    t.terminal.disconnect();
                                                }
                                                self.tabs.clear();
                                                self.tabs.push(kept);
                                                self.active_tab = Some(0);
                                                self.selected_session_id =
                                                    self.tabs.first().map(|t| t.session_id.clone());
                                            }
                                        }
                                        if let Some(idx) = close_right {
                                            if idx + 1 < self.tabs.len() {
                                                for t in self.tabs.iter_mut().skip(idx + 1) {
                                                    t.terminal.disconnect();
                                                }
                                                self.tabs.truncate(idx + 1);
                                                self.active_tab = Some(idx);
                                                self.selected_session_id =
                                                    self.tabs.get(idx).map(|t| t.session_id.clone());
                                            }
                                        }
                                    });
                                    ui.spacing_mut().button_padding = prev_padding;
                                    ui.spacing_mut().item_spacing = prev_item_spacing;
                                });

                            let term_col_w = ui.available_width();
                            if let Some(terminal) = self.current_terminal_mut() {
                                terminal.show(ui, &theme, term_col_w);
                            } else {
                                self.show_welcome(ui);
                            }

                            ui.spacing_mut().item_spacing = saved_col_item_spacing;
                        },
                    );
                });
                    });
            });

        let session_for_fragments = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid).cloned());
        let fragment_cfg = FragmentManager::default_config_path();
        let lib_saved = self.fragment_library.show_window(
            ctx,
            &mut self.fragment_manager,
            &mut self.fragment_sort_by,
            &fragment_cfg,
            session_for_fragments.as_ref(),
            &theme,
        );
        if lib_saved {
            self.fragment_manager.sort(self.fragment_sort_by);
        }

        // 显示新建会话对话框
        if self.show_new_session_dialog {
            let mut open = self.show_new_session_dialog;
            let mut should_close = false;
            egui::Window::new("new_session_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(380.0, 412.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76);
                    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179);
                    let input_stroke = egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                    );
                    let input_fill = egui::Color32::from_rgb(19, 19, 28);
                    let _input_rounding = 4.0;
                    let required_missing =
                        self.new_session_name.trim().is_empty() || self.new_session_host.trim().is_empty();

                    Self::modal_content_frame().show(ui, |ui| {
                            let mut close_via_header = false;
                            Self::modal_header(ui, "新建会话", &mut close_via_header);
                            if close_via_header {
                                self.reset_new_session_form();
                                should_close = true;
                            }

                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                            Self::ui_field_label(ui, "会话名称", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_session_name)
                                        .frame(false)
                                        .hint_text("例: 生产服务器-01")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let host_w = (row_w - 98.0).max(160.0);
                                ui.vertical(|ui| {
                                    ui.set_width(host_w);
                                    Self::ui_field_label(ui, "主机地址", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_host)
                                                .frame(false)
                                                .hint_text("IP 或域名")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(88.0);
                                    Self::ui_field_label(ui, "端口", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add_sized(
                                            [68.0, 20.0],
                                            egui::DragValue::new(&mut self.new_session_port)
                                                .clamp_range(1..=65535)
                                                .speed(1.0),
                                        );
                                    });
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let half = ((row_w - 10.0) / 2.0).max(140.0);
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, "用户名", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_username)
                                                .frame(false)
                                                .hint_text("root")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, "密码", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_password)
                                                .frame(false)
                                                .password(true)
                                                .hint_text("可留空")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                            });

                            Self::ui_field_label(ui, "SSH 私钥路径", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_session_private_key_path)
                                        .frame(false)
                                        .hint_text("~/.ssh/id_rsa（留空则用密码或系统默认密钥）")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            Self::ui_field_label(ui, "分组", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_session_group)
                                        .frame(false)
                                        .hint_text("默认分组")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            if required_missing {
                                ui.add_space(theme.spacing_sm());
                                ui.label(
                                    egui::RichText::new("请先填写会话名称和主机地址")
                                        .size(theme.font_size_panel_title())
                                        .color(egui::Color32::from_rgba_unmultiplied(244, 67, 54, 128)),
                                );
                            }

                            ui.add_space(theme.spacing_list_item_x());
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let save_btn = egui::Button::new(
                                        egui::RichText::new("保存并连接")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgb(102, 126, 234)),
                                    )
                                    .min_size(egui::vec2(104.0, 28.0))
                                    .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 89))
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add_enabled(!required_missing, save_btn).clicked() {
                                        self.create_and_connect_session();
                                        should_close = true;
                                    }
                                    let cancel_btn = egui::Button::new(
                                        egui::RichText::new("取消")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                    )
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add(cancel_btn).clicked() {
                                        self.reset_new_session_form();
                                        should_close = true;
                                    }
                                });
                            });
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                        self.create_and_connect_session();
                        should_close = true;
                    }
                });
            self.show_new_session_dialog = open && !should_close;
        }

        if self.show_about_dialog {
            let mut open = self.show_about_dialog;
            let mut should_close = false;
            egui::Window::new("about_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(420.0, 360.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    Self::modal_content_frame().show(ui, |ui| {
                            Self::modal_header(ui, "关于", &mut should_close);
                            ui.label(
                                egui::RichText::new("MistTerm")
                                    .size(16.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179)),
                            );
                            ui.label(
                                egui::RichText::new("一个现代化 SSH 终端工具")
                                    .size(theme.font_size_panel_title())
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 102)),
                            );
                            ui.add_space(theme.spacing_md());
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                ))
                                .rounding(theme.radius_list_item())
                                .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x(), theme.spacing_search_input_y()))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("版本: v0.1.0")
                                            .size(theme.font_size_panel_title())
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    );
                                    ui.add_space(theme.spacing_panel_gap());
                                    egui::ScrollArea::vertical()
                                        .max_height(200.0)
                                        .show(ui, |ui| {
                                            ui.label(
                                                egui::RichText::new(mistterm_functional_spec_shortcuts())
                                                    .font(egui::FontId::monospace(10.0))
                                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 89)),
                                            );
                                        });
                                });
                            ui.add_space(theme.spacing_list_item_x());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let close_btn = egui::Button::new(
                                    egui::RichText::new("关闭")
                                        .size(theme.font_size_normal())
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                )
                                .min_size(egui::vec2(72.0, 28.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .rounding(theme.radius_list_item());
                                if ui.add(close_btn).clicked() {
                                    should_close = true;
                                }
                            });
                    });
                });
            self.show_about_dialog = open && !should_close;
        }

        if self.show_preferences_dialog {
            let mut open = self.show_preferences_dialog;
            let mut should_close = false;
            let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76);
            let text_low = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 102);
            egui::Window::new("preferences_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(400.0, 400.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    Self::modal_content_frame().show(ui, |ui| {
                        Self::modal_header(ui, "偏好设置", &mut should_close);
                        ui.label(
                            egui::RichText::new(
                                "窗口大小与位置、左侧栏宽度与折叠会在退出时自动保存（§8.1）。",
                            )
                            .size(theme.font_size_small())
                            .color(text_low),
                        );
                        ui.add_space(theme.spacing_md());
                        ui.label(
                            egui::RichText::new("外观")
                                .size(theme.font_size_panel_title())
                                .strong()
                                .color(label_color),
                        );
                        ui.add_space(theme.spacing_panel_gap());
                        let theme_names: Vec<String> = self
                            .theme_manager
                            .list_themes()
                            .iter()
                            .map(|t| t.name.clone())
                            .collect();
                        let current_idx = self.theme_manager.current;
                        for (i, name) in theme_names.into_iter().enumerate() {
                            let label = if i == current_idx {
                                format!("✓ {}", name)
                            } else {
                                name
                            };
                            if ui.button(label).clicked() {
                                self.theme_manager.set_theme_index(i);
                                self.theme_manager.save();
                            }
                        }
                        ui.add_space(theme.spacing_status_bar_x());
                        ui.label(
                            egui::RichText::new("连接")
                                .size(theme.font_size_panel_title())
                                .strong()
                                .color(label_color),
                        );
                        ui.add_space(theme.spacing_panel_gap());
                        let mut ar = self.auto_reconnect_enabled;
                        if ui
                            .checkbox(&mut ar, "网络断开后自动重连（最多 5 次，指数退避）")
                            .on_hover_text(
                                "FUNCTIONAL_SPEC §1.4：默认不自动重连；开启后仅对意外断开生效，手动「断开」不会弹此策略。",
                            )
                            .changed()
                        {
                            self.auto_reconnect_enabled = ar;
                        }
                        ui.add_space(theme.spacing_status_bar_x());
                        ui.label(
                            egui::RichText::new("同步与数据")
                                .size(theme.font_size_panel_title())
                                .strong()
                                .color(label_color),
                        );
                        ui.add_space(theme.spacing_panel_gap());
                        if ui
                            .button(
                                egui::RichText::new("打开云端同步…")
                                    .size(theme.font_size_normal())
                                    .color(egui::Color32::from_rgb(102, 126, 234)),
                            )
                            .clicked()
                        {
                            should_close = true;
                            if Self::right_dock_open_allowed(Self::layout_window_width(ctx)) {
                                self.cloud_sync_panel.open = true;
                            } else {
                                let w = Self::layout_window_width(ctx);
                                self.status_message = format!(
                                    "当前窗口约 {:.0}px，§8 需 ≥ {:.0}px 才能打开右侧「云端同步」面板",
                                    w,
                                    Self::RESP_LAYOUT_WIDE_MIN_PX
                                );
                            }
                        }
                        ui.add_space(theme.spacing_list_item_x());
                        ui.label(
                            egui::RichText::new("其余项请用顶部菜单：视图、工具、帮助。")
                                .size(theme.font_size_small())
                                .color(text_low),
                        );
                        ui.add_space(theme.spacing_list_item_x());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close_btn = egui::Button::new(
                                egui::RichText::new("关闭")
                                    .size(theme.font_size_normal())
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                            )
                            .min_size(egui::vec2(72.0, 28.0))
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .rounding(theme.radius_list_item());
                            if ui.add(close_btn).clicked() {
                                should_close = true;
                            }
                        });
                    });
                });
            self.show_preferences_dialog = open && !should_close;
        }

        if self.large_upload_pending_path.is_some() {
            let path_hint = self
                .large_upload_pending_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let mut open = true;
            #[derive(Clone, Copy)]
            enum LargePick {
                Dismiss,
                Zmodem,
                Scp,
            }
            let mut pick: Option<LargePick> = None;
            egui::Window::new("large_upload_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(440.0, 260.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    Self::modal_content_frame().show(ui, |ui| {
                        let mut should_close_hdr = false;
                        Self::modal_header(ui, "大文件上传", &mut should_close_hdr);
                        if should_close_hdr {
                            pick = Some(LargePick::Dismiss);
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "「{}」≥ 10MB：SCP 无断点续传；ZMODEM 需远端 lrzsz，并将向 PTY 发送 rz -y。",
                                path_hint
                            ))
                            .size(theme.font_size_panel_title())
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179)),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui.button("ZMODEM（推荐大文件）").clicked() {
                                pick = Some(LargePick::Zmodem);
                            }
                            if ui.button("仍用 SCP").clicked() {
                                pick = Some(LargePick::Scp);
                            }
                        });
                        ui.add_space(theme.spacing_md());
                        if ui.button("取消").clicked() {
                            pick = Some(LargePick::Dismiss);
                        }
                    });
                });
            if !open && pick.is_none() {
                pick = Some(LargePick::Dismiss);
            }
            match pick {
                Some(LargePick::Zmodem) => {
                    if let Some(p) = self.large_upload_pending_path.take() {
                        if let Some(t) = self.current_terminal_mut() {
                            t.queue_zmodem_upload_after_rz(p.clone());
                            self.status_message = format!(
                                "已发送 rz -y，握手就绪后以 ZMODEM 上传 {}",
                                p.display()
                            );
                        }
                    }
                }
                Some(LargePick::Scp) => {
                    if let Some(p) = self.large_upload_pending_path.take() {
                        if let Some(t) = self.current_terminal_mut() {
                            match t.start_upload(p.as_path()) {
                                Ok(_) => {
                                    self.status_message =
                                        format!("开始 SCP 上传: {}", p.display());
                                }
                                Err(e) => {
                                    self.status_message =
                                        format!("SCP 上传启动失败: {}", e);
                                }
                            }
                        }
                    }
                }
                Some(LargePick::Dismiss) => {
                    self.large_upload_pending_path = None;
                }
                None => {}
            }
        }

        if let Some((del_id, del_name)) = self.delete_session_confirm.clone() {
            let mut open = true;
            let mut should_close = false;
            let mut do_delete = false;
            egui::Window::new("delete_session_confirm")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(400.0, 200.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    Self::modal_content_frame().show(ui, |ui| {
                        Self::modal_header(ui, "删除会话", &mut should_close);
                        ui.label(
                            egui::RichText::new(format!(
                                "确认删除「{}」的会话配置？此操作不可恢复。",
                                del_name
                            ))
                            .size(theme.font_size_normal())
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179)),
                        );
                        ui.add_space(theme.spacing_lg());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("删除")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgb(239, 83, 80)),
                                    )
                                    .min_size(egui::vec2(72.0, 28.0)),
                                )
                                .clicked()
                            {
                                do_delete = true;
                                should_close = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("取消")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                    )
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item()),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                });
            if do_delete {
                self.delete_session(&del_id);
            }
            if !open || should_close {
                self.delete_session_confirm = None;
            }
        }

        if let Some(pending_idx) = self.close_tab_confirm_idx {
            if pending_idx >= self.tabs.len() {
                self.close_tab_confirm_idx = None;
            } else {
                let tab_title = self.tabs[pending_idx].title.clone();
                let mut open = true;
                let mut should_close = false;
                let mut confirmed = false;
                egui::Window::new("close_tab_confirm")
                    .open(&mut open)
                    .title_bar(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .movable(false)
                    .resizable(false)
                    .collapsible(false)
                    .fixed_size(egui::vec2(400.0, 200.0))
                    .frame(Self::modal_window_frame())
                    .show(ctx, |ui| {
                        Self::modal_content_frame().show(ui, |ui| {
                            Self::modal_header(ui, "关闭标签", &mut should_close);
                            ui.label(
                                egui::RichText::new(format!(
                                    "标签「{}」仍连接或握手中，确定关闭？",
                                    tab_title
                                ))
                                .size(theme.font_size_normal())
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179)),
                            );
                            ui.add_space(theme.spacing_lg());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("关闭")
                                                .size(theme.font_size_normal())
                                                .color(egui::Color32::from_rgb(102, 126, 234)),
                                        )
                                        .min_size(egui::vec2(72.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    confirmed = true;
                                    should_close = true;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new("取消")
                                                .size(theme.font_size_normal())
                                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                        )
                                        .min_size(egui::vec2(72.0, 28.0))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE)
                                        .rounding(theme.radius_list_item()),
                                    )
                                    .clicked()
                                {
                                    should_close = true;
                                }
                            });
                        });
                    });
                if confirmed && pending_idx < self.tabs.len() {
                    self.remove_tab_at(pending_idx);
                }
                if !open || should_close {
                    self.close_tab_confirm_idx = None;
                }
            }
        }

        if self.show_edit_session_dialog {
            let mut open = self.show_edit_session_dialog;
            let mut should_close = false;
            egui::Window::new("edit_session_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(380.0, 412.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76);
                    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179);
                    let input_stroke = egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                    );
                    let input_fill = egui::Color32::from_rgb(19, 19, 28);
                    let _input_rounding = 4.0;
                    let required_missing =
                        self.edit_session_name.trim().is_empty() || self.edit_session_host.trim().is_empty();

                    Self::modal_content_frame().show(ui, |ui| {
                            Self::modal_header(ui, "编辑会话", &mut should_close);

                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                            Self::ui_field_label(ui, "会话名称", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_session_name)
                                        .frame(false)
                                        .hint_text("例: 生产服务器-01")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let host_w = (row_w - 98.0).max(160.0);
                                ui.vertical(|ui| {
                                    ui.set_width(host_w);
                                    Self::ui_field_label(ui, "主机地址", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_host)
                                                .frame(false)
                                                .hint_text("IP 或域名")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(88.0);
                                    Self::ui_field_label(ui, "端口", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add_sized(
                                            [68.0, 20.0],
                                            egui::DragValue::new(&mut self.edit_session_port)
                                                .clamp_range(1..=65535)
                                                .speed(1.0),
                                        );
                                    });
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let half = ((row_w - 10.0) / 2.0).max(140.0);
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, "用户名", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_username)
                                                .frame(false)
                                                .hint_text("root")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, "密码", label_color);
                                    Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_password)
                                                .frame(false)
                                                .password(true)
                                                .hint_text("**** 表示沿用原密码；改为新口令以保存新密码")
                                                .text_color(text_color)
                                                .desired_width(layout_util::finite_content_width(ui)),
                                        );
                                    });
                                });
                            });

                            Self::ui_field_label(ui, "SSH 私钥路径", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_session_private_key_path)
                                        .frame(false)
                                        .hint_text("~/.ssh/id_rsa（留空则用密码或系统默认密钥）")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            Self::ui_field_label(ui, "分组", label_color);
                            Self::ui_input_frame(ui, input_fill, input_stroke, &mut |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_session_group)
                                        .frame(false)
                                        .hint_text("默认分组")
                                        .text_color(text_color)
                                        .desired_width(layout_util::finite_content_width(ui)),
                                );
                            });

                            if required_missing {
                                ui.add_space(theme.spacing_sm());
                                ui.label(
                                    egui::RichText::new("请先填写会话名称和主机地址")
                                        .size(theme.font_size_panel_title())
                                        .color(egui::Color32::from_rgba_unmultiplied(244, 67, 54, 128)),
                                );
                            }

                            ui.add_space(theme.spacing_list_item_x());
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let save_btn = egui::Button::new(
                                        egui::RichText::new("保存")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgb(102, 126, 234)),
                                    )
                                    .min_size(egui::vec2(84.0, 28.0))
                                    .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 89))
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add_enabled(!required_missing, save_btn).clicked() {
                                        self.save_edit_session();
                                        should_close = !self.show_edit_session_dialog;
                                    }
                                    let cancel_btn = egui::Button::new(
                                        egui::RichText::new("取消")
                                            .size(theme.font_size_normal())
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                    )
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add(cancel_btn).clicked() {
                                        should_close = true;
                                    }
                                });
                            });
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                        self.save_edit_session();
                        should_close = !self.show_edit_session_dialog;
                    }
                });
            self.show_edit_session_dialog = open && !should_close;
        }

        if self.show_fragments_dialog {
            let mut open = self.show_fragments_dialog;
            let mut should_close = false;
            egui::Window::new("fragments_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .fixed_size(egui::vec2(380.0, 220.0))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    Self::modal_content_frame().show(ui, |ui| {
                            Self::modal_header(ui, "命令片段", &mut should_close);
                            ui.label(
                                egui::RichText::new("提示：点击底部「命令片段」按钮打开侧边栏面板")
                                    .size(theme.font_size_panel_title())
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                            );
                            ui.add_space(theme.spacing_md());
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                ))
                                .rounding(theme.radius_list_item())
                                .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x(), theme.spacing_search_input_y()))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("📋 命令片段侧边栏提供更丰富的命令分类和快捷操作")
                                            .size(theme.font_size_small())
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                                    );
                                });
                            ui.add_space(theme.spacing_list_item_x());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let close_btn = egui::Button::new(
                                    egui::RichText::new("关闭")
                                        .size(theme.font_size_normal())
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                )
                                .min_size(egui::vec2(72.0, 28.0))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .rounding(theme.radius_list_item());
                                if ui.add(close_btn).clicked() {
                                    should_close = true;
                                }
                            });
                    });
                });
            self.show_fragments_dialog = open && !should_close;
        }

        if self.show_fragment_vars_dialog {
            let mut open = self.show_fragment_vars_dialog;
            let mut should_close = false;
            egui::Window::new("fragment_vars_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(layout_util::fragment_vars_modal_size(ctx))
                .frame(Self::modal_window_frame())
                .show(ctx, |ui| {
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 76);
                    Self::modal_content_frame().show(ui, |ui| {
                            Self::modal_header(ui, "填写片段变量", &mut should_close);
                            ui.add_space(-2.0);
                            ui.label(
                                egui::RichText::new(format!("片段：{}", self.pending_fragment_name))
                                    .size(Self::FRAG_VARS_CAPTION_PX)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                            );
                            ui.add_space(theme.spacing_panel_gap());
                            for (key, value) in &mut self.pending_fragment_vars {
                                ui.label(
                                    egui::RichText::new(format!("<{}>", key))
                                        .size(Self::FRAG_VARS_CAPTION_PX)
                                        .strong()
                                        .color(label_color),
                                );
                                egui::Frame::none()
                                    .fill(egui::Color32::from_rgb(19, 19, 28))
                                    .stroke(egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                                    ))
                                    .rounding(theme.radius_list_item())
                                    .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x(), theme.spacing_search_input_y()))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(value)
                                                .frame(false)
                                                .font(egui::FontId::proportional(
                                                    Self::FRAG_VARS_BODY_PX,
                                                ))
                                                .desired_width(layout_util::finite_content_width(ui))
                                                .text_color(egui::Color32::from_rgba_unmultiplied(
                                                    255, 255, 255, 179,
                                                )),
                                        );
                                    });
                                ui.add_space(theme.spacing_panel_gap());
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("↻ 根据变量重算命令")
                                            .size(Self::FRAG_VARS_BODY_PX)
                                            .color(egui::Color32::from_rgba_unmultiplied(
                                                255, 255, 255, 179,
                                            )),
                                    )
                                    .min_size(egui::vec2(0.0, 28.0)),
                                )
                                .clicked()
                            {
                                self.sync_pending_fragment_command_edit();
                            }
                            ui.label(
                                egui::RichText::new("将要执行（可编辑）")
                                    .size(Self::FRAG_VARS_BODY_PX)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                            );
                            ui.add(
                                egui::TextEdit::multiline(&mut self.pending_fragment_command_edit)
                                    .font(egui::FontId::monospace(Self::FRAG_VARS_MONO_PX))
                                    .desired_width(layout_util::finite_content_width(ui))
                                    .desired_rows(4)
                                    .hint_text("支持 {{ md5(a) }} 等表达式"),
                            );
                            ui.add_space(theme.spacing_sm());
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let insert_label = match self.fragment_vars_completion {
                                        FragmentVarsCompletion::PasteInsertStats => "插入终端",
                                        FragmentVarsCompletion::QuickExecuteSend => "发送命令",
                                    };
                                    let insert_btn = egui::Button::new(
                                        egui::RichText::new(insert_label)
                                            .size(Self::FRAG_VARS_BODY_PX)
                                            .color(egui::Color32::from_rgb(102, 126, 234)),
                                    )
                                    .min_size(egui::vec2(92.0, 28.0))
                                    .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 89))
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add(insert_btn).clicked() {
                                        match self.finalize_pending_fragment_send() {
                                            Ok(filled) => {
                                                match self.fragment_vars_completion {
                                                    FragmentVarsCompletion::PasteInsertStats => {
                                                        if let Some(id) =
                                                            self.pending_fragment_id.clone()
                                                        {
                                                            self.insert_expanded_fragment_with_stats(
                                                                &id, &filled,
                                                            );
                                                        }
                                                    }
                                                    FragmentVarsCompletion::QuickExecuteSend => {
                                                        let start = std::time::Instant::now();
                                                        if let Some(session_id) =
                                                            &self.selected_session_id
                                                        {
                                                            let idx = self
                                                                .active_tab
                                                                .filter(|&i| {
                                                                    i < self.tabs.len()
                                                                        && self.tabs[i].session_id
                                                                            == *session_id
                                                                })
                                                                .or_else(|| {
                                                                    self.tabs.iter().position(|t| {
                                                                        t.session_id == *session_id
                                                                    })
                                                                });
                                                            if let Some(idx) = idx {
                                                                if self.tabs[idx].terminal.is_connected()
                                                                {
                                                                    self.tabs[idx]
                                                                        .terminal
                                                                        .send_command(&filled);
                                                                    if let Some(ref fid) =
                                                                        self.pending_fragment_id
                                                                    {
                                                                        let dur_ms = start
                                                                            .elapsed()
                                                                            .as_millis()
                                                                            .max(1)
                                                                            as u64;
                                                                        self.fragment_manager
                                                                            .record_execution(
                                                                                fid,
                                                                                true,
                                                                                dur_ms,
                                                                            );
                                                                        let _ = self
                                                                            .fragment_manager
                                                                            .save(
                                                                                &FragmentManager::default_config_path(),
                                                                            );
                                                                    }
                                                                } else if let Some(fid) =
                                                                    self.pending_fragment_id.clone()
                                                                {
                                                                    self.insert_fragment_at_tab_index(
                                                                        idx,
                                                                        Some(fid.as_str()),
                                                                        &filled,
                                                                    );
                                                                }
                                                            }
                                                        }
                                                        self.quick_selector.open = false;
                                                    }
                                                }
                                                should_close = true;
                                            }
                                            Err(e) => self.status_message = e,
                                        }
                                    }
                                    let cancel_btn = egui::Button::new(
                                        egui::RichText::new("取消")
                                            .size(Self::FRAG_VARS_BODY_PX)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                                    )
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(theme.radius_list_item());
                                    if ui.add(cancel_btn).clicked() {
                                        should_close = true;
                                    }
                                });
                            });
                        });
                });
            if should_close {
                self.pending_fragment_id = None;
                self.pending_fragment_name.clear();
                self.pending_fragment_command.clear();
                self.pending_fragment_vars.clear();
            }
            self.show_fragment_vars_dialog = open && !should_close;
        }

        // 终端视口搜索（当前屏；与底部快捷栏错开）
        if self.show_terminal_search {
            use egui::*;
            let mut close_search = false;
            let search_bar_w = layout_util::floating_bar_default_width(ctx);
            Window::new("终端搜索")
                .id(Id::new("mistterm_terminal_search"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_BOTTOM, [0.0, -92.0])
                .default_width(search_bar_w)
                .show(ctx, |ui| {
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close_search = true;
                    }
                    self.rebuild_terminal_search_matches();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("查找")
                                .size(theme.font_size_normal())
                                .color(theme.fg_medium_color()),
                        );
                        let resp = ui.add(
                            TextEdit::singleline(&mut self.terminal_search_query)
                                .hint_text("输入关键词…")
                                .desired_width(layout_util::finite_content_width(ui))
                                .text_color(theme.fg_high_color()),
                        );
                        if resp.has_focus()
                            && ui.ctx().input(|i| i.key_pressed(Key::Enter))
                            && !self.terminal_search_matches.is_empty()
                        {
                            self.terminal_search_cur =
                                (self.terminal_search_cur + 1) % self.terminal_search_matches.len();
                        }
                        if ui
                            .checkbox(&mut self.terminal_search_ignore_case, "忽略大小写")
                            .changed()
                        {
                            self.rebuild_terminal_search_matches();
                        }
                        if ui.button("上一个").clicked() {
                            if !self.terminal_search_matches.is_empty() {
                                self.terminal_search_cur = (self.terminal_search_cur
                                    + self.terminal_search_matches.len()
                                    - 1)
                                    % self.terminal_search_matches.len();
                            }
                        }
                        if ui.button("下一个").clicked() {
                            if !self.terminal_search_matches.is_empty() {
                                self.terminal_search_cur =
                                    (self.terminal_search_cur + 1) % self.terminal_search_matches.len();
                            }
                        }
                        if ui.button("关闭").clicked() {
                            close_search = true;
                        }
                    });
                    let n = self.terminal_search_matches.len();
                    let detail = if self.current_terminal().is_none() {
                        "请先打开一个终端标签".to_string()
                    } else if self.terminal_search_query.is_empty() {
                        "输入关键词后自动匹配当前屏幕可见内容（不含卷动历史）".to_string()
                    } else if n == 0 {
                        "无匹配".to_string()
                    } else {
                        let (line, col) = self.terminal_search_matches[self.terminal_search_cur];
                        format!(
                            "第 {} / {} 处 · 行 {} 列 {}",
                            self.terminal_search_cur + 1,
                            n,
                            line,
                            col
                        )
                    };
                    ui.label(RichText::new(detail).size(theme.font_size_panel_title()).color(theme.fg_low_color()));
                });
            if close_search {
                self.show_terminal_search = false;
            }
        }

        // 快速片段选择器
        if self.quick_selector.open {
            use egui::*;
            let qsz = layout_util::centered_window_default_size(ctx, 0.40, 0.48);
            let q_scroll_max = layout_util::dialog_scroll_max_height(ctx, 220.0);
            Window::new("⚡ 快速选择片段")
                .collapsible(false)
                .resizable(true)
                .default_size(qsz)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    // 搜索框
                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        ui.text_edit_singleline(&mut self.quick_selector.search_query);
                    });
                    
                    ui.add_space(theme.spacing_md());
                    
                    // 片段列表
                    egui::ScrollArea::vertical()
                        .max_height(q_scroll_max)
                        .show(ui, |ui| {
                            let fragments: Vec<_> =
                                self.fragment_manager.list().iter().cloned().collect();
                            let search_lower = self.quick_selector.search_query.to_lowercase();
                            
                            for (idx, fragment) in fragments.iter().enumerate() {
                                // 搜索过滤
                                if !search_lower.is_empty() 
                                    && !fragment.title.to_lowercase().contains(&search_lower)
                                    && !fragment.command.to_lowercase().contains(&search_lower) {
                                    continue;
                                }
                                
                                let is_selected = idx == self.quick_selector.selected_index;
                                
                                if ui.selectable_label(is_selected, &fragment.title).clicked() {
                                    // 点击执行
                                    self.execute_fragment(fragment);
                                    self.quick_selector.open = false;
                                }
                            }
                        });
                    
                    ui.add_space(theme.spacing_md());
                    ui.horizontal(|ui| {
                        if ui.button("❌ 取消 (ESC)").clicked() {
                            self.quick_selector.open = false;
                        }
                    });
                });
        }

        // 变量输入对话框（片段库定义的变量；与命令里的 `<pod>` 等占位符可串联）
        if self.variable_dialog.open {
            use egui::*;
            
            let ok_label = if self.variable_dialog.paste_after_fill {
                "✅ 插入终端"
            } else {
                "✅ 执行"
            };

            let var_sz = layout_util::centered_window_default_size(ctx, 0.36, 0.38);
            let scroll_h = layout_util::dialog_scroll_max_height(ctx, 240.0);
            Window::new("📝 输入变量")
                .id(egui::Id::new("mistterm_fragment_variable_dialog"))
                .collapsible(false)
                .resizable(true)
                .default_size(var_sz)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(&self.variable_dialog.fragment_title)
                            .size(Self::FRAG_VARS_BODY_PX)
                            .strong()
                            .color(theme.fg_high_color()),
                    );
                    ui.add_space(theme.spacing_sm());
                    egui::ScrollArea::vertical()
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            if let Some(fragment_id) = &self.variable_dialog.fragment_id {
                                if let Some(fragment) = self.fragment_manager.get(fragment_id) {
                                    for var in &fragment.variables {
                                        ui.label(
                                            egui::RichText::new(&var.description)
                                                .size(Self::FRAG_VARS_BODY_PX)
                                                .color(theme.fg_high_color()),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("占位符 <{}>", var.name))
                                                .size(Self::FRAG_VARS_CAPTION_PX)
                                                .color(theme.fg_low_color()),
                                        );
                                        let value = self
                                            .variable_dialog
                                            .values
                                            .entry(var.name.clone())
                                            .or_insert_with(String::new);
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(19, 19, 28))
                                            .stroke(egui::Stroke::new(
                                                1.0,
                                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                                            ))
                                            .rounding(theme.radius_list_item())
                                            .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x(), theme.spacing_search_input_y()))
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(value)
                                                        .frame(false)
                                                        .font(egui::FontId::proportional(Self::FRAG_VARS_BODY_PX))
                                                        .desired_width(layout_util::finite_content_width(ui))
                                                        .text_color(theme.fg_high_color()),
                                                );
                                            });
                                        ui.add_space(theme.spacing_md());
                                    }
                                    ui.separator();
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("↻ 用上方变量重写命令")
                                                    .size(Self::FRAG_VARS_BODY_PX)
                                                    .color(theme.fg_medium_color()),
                                            )
                                            .min_size(egui::vec2(0.0, 28.0)),
                                        )
                                        .clicked()
                                    {
                                        self.variable_dialog.last_finalize_error = None;
                                        self.variable_dialog.command_edit =
                                            self.build_fragment_command_preview(
                                                fragment,
                                                &self.variable_dialog.values,
                                            );
                                    }
                                    ui.label(
                                        egui::RichText::new("将要执行的命令（可编辑）")
                                            .size(Self::FRAG_VARS_BODY_PX)
                                            .color(theme.fg_medium_color()),
                                    );
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.variable_dialog.command_edit)
                                            .font(egui::FontId::monospace(Self::FRAG_VARS_MONO_PX))
                                            .desired_width(layout_util::finite_content_width(ui))
                                            .desired_rows(5)
                                            .text_color(theme.fg_high_color())
                                            .hint_text("可先填变量再点 ↻ 同步；{{ … }} 为表达式，见片段库帮助"),
                                    );
                                }
                            }
                        });
                    if let Some(ref err) = self.variable_dialog.last_finalize_error {
                        ui.add_space(theme.spacing_panel_gap());
                        ui.label(
                            egui::RichText::new(err)
                                .size(Self::FRAG_VARS_CAPTION_PX)
                                .color(egui::Color32::from_rgb(255, 138, 128)),
                        );
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(12.0, 0.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("❌ 取消")
                                        .size(Self::FRAG_VARS_BODY_PX)
                                        .color(theme.fg_low_color()),
                                )
                                .min_size(egui::vec2(88.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.variable_dialog.open = false;
                            self.variable_dialog.paste_after_fill = false;
                            self.variable_dialog.last_finalize_error = None;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(ok_label)
                                        .size(Self::FRAG_VARS_BODY_PX)
                                        .color(theme.fg_high_color()),
                                )
                                .min_size(egui::vec2(108.0, 30.0))
                                .fill(theme.accent_color()),
                            )
                            .clicked()
                        {
                            let paste = self.variable_dialog.paste_after_fill;
                            if let Some(fid) = self.variable_dialog.fragment_id.clone() {
                                if let Some(fragment) = self.fragment_manager.get(&fid).cloned() {
                                    match self.finalize_fragment_command_text(
                                        &self.variable_dialog.command_edit,
                                        &self.variable_dialog.values,
                                    ) {
                                        Ok(cmd) => {
                                            self.variable_dialog.last_finalize_error = None;
                                            let needs = placeholders_needing_user(&cmd);
                                            if needs.is_empty() {
                                                if paste {
                                                    self.insert_expanded_fragment_with_stats(&fid, &cmd);
                                                } else if let Some(session_id) =
                                                    &self.selected_session_id
                                                {
                                                    if let Some(tab) = self
                                                        .tabs
                                                        .iter_mut()
                                                        .find(|t| t.session_id == *session_id)
                                                    {
                                                        let _ = tab.terminal.send_command(&cmd);
                                                    }
                                                    self.quick_selector.open = false;
                                                }
                                            } else {
                                                self.pending_fragment_id = Some(fid.clone());
                                                self.pending_fragment_name = fragment.title.clone();
                                                self.pending_fragment_command = cmd;
                                                self.pending_fragment_vars = needs
                                                    .into_iter()
                                                    .map(|k| (k, String::new()))
                                                    .collect();
                                                self.fragment_vars_completion = if paste {
                                                    FragmentVarsCompletion::PasteInsertStats
                                                } else {
                                                    FragmentVarsCompletion::QuickExecuteSend
                                                };
                                                self.sync_pending_fragment_command_edit();
                                                self.show_fragment_vars_dialog = true;
                                            }
                                            self.variable_dialog.paste_after_fill = false;
                                            self.variable_dialog.open = false;
                                        }
                                        Err(e) => {
                                            self.status_message = e.clone();
                                            self.variable_dialog.last_finalize_error = Some(e);
                                        }
                                    }
                                } else {
                                    self.status_message =
                                        "找不到该片段（可能已从库中删除）".to_string();
                                }
                            }
                        }
                    });
                });
            ctx.move_to_top(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("mistterm_fragment_variable_dialog"),
            ));
        }
    }
}

impl MistTermApp {
    /// 执行命令片段（⌘J 快速选择）：会话占位符展开；片段库变量与 `<自定义>` 占位符弹窗填写。
    fn execute_fragment(&mut self, fragment: &FragmentStats) {
        if self.selected_session_id.is_none() {
            self.status_message = "请先选择左侧会话".to_string();
            return;
        }

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
        let s = ui.available_size();
        if s.x.is_finite() && s.y.is_finite() && s.x > 0.0 && s.y > 0.0 {
            ui.set_min_size(s);
        }
        ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            ui.heading("欢迎使用 MistTerm");
            ui.separator();
            ui.colored_label(
                ui.style().visuals.selection.bg_fill,
                "🚀 快速开始"
            );
            ui.horizontal(|ui| {
                ui.label("1. 点击左侧");
                ui.label("➕ 创建新会话");
            });
            ui.horizontal(|ui| {
                ui.label("2. 选择会话");
                ui.label("🔌 建立连接");
            });
            ui.horizontal(|ui| {
                ui.label("3. 使用");
                ui.label("rz/sz");
                ui.label("进行文件传输");
            });
            ui.horizontal(|ui| {
                ui.label("自建命令片段：菜单「工具 → 命令片段库」或右侧栏「➕ 新建」");
            });
            ui.separator();
            ui.small("提示：双击侧边栏可以折叠/展开");
        });
    }
}

#[cfg(test)]
mod responsive_layout_tests {
    use super::MistTermApp;

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
