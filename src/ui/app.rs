//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等

use eframe::egui;
use rfd::FileDialog;
use std::collections::HashSet;
use std::time::Instant;
use crate::core::{
    Credential, CredentialAuthKind, expand_command_template, SessionManager, FragmentManager, FragmentStats, SortBy,
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

struct TerminalTab {
    session_id: String,
    title: String,
    terminal: TerminalView,
}

/// 变量输入对话框状态
#[derive(Clone, Debug, Default)]
pub struct FragmentVariableDialog {
    pub open: bool,
    pub fragment_id: Option<String>,
    pub fragment_title: String,
    pub values: std::collections::HashMap<String, String>,
}

/// 快速片段选择器状态
#[derive(Clone, Debug, Default)]
pub struct FragmentQuickSelector {
    pub open: bool,
    pub search_query: String,
    pub selected_index: usize,
}

/// 片段面板筛选状态
#[derive(Clone, Debug)]
pub struct FragmentPanelState {
    pub category_filter: Option<String>,
}

impl Default for FragmentPanelState {
    fn default() -> Self {
        Self {
            category_filter: None,
        }
    }
}

/// 主应用程序
pub struct MistTermApp {
    /// 会话管理器
    session_manager: SessionManager,
    
    /// 当前选中的会话 ID
    selected_session_id: Option<String>,
    
    /// 侧边栏状态
    sidebar_collapsed: bool,
    sidebar_width: f32,
    
    /// 终端标签页
    tabs: Vec<TerminalTab>,
    active_tab: Option<usize>,
    
    /// 状态栏信息
    status_message: String,
    
    /// 是否显示新建会话对话框
    show_new_session_dialog: bool,
    show_edit_session_dialog: bool,
    show_about_dialog: bool,
    show_fragments_dialog: bool,
    show_fragment_panel: bool,  // 命令片段侧边栏
    show_git_sync_panel: bool,  // Git 同步面板
    show_monitor_panel: bool,   // 监控面板
    show_sftp_panel: bool,       // SFTP 文件浏览器
    /// 上次已同步 SFTP 列表的终端标签索引（切换标签时重置远端浏览状态）
    sftp_last_tab: Option<usize>,
    
    /// 新建会话表单
    new_session_name: String,
    new_session_host: String,
    new_session_port: u16,
    new_session_username: String,
    new_session_password: String,
    new_session_group: String,

    edit_session_id: Option<String>,
    edit_session_name: String,
    edit_session_host: String,
    edit_session_port: u16,
    edit_session_username: String,
    edit_session_password: String,
    edit_session_group: String,
    sidebar_search_query: String,
    fragment_search_query: String,
    
    /// 命令片段管理器
    fragment_manager: FragmentManager,
    /// 片段排序方式
    fragment_sort_by: SortBy,
    /// 片段面板使用统计跟踪：记录插入时的 Instant
    fragment_pending_ids: Vec<(String, Instant)>,
    /// 片段面板状态（分类筛选等）
    fragment_panel_state: FragmentPanelState,
    /// 变量输入对话框
    variable_dialog: FragmentVariableDialog,
    /// 快速片段选择器
    quick_selector: FragmentQuickSelector,

    /// 主题管理器
    theme_manager: ThemeManager,
}

impl MistTermApp {
    /// 应用当前主题（由 ThemeManager 统一管理）
    fn apply_current_theme(&self, ctx: &egui::Context) {
        self.theme_manager.apply_theme(ctx);
    }

    /// 创建新的应用实例
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let session_manager = SessionManager::new();
        let sessions = session_manager.list_sessions();
        
        // 自动选择第一个会话
        let selected_session_id = sessions.first().map(|s| s.id.clone());

        Self {
            session_manager,
            selected_session_id,
            sidebar_collapsed: false,
            sidebar_width: 240.0,
            tabs: Vec::new(),
            active_tab: None,
            status_message: "就绪".to_string(),
            show_new_session_dialog: false,
            show_edit_session_dialog: false,
            show_about_dialog: false,
            show_fragments_dialog: false,
            show_fragment_panel: false,
            show_git_sync_panel: false,
            show_monitor_panel: false,
            show_sftp_panel: false,
            sftp_last_tab: None,
            git_sync_panel: GitSyncPanel::new(),
            monitor_panel: MonitorPanel::new(),
            sftp_panel: SftpPanel::new(),
            fragment_library: FragmentLibraryState::new(),
            credential_panel: CredentialPanel::new(),
            cloud_sync_panel: CloudSyncPanel::new(),
            new_session_name: String::new(),
            new_session_host: String::new(),
            new_session_port: 22,
            new_session_username: String::new(),
            new_session_password: String::new(),
            new_session_group: "默认".to_string(),
            edit_session_id: None,
            edit_session_name: String::new(),
            edit_session_host: String::new(),
            edit_session_port: 22,
            edit_session_username: String::new(),
            edit_session_password: String::new(),
            edit_session_group: "默认".to_string(),
            sidebar_search_query: String::new(),
            fragment_search_query: String::new(),
            fragment_manager: FragmentManager::load(&FragmentManager::default_config_path())
                .unwrap_or_else(|_| FragmentManager::new()),
            fragment_sort_by: SortBy::UsageCount,
            fragment_pending_ids: Vec::new(),
            fragment_panel_state: FragmentPanelState::default(),
            variable_dialog: FragmentVariableDialog::default(),
            quick_selector: FragmentQuickSelector::default(),
            theme_manager: ThemeManager::load(),
        }
    }

    fn current_terminal_mut(&mut self) -> Option<&mut TerminalView> {
        let idx = self.active_tab?;
        self.tabs.get_mut(idx).map(|t| &mut t.terminal)
    }

    fn current_terminal(&self) -> Option<&TerminalView> {
        let idx = self.active_tab?;
        self.tabs.get(idx).map(|t| &t.terminal)
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
            let mut terminal = TerminalView::new();
            terminal.connect(
                &session.host,
                session.port,
                &session.username,
                &session.password,
            );
            self.tabs.push(TerminalTab {
                session_id: session.id.clone(),
                title: format!("{}@{}", session.username, session.host),
                terminal,
            });
            self.active_tab = Some(self.tabs.len() - 1);
            self.session_manager.mark_session_connected(&session.id);
            self.status_message = format!("正在连接：{}", session.name);
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
        );

        // 选择会话
        self.selected_session_id = Some(session.id.clone());
        
        // 创建终端并连接
        let mut terminal = TerminalView::new();
        terminal.connect(
            &self.new_session_host,
            self.new_session_port,
            &self.new_session_username,
            &self.new_session_password,
        );
        self.tabs.push(TerminalTab {
            session_id: session.id.clone(),
            title: format!("{}@{}", self.new_session_username, self.new_session_host),
            terminal,
        });
        self.active_tab = Some(self.tabs.len() - 1);
        self.session_manager.mark_session_connected(&session.id);
        
        self.status_message = format!("正在连接：{}", self.new_session_name);
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
    }

    /// 删除会话
    pub fn delete_session(&mut self, session_id: &str) {
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
        self.status_message = format!("已删除会话：{}", session_id);
    }

    fn open_edit_session_dialog(&mut self, session_id: &str) {
        if let Some(session) = self.session_manager.get_session(session_id).cloned() {
            self.edit_session_id = Some(session.id);
            self.edit_session_name = session.name;
            self.edit_session_host = session.host;
            self.edit_session_port = session.port;
            self.edit_session_username = session.username;
            self.edit_session_password = session.password;
            self.edit_session_group = session.group;
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

        let updated = self.session_manager.update_session(
            &session_id,
            &self.edit_session_name,
            &self.edit_session_host,
            self.edit_session_port,
            &self.edit_session_username,
            &self.edit_session_password,
            &self.edit_session_group,
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
    fn show_fragment_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("fragment_panel")
            .default_width(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("⚡ 命令片段");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 排序按钮
                        let sort_label = match self.fragment_sort_by {
                            SortBy::UsageCount => "🔢 次数",
                            SortBy::SuccessRate => "✅ 成功率",
                            SortBy::LastUsed => "🕐 最近",
                            SortBy::Name => "🔤 名称",
                        };
                        if ui.button(sort_label).clicked() {
                            // 循环切换排序方式
                            self.fragment_sort_by = match self.fragment_sort_by {
                                SortBy::UsageCount => SortBy::SuccessRate,
                                SortBy::SuccessRate => SortBy::LastUsed,
                                SortBy::LastUsed => SortBy::Name,
                                SortBy::Name => SortBy::UsageCount,
                            };
                            self.fragment_manager.sort(self.fragment_sort_by);
                        }
                    });
                });
                ui.separator();
                
                // 搜索框
                ui.add(
                    egui::TextEdit::singleline(&mut self.fragment_search_query)
                        .hint_text("搜索片段...")
                        .desired_width(f32::INFINITY)
                );
                ui.add_space(4.0);
                
                // 分类筛选
                ui.horizontal(|ui| {
                    ui.label("📂 分类：");
                    
                    // 获取所有分类
                    let mut categories = self.fragment_manager.get_categories();
                    categories.sort();
                    categories.dedup();
                    
                    let mut selected = self.fragment_panel_state.category_filter.clone();
                    
                    let mut current_text = selected.clone().unwrap_or_else(|| "全部".to_string());
                    
                    egui::ComboBox::from_id_source("category_filter")
                        .selected_text(&current_text)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(selected.is_none(), "全部").clicked() {
                                self.fragment_panel_state.category_filter = None;
                            }
                            for cat in categories {
                                if ui.selectable_label(
                                    selected.as_deref() == Some(&cat),
                                    &cat
                                ).clicked() {
                                    self.fragment_panel_state.category_filter = Some(cat);
                                }
                            }
                        });
                });

                ui.add_space(8.0);
                
                ui.vertical(|ui| {
                    // 根据搜索和分类显示片段
                    let search_lower = self.fragment_search_query.to_lowercase();
                    let category_filter = &self.fragment_panel_state.category_filter;
                    
                    let all_fragments = self.fragment_manager.get_all();
                    let filtered_fragments: Vec<&FragmentStats> = all_fragments
                        .iter()
                        .filter(|f| {
                            // 搜索过滤
                            let search_match = search_lower.is_empty() 
                                || f.title.to_lowercase().contains(&search_lower)
                                || f.command.to_lowercase().contains(&search_lower);
                            
                            // 分类过滤
                            let category_match = category_filter.is_none() 
                                || category_filter.as_deref() == Some(&f.category);
                            
                            search_match && category_match
                        })
                        .collect();
                    
                    let categories = self.fragment_manager.get_categories();
                    for category in &categories {
                        // 先检查该分类下是否有符合筛选条件的片段
                        let has_fragments_in_category = filtered_fragments
                            .iter()
                            .any(|f| f.category == *category);
                        
                        if !has_fragments_in_category {
                            continue;
                        }
                        
                        let category_fragments: Vec<&FragmentStats> = filtered_fragments
                            .iter()
                            .filter(|f| f.category == *category)
                            .cloned()
                            .collect();
                        
                        let category_emoji = match category.as_str() {
                            "系统监控" => "📊",
                            "进程管理" => "🔄",
                            "网络" => "🌐",
                            "Docker" => "🐳",
                            "Nginx" => "🌍",
                            _ => "📁",
                        };
                        
                        ui.collapsing(format!("{} {}", category_emoji, category), |ui| {
                            for frag in category_fragments {
                                // 在每个片段项中添加统计显示
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        let button = ui.button(frag.title.as_str());
                                        if button.clicked() {
                                            let id = frag.id.clone();
                                            let command = frag.command.clone();
                                            self.insert_fragment_with_stats(&id, &command);
                                        }
                                        if button.hovered() {
                                            button.on_hover_text(&frag.command);
                                        }
                                        
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // 统计信息
                                            let success_rate = if frag.usage_count > 0 {
                                                (frag.success_count as f32 / frag.usage_count as f32) * 100.0
                                            } else {
                                                0.0
                                            };
                                            
                                            ui.label(
                                                egui::RichText::new(format!("🔢{}次 ✅{:.0}%", frag.usage_count, success_rate))
                                                    .small()
                                                    .color(self.theme_manager.current_theme().fg_low_color())
                                            );
                                        });
                                    });
                                    
                                    // 命令预览
                                    ui.label(
                                        egui::RichText::new(&frag.command)
                                            .small()
                                            .color(self.theme_manager.current_theme().fg_low_color())
                                    );
                                });
                            }
                        });
                        
                        ui.add_space(2.0);
                    }
                });
            });
    }

    /// 向当前标签页插入命令片段并记录统计
    fn insert_fragment_with_stats(&mut self, id: &str, command: &str) {
        let session = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid));
        let expanded = expand_command_template(command, session, &std::collections::HashMap::new());
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(idx) {
                match tab.terminal.insert_fragment(&expanded) {
                    Ok(_) => {
                        // 记录成功，耗时估算为 0（实际耗时无法精确测量）
                        self.fragment_manager.record_usage(id, true, 0);
                        let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                        self.status_message = format!("插入命令：{}", expanded);
                    }
                    Err(e) => {
                        self.fragment_manager.record_usage(id, false, 0);
                        let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                        self.status_message = format!("插入失败：{}", e);
                    }
                }
            }
        } else {
            self.status_message = "没有活动的终端标签页".to_string();
        }
    }

    /// 显示 Git 同步面板
    fn show_git_sync_panel(&mut self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        egui::SidePanel::right("git_sync_panel")
            .default_width(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.git_sync_panel.show(ui, theme);
            });
    }

    /// 向当前标签页插入命令片段
    fn insert_fragment_to_active_tab(&mut self, command: &str) {
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(idx) {
                tab.terminal.insert_fragment(command);
                self.status_message = format!("插入命令：{}", command);
            }
        } else {
            self.status_message = "没有活动的终端标签页".to_string();
        }
    }

    /// README §2.4 状态徽章：淡色底，内边距 2px 8px，圆角 4px，11px 高对比字色
    fn status_chip(ui: &mut egui::Ui, text: &str, theme: &crate::ui::theme::Theme) {
        let c = theme.fg_high_color();
        let [r, g, b, _] = c.to_array();
        let fill = egui::Color32::from_rgba_unmultiplied(r, g, b, 51);
        egui::Frame::none()
            .fill(fill)
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(8.0, 2.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(11.0)
                        .color(theme.fg_high_color()),
                );
            });
    }

    /// 底部快捷栏 + 状态栏合并为 **一个** TopBottomPanel，避免两个 bottom 叠绘挡住紫色条（README §2.3）
    fn show_bottom_chrome(&mut self, ctx: &egui::Context) {
        const QUICK_H: f32 = 44.0;
        const STATUS_H: f32 = 28.0;

        let theme = self.theme_manager.current_theme();
        let bar_fill = theme.bg_window_color();
        let btn_idle = theme.border_color();
        let btn_primary = theme.accent_color();
        let status_bar_bg = theme.accent_color();
        let h_btn = 32.0;

        egui::TopBottomPanel::bottom("bottom_chrome")
            .exact_height(QUICK_H + STATUS_H)
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                // 上：快捷操作栏（固定 44px，避免被内边距挤压）
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
                        ui.add_space(10.0);
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                        ui.horizontal(|ui| {
                            let mk = |label: &str, fill: egui::Color32, w: f32| {
                                egui::Button::new(
                                    egui::RichText::new(label)
                                        .size(12.0)
                                        .color(theme.fg_high_color()),
                                )
                                .fill(fill)
                                .rounding(4.0)
                                .min_size(egui::vec2(w, h_btn))
                            };

                            if ui
                                .add(mk("📋 命令片段", btn_primary, 108.0))
                                .on_hover_text("⌘J")
                                .clicked()
                            {
                                self.show_fragment_panel = !self.show_fragment_panel;
                            }
                            if ui.add(mk("📤 上传", btn_idle, 88.0)).clicked() {
                                if let Some(terminal) = self.current_terminal_mut() {
                                    if let Some(path) = FileDialog::new().pick_file() {
                                        match terminal.start_upload(path.as_path()) {
                                            Ok(_) => {
                                                self.status_message =
                                                    format!("开始上传: {}", path.display())
                                            }
                                            Err(e) => {
                                                self.status_message = format!("上传失败: {}", e)
                                            }
                                        }
                                    }
                                }
                            }
                            if ui.add(mk("📥 下载/SFTP", btn_idle, 120.0))
                                .on_hover_text("打开 SFTP；ZMODEM 默认目录见侧栏提示")
                                .clicked()
                            {
                                self.show_sftp_panel = true;
                                self.sftp_last_tab = None;
                                self.sftp_panel.request_list_on_open();
                                if let Some(terminal) = self.current_terminal() {
                                    self.status_message = format!(
                                        "SFTP 已打开；ZMODEM 下载目录 {}",
                                        terminal.download_dir()
                                    );
                                }
                            }
                            if ui.add(mk("🔍 搜索", btn_idle, 88.0)).clicked() {
                                self.status_message = "终端搜索（开发中）".to_string();
                            }
                            if ui.add(mk("⚙️ 设置", btn_idle, 88.0)).clicked() {
                                self.status_message = "设置（开发中）".to_string();
                            }
                            if ui.add(mk("🔀 Git", btn_idle, 88.0)).on_hover_text("Git 同步面板").clicked() {
                                self.show_git_sync_panel = !self.show_git_sync_panel;
                            }
                            if ui.add(mk("📊 监控", btn_idle, 88.0)).on_hover_text("系统监控面板").clicked() {
                                self.show_monitor_panel = !self.show_monitor_panel;
                            }
                            if ui
                                .add(mk("🔐 凭证", btn_idle, 88.0))
                                .on_hover_text("加密凭证库")
                                .clicked()
                            {
                                self.credential_panel.open = !self.credential_panel.open;
                            }
                            if ui
                                .add(mk("☁️ 同步", btn_idle, 88.0))
                                .on_hover_text("云端同步 / 导出包")
                                .clicked()
                            {
                                self.cloud_sync_panel.open = !self.cloud_sync_panel.open;
                            }
                            if ui
                                .add(mk("📂 SFTP", btn_idle, 88.0))
                                .on_hover_text("显示/隐藏远端 SFTP 侧栏")
                                .clicked()
                            {
                                self.show_sftp_panel = !self.show_sftp_panel;
                                if self.show_sftp_panel {
                                    self.sftp_last_tab = None;
                                    self.sftp_panel.request_list_on_open();
                                }
                            }
                        });
                    },
                );

                // 下：状态栏（固定 28px，贴底）
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), STATUS_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0.0, status_bar_bg);
                        ui.add_space(10.0);

                        let mut server_line = "🖥️ 未选择会话".to_string();
                        let mut font_px = "14px".to_string();
                        let mut duration_chip = "—".to_string();

                        if let Some(terminal) = self.current_terminal() {
                            server_line = format!("🖥️ {}", terminal.connection_server_text());
                            font_px = format!("{:.0}px", terminal.font_size());
                            duration_chip = if let Some(err) = terminal.connection_error_text() {
                                truncate_status(err, 28)
                            } else if terminal.is_connected() {
                                terminal.connection_duration_text()
                            } else {
                                "连接中…".to_string()
                            };
                        }

                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&server_line)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme.fg_high_color()),
                            );
                            ui.label(
                                egui::RichText::new("🔒 SSH-2.0")
                                    .monospace()
                                    .size(11.0)
                                    .color(theme.fg_high_color()),
                            );
                            ui.label(
                                egui::RichText::new("🌐 Asia/Shanghai")
                                    .monospace()
                                    .size(11.0)
                                    .color(theme.fg_high_color()),
                            );
                            ui.add_space(4.0);
                            Self::status_chip(ui, "UTF-8", theme);
                            Self::status_chip(ui, &font_px, theme);
                            Self::status_chip(ui, &duration_chip, theme);
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

/// 状态栏错误摘要，避免撑爆单行
fn truncate_status(s: &str, max_chars: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max_chars {
        return format!("❌ {}", t);
    }
    format!("❌ {}…", t.chars().take(max_chars.saturating_sub(1)).collect::<String>())
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Ctrl+J 快捷键：打开快速片段选择器
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::J)) {
            self.quick_selector.open = true;
        }

        self.apply_current_theme(ctx);
        let theme = self.theme_manager.current_theme();

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

        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
            self.show_new_session_dialog = true;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::T)) {
            self.show_new_session_dialog = true;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::J)) {
            self.show_fragment_panel = !self.show_fragment_panel;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            if let Some(terminal) = self.current_terminal_mut() {
                terminal.clear_screen();
                self.status_message = "已清空终端".to_string();
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::W)) {
            if let Some(idx) = self.active_tab {
                self.tabs.remove(idx);
                self.active_tab = self.tabs.len().checked_sub(1);
                self.selected_session_id = self
                    .active_tab
                    .and_then(|i| self.tabs.get(i))
                    .map(|t| t.session_id.clone());
            }
        }

        // 顶部标题栏
        egui::TopBottomPanel::top("title_bar")
            .exact_height(36.0)
            .frame(egui::Frame::none().fill(theme.bg_tab_bar_color()))
            .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("文件", |ui| {
                    if ui.button("新建会话 ⌘N").clicked() {
                        self.show_new_session_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("关闭标签 ⌘W").clicked() {
                        if let Some(idx) = self.active_tab {
                            self.tabs.remove(idx);
                            self.active_tab = self.tabs.len().checked_sub(1);
                            self.selected_session_id = self.active_tab.and_then(|i| self.tabs.get(i)).map(|t| t.session_id.clone());
                        }
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
                        ui.close_menu();
                    }
                    ui.separator();
                    let sftp_menu = if self.show_sftp_panel {
                        "✓ SFTP 文件面板"
                    } else {
                        "SFTP 文件面板"
                    };
                    if ui.button(sftp_menu).clicked() {
                        self.show_sftp_panel = !self.show_sftp_panel;
                        if self.show_sftp_panel {
                            self.sftp_last_tab = None;
                            self.sftp_panel.request_list_on_open();
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.menu_button("主题", |ui| {
                        for (i, theme) in self.theme_manager.list_themes().iter().enumerate() {
                            let is_current = i == self.theme_manager.current;
                            let label = if is_current {
                                format!("✓ {}", theme.name)
                            } else {
                                theme.name.clone()
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
                        self.credential_panel.open = true;
                        ui.close_menu();
                    }
                    if ui.button("云端同步").clicked() {
                        self.cloud_sync_panel.open = true;
                        ui.close_menu();
                    }
                });
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        self.show_about_dialog = true;
                        ui.close_menu();
                    }
                });
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let title = self
                        .current_terminal()
                        .map(|t| format!("MistTerm - {}", t.connection_server_text()))
                        .unwrap_or_else(|| "MistTerm".to_string());
                    // README §2.4 标题栏：13px
                    ui.label(
                        egui::RichText::new(title)
                            .size(13.0)
                            .color(theme.fg_low_color()),
                    );
                });
            });
        });

        // 底部快捷栏 + 状态栏：单面板纵向排布，避免两个 bottom 叠绘挡住状态栏
        self.show_bottom_chrome(ctx);

        // 命令片段面板
        if self.show_fragment_panel {
            self.show_fragment_panel(ctx);
        }

        // Git 同步面板
        if self.show_git_sync_panel {
            self.show_git_sync_panel(ctx, theme);
        }

        let mut cred_action: Option<CredentialPanelAction> = None;
        if self.credential_panel.open {
            if self
                .credential_panel
                .show_side_panel(ctx, theme, &mut cred_action)
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
        self.cloud_sync_panel.show(ctx, theme, &mut deps);

        if let Some(action) = cred_action {
            if let CredentialPanelAction::UseForQuickConnect(c) = action {
                self.apply_credential_to_new_session_form(c);
            }
        }

        // SFTP（右侧面板；切换终端标签时重置远端路径并重新拉列表）
        let mut close_sftp_panel = false;
        if self.show_sftp_panel {
            if self.sftp_last_tab != self.active_tab {
                self.sftp_last_tab = self.active_tab;
                self.sftp_panel.reset();
                self.sftp_panel.request_list_on_open();
            }
            self.sftp_panel.show_side_panel(
                ctx,
                theme,
                self.current_terminal(),
                &mut close_sftp_panel,
            );
        }
        if close_sftp_panel {
            self.show_sftp_panel = false;
        }

        // 系统监控面板
        if self.show_monitor_panel {
            self.show_monitor_panel(ctx);
        }

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
            theme,
        );
        if lib_saved {
            self.fragment_manager.sort(self.fragment_sort_by);
        }

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme.bg_body_color()))
            .show(ctx, |ui| {
                let full_h = ui.available_height();
                ui.horizontal(|ui| {
                    ui.set_height(full_h);
                    if !self.sidebar_collapsed {
                        let connected_sessions: HashSet<String> = self
                            .tabs
                            .iter()
                            .filter(|t| t.terminal.is_connected())
                            .map(|t| t.session_id.clone())
                            .collect();

                        ui.allocate_ui_with_layout(
                            egui::vec2(self.sidebar_width, full_h),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                egui::Frame::none()
                                    .fill(theme.bg_window_color())
                                    .stroke(egui::Stroke::new(1.0, theme.border_color()))
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.set_width(self.sidebar_width);
                                        // README §2.4 搜索框：高 36px、内边距 8px 12px、背景 #3c3c3c、圆角 6px
                                        egui::Frame::none()
                                            .fill(theme.border_color())
                                            .rounding(6.0)
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.sidebar_search_query)
                                                        .hint_text("搜索连接...")
                                                        .text_color(theme.fg_high_color())
                                                        .desired_width(f32::INFINITY),
                                                );
                                            });
                                        ui.add_space(8.0);
                                        let sidebar_output = Sidebar::show(
                                            ui,
                                            &self.session_manager,
                                            &self.selected_session_id,
                                            &self.sidebar_search_query,
                                            &connected_sessions,
                                            theme,
                                        );

                                        if sidebar_output.create_session_clicked {
                                            self.show_new_session_dialog = true;
                                        }
                                        if let Some(session_id) = sidebar_output.selected_session_id {
                                            self.select_session(&session_id);
                                        }
                                        if let Some(session_id) = sidebar_output.delete_session_id {
                                            self.delete_session(&session_id);
                                        }
                                        if let Some(session_id) = sidebar_output.edit_session_id {
                                            self.open_edit_session_dialog(&session_id);
                                        }
                                        if sidebar_output.response.double_clicked() {
                                            self.sidebar_collapsed = true;
                                        }
                                    });
                            },
                        );
                    } else if ui.button("☰").clicked() {
                        self.sidebar_collapsed = false;
                    }

                    if !self.sidebar_collapsed {
                        let (drag_rect, drag_resp) = ui.allocate_exact_size(
                            egui::vec2(4.0, full_h),
                            egui::Sense::drag(),
                        );
                        let color = if drag_resp.hovered() || drag_resp.dragged() {
                            theme.accent_dim_color()
                        } else {
                            theme.border_color()
                        };
                        ui.painter().rect_filled(drag_rect, 0.0, color);
                        if drag_resp.dragged() {
                            self.sidebar_width =
                                (self.sidebar_width + drag_resp.drag_delta().x).clamp(180.0, 400.0);
                        }
                    } else {
                        ui.add_space(6.0);
                    }

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), full_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            // README §2.4 标签栏
                            egui::Frame::none()
                                .fill(theme.bg_tab_bar_color())
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    theme.border_color(),
                                ))
                                .inner_margin(egui::Margin::symmetric(8.0, 0.0))
                                .show(ui, |ui| {
                                    ui.set_min_height(40.0);
                                    let prev_padding = ui.spacing().button_padding;
                                    ui.spacing_mut().button_padding = egui::vec2(12.0, 6.0);
                                    ui.horizontal_wrapped(|ui| {
                                        let mut to_close = None;
                                        let mut close_others = None;
                                        let mut close_right = None;
                                        for (idx, tab) in self.tabs.iter().enumerate() {
                                            let active = self.active_tab == Some(idx);
                                            let tab_label = format!("🖥️ {}", tab.title);
                                            ui.horizontal(|ui| {
                                                let tab_resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(&tab_label).size(12.0).color(
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
                                                    .rounding(4.0)
                                                    .min_size(egui::vec2(170.0, 30.0)),
                                                );
                                                if tab_resp.clicked() {
                                                    self.active_tab = Some(idx);
                                                    self.selected_session_id = Some(tab.session_id.clone());
                                                }
                                                tab_resp.context_menu(|ui| {
                                                    if ui.button("关闭其他标签").clicked() {
                                                        close_others = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                    if ui.button("关闭右侧标签").clicked() {
                                                        close_right = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                });
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(13.0)
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
                                                        .size(14.0)
                                                        .color(theme.fg_low_color()),
                                                )
                                                .fill(egui::Color32::TRANSPARENT)
                                                .frame(false),
                                            )
                                            .on_hover_text("新建会话")
                                            .clicked()
                                        {
                                            self.show_new_session_dialog = true;
                                        }
                                        if let Some(idx) = to_close {
                                            self.tabs.remove(idx);
                                            self.active_tab = self.tabs.len().checked_sub(1);
                                            self.selected_session_id = self
                                                .active_tab
                                                .and_then(|i| self.tabs.get(i))
                                                .map(|t| t.session_id.clone());
                                        }
                                        if let Some(idx) = close_others {
                                            if idx < self.tabs.len() {
                                                let kept = self.tabs.remove(idx);
                                                self.tabs.clear();
                                                self.tabs.push(kept);
                                                self.active_tab = Some(0);
                                                self.selected_session_id = self.tabs.first().map(|t| t.session_id.clone());
                                            }
                                        }
                                        if let Some(idx) = close_right {
                                            if idx + 1 < self.tabs.len() {
                                                self.tabs.truncate(idx + 1);
                                                self.active_tab = Some(idx);
                                                self.selected_session_id = self.tabs.get(idx).map(|t| t.session_id.clone());
                                            }
                                        }
                                    });
                                    ui.spacing_mut().button_padding = prev_padding;
                                });

                            let terminal_h = ui.available_height().max(120.0);
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), terminal_h),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    if let Some(terminal) = self.current_terminal_mut() {
                                        terminal.show(ui, theme);
                                    } else {
                                        self.show_welcome(ui);
                                    }
                                },
                            );

                        },
                    );
                });
            });

        // 显示新建会话对话框
        if self.show_new_session_dialog {
            let mut open = self.show_new_session_dialog;
            let mut should_close = false;
            egui::Window::new("新建会话")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label("会话名称");
                        ui.text_edit_singleline(&mut self.new_session_name);
                        
                        ui.separator();
                        
                        ui.label("主机地址");
                        ui.text_edit_singleline(&mut self.new_session_host);
                        
                        ui.horizontal(|ui| {
                            ui.label("端口");
                            ui.add(egui::DragValue::new(&mut self.new_session_port));
                        });
                        
                        ui.separator();
                        
                        ui.label("用户名");
                        ui.text_edit_singleline(&mut self.new_session_username);
                        
                        ui.label("密码");
                        ui.add(egui::TextEdit::singleline(&mut self.new_session_password).password(true));
                        ui.label("分组");
                        ui.text_edit_singleline(&mut self.new_session_group);
                        
                        ui.separator();
                        
                        ui.horizontal(|ui| {
                            if ui.button("取消").clicked() {
                                self.reset_new_session_form();
                                should_close = true;
                            }
                            
                            if ui.button("创建并连接").clicked() {
                                self.create_and_connect_session();
                                should_close = true;
                            }
                        });
                    });
                });
            self.show_new_session_dialog = open && !should_close;
        }

        if self.show_about_dialog {
            let mut open = self.show_about_dialog;
            let mut should_close = false;
            egui::Window::new("关于 MistTerm")
                .open(&mut open)
                .resizable(false)
                .collapsible(false)
                .default_width(320.0)
                .show(ctx, |ui| {
                    ui.heading("MistTerm");
                    ui.label("版本: v0.1.0");
                    ui.label("一个现代化 SSH 终端工具");
                    ui.separator();
                    if ui.button("关闭").clicked() {
                        should_close = true;
                    }
                });
            self.show_about_dialog = open && !should_close;
        }

        if self.show_edit_session_dialog {
            let mut open = self.show_edit_session_dialog;
            let mut should_close = false;
            egui::Window::new("编辑会话")
                .open(&mut open)
                .resizable(true)
                .collapsible(false)
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label("会话名称");
                        ui.text_edit_singleline(&mut self.edit_session_name);

                        ui.separator();

                        ui.label("主机地址");
                        ui.text_edit_singleline(&mut self.edit_session_host);

                        ui.horizontal(|ui| {
                            ui.label("端口");
                            ui.add(egui::DragValue::new(&mut self.edit_session_port));
                        });

                        ui.separator();

                        ui.label("用户名");
                        ui.text_edit_singleline(&mut self.edit_session_username);

                        ui.label("密码");
                        ui.add(egui::TextEdit::singleline(&mut self.edit_session_password).password(true));
                        ui.label("分组");
                        ui.text_edit_singleline(&mut self.edit_session_group);

                        ui.separator();

                        ui.horizontal(|ui| {
                            if ui.button("取消").clicked() {
                                should_close = true;
                            }

                            if ui.button("保存").clicked() {
                                self.save_edit_session();
                                should_close = !self.show_edit_session_dialog;
                            }
                        });
                    });
                });
            self.show_edit_session_dialog = open && !should_close;
        }

        if self.show_fragments_dialog {
            let mut open = self.show_fragments_dialog;
            egui::Window::new("命令片段")
                .open(&mut open)
                .resizable(true)
                .default_width(520.0)
                .default_height(420.0)
                .show(ctx, |ui| {
                    ui.label("提示：点击底部「命令片段」按钮打开侧边栏面板");
                    ui.add_space(16.0);
                    ui.label("📋 命令片段侧边栏提供更丰富的命令分类和快捷操作");
                });
            self.show_fragments_dialog = open;
        }

        // 快速片段选择器
        if self.quick_selector.open {
            use egui::*;
            
            Window::new("⚡ 快速选择片段")
                .collapsible(false)
                .resizable(true)
                .default_size([500.0, 400.0])
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    // 搜索框
                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        ui.text_edit_singleline(&mut self.quick_selector.search_query);
                    });
                    
                    ui.add_space(8.0);
                    
                    // 片段列表
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            let fragments = self.fragment_manager.list();
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
                    
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("❌ 取消 (ESC)").clicked() {
                            self.quick_selector.open = false;
                        }
                    });
                });
        }

        // 变量输入对话框
        if self.variable_dialog.open {
            use egui::*;
            
            Window::new(format!("📝 输入变量：{}", self.variable_dialog.fragment_title))
                .collapsible(false)
                .resizable(true)
                .default_size([450.0, 300.0])
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if let Some(fragment_id) = &self.variable_dialog.fragment_id {
                        if let Some(fragment) = self.fragment_manager.get(fragment_id) {
                            for var in &fragment.variables {
                                ui.horizontal(|ui| {
                                    ui.label(&var.description);
                                    let value = self.variable_dialog.values
                                        .entry(var.name.clone())
                                        .or_insert_with(String::new);
                                    ui.text_edit_singleline(value);
                                });
                                ui.add_space(4.0);
                            }
                        }
                    }
                    
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("❌ 取消").clicked() {
                            self.variable_dialog.open = false;
                        }
                        if ui.button("✅ 执行").clicked() {
                            if let Some(fragment_id) = &self.variable_dialog.fragment_id {
                                if let Some(fragment) = self.fragment_manager.get(fragment_id) {
                                    let command = fragment.apply_variables(&self.variable_dialog.values);
                                    if let Some(session_id) = &self.selected_session_id {
                                        if let Some(tab) = self.tabs.iter_mut().find(|t| t.session_id == *session_id) {
                                            let _ = tab.terminal.send_command(&command);
                                        }
                                    }
                                }
                            }
                            self.variable_dialog.open = false;
                        }
                    });
                });
        }
    }
}

impl MistTermApp {
    /// 执行命令片段
    fn execute_fragment(&mut self, fragment: &FragmentStats) {
        if fragment.has_variables() {
            // 有变量，打开对话框
            self.variable_dialog.open = true;
            self.variable_dialog.fragment_id = Some(fragment.id.clone());
            self.variable_dialog.fragment_title = fragment.title.clone();
            self.variable_dialog.values = fragment.variable_defaults();
        } else {
            // 无变量，直接执行
            if let Some(session_id) = &self.selected_session_id {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.session_id == *session_id) {
                    let _ = tab.terminal.send_command(&fragment.command);
                }
            }
            self.quick_selector.open = false;
        }
    }

    /// 显示欢迎界面
    fn show_welcome(&self, ui: &mut egui::Ui) {
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
            ui.separator();
            ui.small("提示：双击侧边栏可以折叠/展开");
        });
    }
}
