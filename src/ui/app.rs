//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等

use eframe::egui;
use rfd::FileDialog;
use std::collections::HashSet;
use std::time::Instant;
use crate::core::{SessionManager, FragmentManager, FragmentStats, SortBy};
use crate::ui::sidebar::Sidebar;
use crate::ui::terminal::TerminalView;
use crate::ui::git_sync::GitSyncPanel;
use crate::ui::theme::ThemeManager;

struct TerminalTab {
    session_id: String,
    title: String,
    terminal: TerminalView,
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
    git_sync_panel: GitSyncPanel,
    
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
            git_sync_panel: GitSyncPanel::new(),
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
                
                ui.vertical(|ui| {
                    // 根据搜索和分类显示片段
                    let search_results = if self.fragment_search_query.is_empty() {
                        self.fragment_manager.get_all().to_vec()
                    } else {
                        self.fragment_manager.search(&self.fragment_search_query)
                            .into_iter().cloned().collect()
                    };
                    
                    let categories = self.fragment_manager.get_categories();
                    for category in &categories {
                        let category_fragments: Vec<&FragmentStats> = search_results
                            .iter()
                            .filter(|f| f.category == *category)
                            .collect();
                        
                        if category_fragments.is_empty() {
                            continue;
                        }
                        
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
                                });
                                
                                // 显示统计信息
                                if frag.usage_count > 0 {
                                    let stats_text = frag.human_readable();
                                    ui.label(
                                        egui::RichText::new(stats_text)
                                            .small()
                                            .color(egui::Color32::from_rgb(153, 153, 153))
                                    );
                                }
                            }
                        });
                        
                        ui.add_space(2.0);
                    }
                });
            });
    }

    /// 向当前标签页插入命令片段并记录统计
    fn insert_fragment_with_stats(&mut self, id: &str, command: &str) {
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(idx) {
                match tab.terminal.insert_fragment(command) {
                    Ok(_) => {
                        // 记录成功，耗时估算为 0（实际耗时无法精确测量）
                        self.fragment_manager.record_usage(id, true, 0);
                        let _ = self.fragment_manager.save(&FragmentManager::default_config_path());
                        self.status_message = format!("插入命令：{}", command);
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
    fn show_git_sync_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("git_sync_panel")
            .default_width(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.git_sync_panel.show(ui);
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

    /// README §2.4 状态徽章：rgba(255,255,255,0.2)，内边距 2px 8px，圆角 4px，11px 白字
    fn status_chip(ui: &mut egui::Ui, text: &str) {
        egui::Frame::none()
            .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51))
            .rounding(egui::Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(8.0, 2.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(11.0)
                        .color(egui::Color32::WHITE),
                );
            });
    }

    /// 底部快捷栏 + 状态栏合并为 **一个** TopBottomPanel，避免两个 bottom 叠绘挡住紫色条（README §2.3）
    fn show_bottom_chrome(&mut self, ctx: &egui::Context) {
        const QUICK_H: f32 = 44.0;
        const STATUS_H: f32 = 28.0;

        let bar_fill = egui::Color32::from_rgb(37, 37, 38); // #252526
        let btn_idle = egui::Color32::from_rgb(60, 60, 60); // #3c3c3c
        let btn_primary = egui::Color32::from_rgb(102, 126, 234); // #667eea
        let purple = egui::Color32::from_rgb(102, 126, 234);
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
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60)),
                        );
                        ui.add_space(10.0);
                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                        ui.horizontal(|ui| {
                            let mk = |label: &str, fill: egui::Color32, w: f32| {
                                egui::Button::new(
                                    egui::RichText::new(label)
                                        .size(12.0)
                                        .color(egui::Color32::WHITE),
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
                            if ui.add(mk("📥 下载", btn_idle, 88.0)).clicked() {
                                if let Some(terminal) = self.current_terminal() {
                                    self.status_message =
                                        format!("下载目录: {}", terminal.download_dir());
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
                        });
                    },
                );

                // 下：状态栏（固定 28px，贴底）
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), STATUS_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let rect = ui.max_rect();
                        ui.painter().rect_filled(rect, 0.0, purple);
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
                                    .color(egui::Color32::from_rgb(248, 250, 255)),
                            );
                            ui.label(
                                egui::RichText::new("🔒 SSH-2.0")
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(248, 250, 255)),
                            );
                            ui.label(
                                egui::RichText::new("🌐 Asia/Shanghai")
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(248, 250, 255)),
                            );
                            ui.add_space(4.0);
                            Self::status_chip(ui, "UTF-8");
                            Self::status_chip(ui, &font_px);
                            Self::status_chip(ui, &duration_chip);
                        });
                    },
                );
            });
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
        self.apply_current_theme(ctx);

        // 检查是否有终端等待 rz 上传文件
        if let Some(terminal) = self.current_terminal() {
            if terminal.pending_rz_upload {
                // 重置状态，防止重复触发
                if let Some(t) = self.current_terminal_mut() {
                    t.pending_rz_upload = false;
                }
                if let Some(path) = FileDialog::new()
                    .set_title("选择要上传的文件")
                    .pick_file() 
                {
                    self.status_message = format!("rz上传: {}", path.display());
                    if let Some(t) = self.current_terminal_mut() {
                        match t.start_upload(path.as_path()) {
                            Ok(_) => {
                                self.status_message = format!("上传成功: {}", path.display());
                            }
                            Err(e) => {
                                self.status_message = format!("上传失败: {}", e);
                            }
                        }
                    }
                } else {
                    self.status_message = "rz上传已取消".to_string();
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
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(45, 45, 45)))
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
                    ui.menu_button("主题", |ui| {
                        for (i, theme) in self.theme_manager.list_themes().iter().enumerate() {
                            let is_current = i == self.theme_manager.current;
                            let label = if is_current {
                                format!"✓ {}", theme.name)
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
                    // README §2.4 标题栏：13px，#999
                    ui.label(
                        egui::RichText::new(title)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(153, 153, 153)),
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
            self.show_git_sync_panel(ctx);
        }

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(45, 45, 45)))
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
                                    .fill(egui::Color32::from_rgb(37, 37, 38)) // #252526
                                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60))) // #3c3c3c
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.set_width(self.sidebar_width);
                                        // README §2.4 搜索框：高 36px、内边距 8px 12px、背景 #3c3c3c、圆角 6px
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(60, 60, 60))
                                            .rounding(6.0)
                                            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.sidebar_search_query)
                                                        .hint_text("搜索连接...")
                                                        .text_color(egui::Color32::WHITE)
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
                            egui::Color32::from_rgb(90, 90, 90)
                        } else {
                            egui::Color32::from_rgb(60, 60, 60)
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
                            // README §2.4 标签栏：背景 #2d2d2d、底部分隔 1px #3c3c3c、标签高 40px
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(45, 45, 45))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(60, 60, 60),
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
                                                                egui::Color32::WHITE
                                                            } else {
                                                                egui::Color32::from_rgb(153, 153, 153)
                                                            },
                                                        ),
                                                    )
                                                    .fill(if active {
                                                        egui::Color32::from_rgb(30, 30, 30) // #1e1e1e 激活
                                                    } else {
                                                        egui::Color32::from_rgb(45, 45, 45) // #2d2d2d 非激活
                                                    })
                                                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 60, 60)))
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
                                                                .color(egui::Color32::from_rgb(153, 153, 153)),
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
                                                        .color(egui::Color32::from_rgb(153, 153, 153)),
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
                                        terminal.show(ui);
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
    }
}

impl MistTermApp {
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
