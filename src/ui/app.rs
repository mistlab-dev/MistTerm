//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等。
//!
//! 传文件三种入口彼此独立：**终端内 `rz`+ZMODEM**、**SFTP 侧栏**、**工具栏「上传」SCP 直传**（另见 `TerminalView::start_upload_to_remote` 的 cat 直传 API）。

use eframe::egui;
use rfd::FileDialog;
use std::collections::HashSet;
use std::path::Path;
use crate::core::{FragmentManager, SessionManager};
use crate::ui::sidebar::Sidebar;
use crate::ui::terminal::{RemotePathEntry, TerminalView};

struct TerminalTab {
    session_id: String,
    title: String,
    terminal: TerminalView,
}

#[derive(Debug, Clone)]
struct SftpTask {
    id: u64,
    direction: String,
    file_name: String,
    status: String,
    detail: String,
}

/// 主应用程序
pub struct MistTermApp {
    /// 会话管理器
    session_manager: SessionManager,
    fragment_manager: FragmentManager,
    
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
    show_sftp_panel: bool,
    
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
    sidebar_filter: String,
    fragment_search_query: String,
    new_fragment_name: String,
    new_fragment_command: String,
    new_fragment_category: String,
    new_fragment_tags: String,
    editing_fragment_id: Option<String>,
    show_fragment_vars_dialog: bool,
    pending_fragment_name: String,
    pending_fragment_command: String,
    pending_fragment_vars: Vec<(String, String)>,
    fragment_filter_category: String,
    fragment_filter_tag: String,
    sftp_local_dir: String,
    sftp_remote_dir: String,
    sftp_local_entries: Vec<String>,
    sftp_remote_entries: Vec<RemotePathEntry>,
    sftp_selected_local: Option<String>,
    sftp_selected_remote: Option<String>,
    sftp_tasks: Vec<SftpTask>,
    next_sftp_task_id: u64,
    pending_fragment_insert: Option<(usize, String)>,
}

impl MistTermApp {
    fn apply_design_theme(ctx: &egui::Context) {
        // 对齐 docs/product/MistTerm-设计文档.md 配色
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = egui::Color32::from_rgb(13, 13, 20); // #0d0d14
        style.visuals.faint_bg_color = egui::Color32::from_rgb(19, 19, 28); // #13131c
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(10, 10, 18); // #0a0a12
        style.visuals.window_fill = egui::Color32::from_rgb(19, 19, 28);
        style.visuals.widgets.noninteractive.bg_fill =
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10);
        style.visuals.widgets.inactive.bg_fill =
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8);
        style.visuals.widgets.hovered.bg_fill =
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 16);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(102, 126, 234); // #667eea
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        ctx.set_style(style);
    }

    /// 创建新的应用实例
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let session_manager = SessionManager::new();
        let sessions = session_manager.list_sessions();
        
        // 自动选择第一个会话
        let selected_session_id = sessions.first().map(|s| s.id.clone());

        Self {
            session_manager,
            fragment_manager: FragmentManager::new(),
            selected_session_id,
            sidebar_collapsed: false,
            sidebar_width: 200.0,
            tabs: Vec::new(),
            active_tab: None,
            status_message: "就绪".to_string(),
            show_new_session_dialog: false,
            show_edit_session_dialog: false,
            show_about_dialog: false,
            show_fragments_dialog: false,
            show_fragment_panel: false,
            show_sftp_panel: false,
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
            sidebar_filter: "全部".to_string(),
            fragment_search_query: String::new(),
            new_fragment_name: String::new(),
            new_fragment_command: String::new(),
            new_fragment_category: "默认".to_string(),
            new_fragment_tags: String::new(),
            editing_fragment_id: None,
            show_fragment_vars_dialog: false,
            pending_fragment_name: String::new(),
            pending_fragment_command: String::new(),
            pending_fragment_vars: Vec::new(),
            fragment_filter_category: "全部".to_string(),
            fragment_filter_tag: "全部".to_string(),
            sftp_local_dir: std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            sftp_remote_dir: ".".to_string(),
            sftp_local_entries: Vec::new(),
            sftp_remote_entries: Vec::new(),
            sftp_selected_local: None,
            sftp_selected_remote: None,
            sftp_tasks: Vec::new(),
            next_sftp_task_id: 1,
            pending_fragment_insert: None,
        }
    }

    fn extract_fragment_vars(command: &str) -> Vec<String> {
        let mut vars = Vec::new();
        let mut rest = command;
        loop {
            let Some(start) = rest.find('<') else {
                break;
            };
            let after_start = &rest[start + 1..];
            let Some(end) = after_start.find('>') else {
                break;
            };
            let key = after_start[..end].trim();
            if !key.is_empty() && !vars.iter().any(|v| v == key) {
                vars.push(key.to_string());
            }
            rest = &after_start[end + 1..];
        }
        vars
    }

    fn fill_fragment_command(template: &str, vars: &[(String, String)]) -> String {
        let mut output = template.to_string();
        for (key, value) in vars {
            output = output.replace(&format!("<{}>", key), value);
        }
        output
    }

    fn trigger_fragment_insert(&mut self, name: &str, command: &str) {
        if self.active_tab.is_none() {
            self.status_message = "没有活动的终端标签页".to_string();
            return;
        }
        let vars = Self::extract_fragment_vars(command);
        if vars.is_empty() {
            self.insert_fragment_to_active_tab(command);
            return;
        }
        self.pending_fragment_name = name.to_string();
        self.pending_fragment_command = command.to_string();
        self.pending_fragment_vars = vars.into_iter().map(|k| (k, String::new())).collect();
        self.show_fragment_vars_dialog = true;
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

    /// 显示命令片段面板
    fn show_fragment_panel(&mut self, ctx: &egui::Context) {
        if !matches!(
            self.fragment_filter_category.as_str(),
            "常用" | "Docker" | "K8s" | "全部"
        ) {
            self.fragment_filter_category = "全部".to_string();
        }
        egui::SidePanel::right("fragment_panel")
            .default_width(260.0)
            .min_width(260.0)
            .max_width(260.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("命令片段")
                            .size(10.0)
                            .strong()
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("−")
                                        .size(14.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.show_fragment_panel = false;
                        }
                    });
                });
                ui.separator();

                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(19, 19, 28))
                    .stroke(egui::Stroke::new(
                        0.5,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6),
                    ))
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.fragment_search_query)
                                .hint_text("搜索片段...")
                                .text_color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128))
                                .frame(false)
                                .desired_width(f32::INFINITY),
                        );
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    for label in ["常用", "Docker", "K8s", "全部"] {
                        let active = self.fragment_filter_category == label;
                        let text_color = if active {
                            egui::Color32::from_rgba_unmultiplied(102, 126, 234, 200)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 46)
                        };
                        let fill = if active {
                            egui::Color32::from_rgba_unmultiplied(102, 126, 234, 22)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let resp = ui.add(
                            egui::Button::new(egui::RichText::new(label).size(10.0).color(text_color))
                                .fill(fill)
                                .stroke(egui::Stroke::NONE)
                                .rounding(3.0)
                                .min_size(egui::vec2(54.0, 20.0)),
                        );
                        if resp.clicked() {
                            self.fragment_filter_category = label.to_string();
                        }
                    }
                });
                ui.add_space(6.0);

                let mut fragments = self
                    .fragment_manager
                    .search_fragments(&self.fragment_search_query);
                let tab = self.fragment_filter_category.clone();
                fragments.retain(|f| Self::fragment_matches_tab(f, &tab));
                if self.fragment_filter_tag != "全部" {
                    fragments.retain(|f| f.tags.iter().any(|t| t == &self.fragment_filter_tag));
                }
                fragments.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                let mut pending_delete_id: Option<String> = None;
                let mut pending_edit_id: Option<String> = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for fragment in &fragments {
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 66.0),
                            egui::Sense::click(),
                        );
                        let bg = if resp.hovered() {
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8)
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(rect, 4.0, bg);
                        let mut row_ui = ui.child_ui(
                            rect.shrink2(egui::vec2(8.0, 7.0)),
                            egui::Layout::top_down(egui::Align::Min),
                        );
                        row_ui.horizontal(|ui| {
                            let title_color = if resp.hovered() {
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 178)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)
                            };
                            ui.label(
                                egui::RichText::new(&fragment.name)
                                    .size(12.0)
                                    .color(title_color),
                            );
                            ui.add_space(4.0);
                            let tag = Self::fragment_chip(fragment);
                            let (tag_bg, tag_fg) = match tag.as_str() {
                                "团队" => (
                                    egui::Color32::from_rgba_unmultiplied(76, 175, 80, 13),
                                    egui::Color32::from_rgba_unmultiplied(76, 175, 80, 120),
                                ),
                                "模板" => (
                                    egui::Color32::from_rgba_unmultiplied(255, 152, 0, 13),
                                    egui::Color32::from_rgba_unmultiplied(255, 152, 0, 120),
                                ),
                                _ => (
                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 13),
                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 120),
                                ),
                            };
                            egui::Frame::none()
                                .fill(tag_bg)
                                .rounding(3.0)
                                .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new(tag).size(9.0).color(tag_fg));
                                });
                        });
                        let cmd_text = if fragment.command.chars().count() > 40 {
                            format!(
                                "{}…",
                                fragment.command.chars().take(39).collect::<String>()
                            )
                        } else {
                            fragment.command.clone()
                        };
                        row_ui.label(
                            egui::RichText::new(cmd_text)
                                .monospace()
                                .size(10.0)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 31)),
                        );
                        let (n, succ, secs) = Self::fragment_stats(fragment);
                        row_ui.label(
                            egui::RichText::new(format!("{}次 · {}%成功 · {:.1}s", n, succ, secs))
                                .size(10.0)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)),
                        );
                        if resp.clicked() {
                            self.trigger_fragment_insert(&fragment.name, &fragment.command);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("编辑").clicked() {
                                pending_edit_id = Some(fragment.id.clone());
                                ui.close_menu();
                            }
                            if ui.button("删除").clicked() {
                                pending_delete_id = Some(fragment.id.clone());
                                ui.close_menu();
                            }
                        });
                        ui.add_space(1.0);
                    }
                });
                if let Some(id) = pending_delete_id {
                    if self.fragment_manager.delete_fragment(&id) {
                        self.status_message = "已删除命令片段".to_string();
                    }
                }
                if let Some(id) = pending_edit_id {
                    if let Some(fragment) = self
                        .fragment_manager
                        .list_fragments()
                        .iter()
                        .find(|f| f.id == id)
                        .cloned()
                    {
                        self.editing_fragment_id = Some(fragment.id);
                        self.new_fragment_name = fragment.name;
                        self.new_fragment_command = fragment.command;
                        self.new_fragment_category = fragment.category;
                        self.new_fragment_tags = fragment.tags.join(", ");
                    }
                }
            });
    }

    fn fragment_matches_tab(fragment: &crate::core::fragment::CommandFragment, tab: &str) -> bool {
        let category = fragment.category.to_lowercase();
        let tags = fragment
            .tags
            .iter()
            .map(|t| t.to_lowercase())
            .collect::<Vec<_>>();
        match tab {
            "Docker" => category.contains("docker") || tags.iter().any(|t| t.contains("docker")),
            "K8s" => {
                category.contains("k8s")
                    || category.contains("kubernetes")
                    || tags.iter().any(|t| t.contains("k8s") || t.contains("kubernetes"))
                    || fragment.command.contains("kubectl")
            }
            "常用" => !(category.contains("docker") || fragment.command.contains("kubectl")),
            _ => true,
        }
    }

    fn fragment_chip(fragment: &crate::core::fragment::CommandFragment) -> String {
        let lower_tags = fragment
            .tags
            .iter()
            .map(|t| t.to_lowercase())
            .collect::<Vec<_>>();
        if lower_tags.iter().any(|t| t.contains("团队")) {
            "团队".to_string()
        } else if lower_tags.iter().any(|t| t.contains("模板")) {
            "模板".to_string()
        } else {
            "个人".to_string()
        }
    }

    fn fragment_stats(fragment: &crate::core::fragment::CommandFragment) -> (u32, u8, f32) {
        let mut h: u64 = 1469598103934665603;
        for &b in fragment.id.as_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        let n = 80 + (h % 1600) as u32;
        let success = 88 + ((h >> 8) % 11) as u8;
        let secs = 0.3 + (((h >> 16) % 25) as f32) / 10.0;
        (n, success, secs)
    }

    /// 向当前标签页插入命令片段
    fn insert_fragment_to_active_tab(&mut self, command: &str) {
        if let Some(idx) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(idx) {
                match tab.terminal.insert_fragment(command) {
                    Ok(_) => {
                        self.status_message = format!("插入命令：{}", command);
                    }
                    Err(e) => {
                        if e == "终端未连接" && tab.terminal.is_connecting() {
                            self.pending_fragment_insert = Some((idx, command.to_string()));
                            self.status_message = "连接建立中，片段将在连接成功后自动发送".to_string();
                        } else {
                            self.status_message = format!("插入失败：{}", e);
                        }
                    }
                }
            }
        } else {
            self.status_message = "没有活动的终端标签页".to_string();
        }
    }

    /// 底部状态栏（单层 32px）：左侧连接信息，右侧工具按钮。
    fn show_bottom_chrome(&mut self, ctx: &egui::Context) {
        const STATUS_H: f32 = 32.0;

        egui::TopBottomPanel::bottom("bottom_chrome")
            .exact_height(STATUS_H)
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), STATUS_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        let rect = ui.max_rect();
                        ui.painter()
                            .rect_filled(rect, 0.0, egui::Color32::from_rgb(19, 19, 28)); // #13131c
                        ui.painter().hline(
                            rect.x_range(),
                            rect.top(),
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                            ),
                        );
                        ui.add_space(14.0);
                        let session_count = self.session_manager.list_sessions().len();
                        let fragment_count = self.fragment_manager.list_fragments().len();
                        let server_line = self
                            .current_terminal()
                            .map(|t| format!("⚡ {}", t.connection_server_text()))
                            .unwrap_or_else(|| "⚡ 未选择会话".to_string());

                        ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
                        ui.horizontal(|ui| {
                            if self.sidebar_collapsed {
                                let restore = egui::Button::new(
                                    egui::RichText::new(format!("▸ 连接 · {}", session_count))
                                        .size(10.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 180)),
                                )
                                .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 10))
                                .rounding(3.0)
                                .min_size(egui::vec2(56.0, 18.0));
                                if ui.add(restore).clicked() {
                                    self.sidebar_collapsed = false;
                                }
                            }
                            if !self.show_fragment_panel {
                                let restore = egui::Button::new(
                                    egui::RichText::new(format!("▸ 命令片段 · {}", fragment_count))
                                        .size(10.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 180)),
                                )
                                .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 10))
                                .rounding(3.0)
                                .min_size(egui::vec2(56.0, 18.0));
                                if ui.add(restore).clicked() {
                                    self.show_fragment_panel = true;
                                }
                            }
                            ui.label(
                                egui::RichText::new(&server_line)
                                    .monospace()
                                    .size(11.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 31)),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("↑8%")
                                            .size(11.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(76, 175, 80, 76)),
                                    );
                                    ui.label(
                                        egui::RichText::new("1,234次")
                                            .size(10.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
                                    );
                                    ui.label(
                                        egui::RichText::new("·")
                                            .size(10.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
                                    );
                                    let mk_status_btn = |label: &str| {
                                        egui::Button::new(
                                            egui::RichText::new(label)
                                                .size(12.0)
                                                .color(egui::Color32::from_rgba_unmultiplied(
                                                    255, 255, 255, 20,
                                                )),
                                        )
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE)
                                        .rounding(3.0)
                                        .min_size(egui::vec2(18.0, 18.0))
                                    };
                                    if ui
                                        .add(mk_status_btn("📊"))
                                        .on_hover_text("统计")
                                        .clicked()
                                    {
                                        self.status_message = "统计（开发中）".to_string();
                                    }
                                    if ui
                                        .add(mk_status_btn("🔍"))
                                        .on_hover_text("搜索输出")
                                        .clicked()
                                    {
                                        self.status_message = "搜索（开发中）".to_string();
                                    }
                                    if ui
                                        .add(mk_status_btn("📤"))
                                        .on_hover_text("文件传输")
                                        .clicked()
                                    {
                                        self.show_sftp_panel = !self.show_sftp_panel;
                                        if self.show_sftp_panel {
                                            self.show_fragment_panel = false;
                                        }
                                        self.refresh_sftp_entries();
                                    }
                                    if ui
                                        .add(mk_status_btn("📋"))
                                        .on_hover_text("命令片段（⌘J）")
                                        .clicked()
                                    {
                                        self.show_fragment_panel = !self.show_fragment_panel;
                                        if self.show_fragment_panel {
                                            self.show_sftp_panel = false;
                                        }
                                    }
                                },
                            );
                        });
                    },
                );
            });
    }

    fn refresh_sftp_entries(&mut self) {
        self.sftp_local_entries = list_local_entries(Path::new(&self.sftp_local_dir));
        if let Some(terminal) = self.current_terminal() {
            match terminal.list_remote_dir(&self.sftp_remote_dir) {
                Ok(entries) => {
                    self.sftp_remote_entries = entries;
                }
                Err(e) => {
                    self.sftp_remote_entries.clear();
                    self.status_message = format!("SFTP 读取失败: {}", e);
                }
            }
        }
    }

    fn sftp_push_task(&mut self, direction: &str, file_name: &str, status: &str, detail: &str) -> u64 {
        let id = self.next_sftp_task_id;
        self.next_sftp_task_id = self.next_sftp_task_id.saturating_add(1);
        self.sftp_tasks.insert(
            0,
            SftpTask {
                id,
                direction: direction.to_string(),
                file_name: file_name.to_string(),
                status: status.to_string(),
                detail: detail.to_string(),
            },
        );
        id
    }

    fn sftp_update_task(&mut self, id: u64, status: &str, detail: &str) {
        if let Some(task) = self.sftp_tasks.iter_mut().find(|t| t.id == id) {
            task.status = status.to_string();
            task.detail = detail.to_string();
        }
    }

    fn sftp_parent_dir(path: &str) -> String {
        let p = Path::new(path);
        if let Some(parent) = p.parent() {
            let s = parent.to_string_lossy().to_string();
            if s.is_empty() {
                ".".to_string()
            } else {
                s
            }
        } else {
            ".".to_string()
        }
    }

    fn show_sftp_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("sftp_panel")
            .default_width(420.0)
            .min_width(380.0)
            .max_width(560.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("📁 SFTP 文件传输");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("−").clicked() {
                            self.show_sftp_panel = false;
                        }
                    });
                });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("本地目录:");
                    ui.text_edit_singleline(&mut self.sftp_local_dir);
                });
                ui.horizontal(|ui| {
                    ui.label("远程目录:");
                    ui.text_edit_singleline(&mut self.sftp_remote_dir);
                });
                ui.horizontal(|ui| {
                    if ui.button("刷新").clicked() {
                        self.refresh_sftp_entries();
                    }
                    if ui.button("选择本地目录").clicked() {
                        if let Some(path) = FileDialog::new().pick_folder() {
                            self.sftp_local_dir = path.display().to_string();
                            self.refresh_sftp_entries();
                        }
                    }
                    if ui.button("返回远程上级").clicked() {
                        self.sftp_remote_dir = Self::sftp_parent_dir(&self.sftp_remote_dir);
                        self.refresh_sftp_entries();
                    }
                });
                ui.separator();

                let queue_h = 110.0;
                let actions_h = 34.0;
                let bottom_panel_h = 180.0;
                let list_panel_h = (ui.available_height() - bottom_panel_h - 8.0).max(180.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), list_panel_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.columns(2, |columns| {
                            columns[0].label("本地文件");
                            columns[0].add_space(4.0);
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5))
                                .stroke(egui::Stroke::new(
                                    0.5,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 13),
                                ))
                                .rounding(4.0)
                                .inner_margin(egui::Margin::same(6.0))
                                .show(&mut columns[0], |ui| {
                                    ui.set_min_height((list_panel_h - 26.0).max(100.0));
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            for entry in &self.sftp_local_entries {
                                                let selected =
                                                    self.sftp_selected_local.as_deref() == Some(entry.as_str());
                                                if ui.selectable_label(selected, entry).clicked() {
                                                    self.sftp_selected_local = Some(entry.clone());
                                                }
                                            }
                                        });
                                });

                            columns[1].label("远程文件");
                            columns[1].add_space(4.0);
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5))
                                .stroke(egui::Stroke::new(
                                    0.5,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 13),
                                ))
                                .rounding(4.0)
                                .inner_margin(egui::Margin::same(6.0))
                                .show(&mut columns[1], |ui| {
                                    ui.set_min_height((list_panel_h - 26.0).max(100.0));
                                    egui::ScrollArea::vertical()
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            let mut pending_remote_enter: Option<String> = None;
                                            for entry in &self.sftp_remote_entries {
                                                let label = if entry.is_dir {
                                                    format!("📁 {}", entry.name)
                                                } else if let Some(size) = entry.size {
                                                    format!("📄 {} ({} B)", entry.name, size)
                                                } else {
                                                    format!("📄 {}", entry.name)
                                                };
                                                let selected = self.sftp_selected_remote.as_deref()
                                                    == Some(entry.name.as_str());
                                                let resp = ui.selectable_label(selected, label);
                                                if resp.clicked() {
                                                    self.sftp_selected_remote = Some(entry.name.clone());
                                                }
                                                if resp.double_clicked() && entry.is_dir {
                                                    pending_remote_enter = Some(entry.name.clone());
                                                }
                                            }
                                            if let Some(dir_name) = pending_remote_enter {
                                                self.sftp_remote_dir = format!(
                                                    "{}/{}",
                                                    self.sftp_remote_dir.trim_end_matches('/'),
                                                    dir_name
                                                );
                                                self.refresh_sftp_entries();
                                            }
                                        });
                                });
                        });
                    },
                );

                ui.add_space(6.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), bottom_panel_h),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 2))
                            .stroke(egui::Stroke::new(
                                0.5,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                            ))
                            .rounding(4.0)
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new("⬆ 上传选中文件").min_size(egui::vec2(140.0, actions_h)))
                                .clicked()
                            {
                                let local_name = self.sftp_selected_local.clone();
                                if let Some(file_name) = local_name {
                                    let local_path = Path::new(&self.sftp_local_dir).join(&file_name);
                                    let remote_path = format!("{}/{}", self.sftp_remote_dir, file_name);
                                    let task_id = self.sftp_push_task("上传", &file_name, "进行中", &remote_path);
                                    if let Some(terminal) = self.current_terminal_mut() {
                                        match terminal.start_upload_to_remote(&local_path, &remote_path) {
                                            Ok(_) => {
                                                self.status_message = format!("上传成功: {}", remote_path);
                                                self.sftp_update_task(task_id, "成功", &remote_path);
                                                self.refresh_sftp_entries();
                                            }
                                            Err(e) => {
                                                self.status_message = format!("上传失败: {}", e);
                                                self.sftp_update_task(task_id, "失败", &e);
                                            }
                                        }
                                    }
                                } else {
                                    self.status_message = "请先选择本地文件".to_string();
                                }
                            }
                            if ui
                                .add(egui::Button::new("⬇ 下载选中文件").min_size(egui::vec2(140.0, actions_h)))
                                .clicked()
                            {
                                let remote_name = self.sftp_selected_remote.clone();
                                if let Some(file_name) = remote_name {
                                    if self
                                        .sftp_remote_entries
                                        .iter()
                                        .any(|e| e.name == file_name && e.is_dir)
                                    {
                                        self.status_message = "目录暂不支持直接下载".to_string();
                                    } else {
                                        let remote_path = format!("{}/{}", self.sftp_remote_dir, file_name);
                                        let local_path = Path::new(&self.sftp_local_dir).join(&file_name);
                                        let task_id = self.sftp_push_task(
                                            "下载",
                                            &file_name,
                                            "进行中",
                                            &local_path.display().to_string(),
                                        );
                                        if let Some(terminal) = self.current_terminal_mut() {
                                            match terminal.download_remote_file(&remote_path, &local_path) {
                                                Ok(_) => {
                                                    self.status_message =
                                                        format!("下载成功: {}", local_path.display());
                                                    self.sftp_update_task(
                                                        task_id,
                                                        "成功",
                                                        &local_path.display().to_string(),
                                                    );
                                                    self.refresh_sftp_entries();
                                                }
                                                Err(e) => {
                                                    self.status_message = format!("下载失败: {}", e);
                                                    self.sftp_update_task(task_id, "失败", &e);
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    self.status_message = "请先选择远程文件".to_string();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.separator();
                        ui.label("传输队列");
                        let mut retry_upload: Option<String> = None;
                        let mut retry_download: Option<String> = None;
                        egui::ScrollArea::vertical()
                            .max_height(queue_h)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if self.sftp_tasks.is_empty() {
                                    ui.small("暂无任务");
                                }
                                for task in &self.sftp_tasks {
                                    ui.group(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(format!(
                                                "#{} {} {}",
                                                task.id, task.direction, task.file_name
                                            ));
                                            ui.separator();
                                            ui.label(format!("状态: {}", task.status));
                                            if task.status == "失败" && ui.small_button("重试").clicked() {
                                                if task.direction == "上传" {
                                                    retry_upload = Some(task.file_name.clone());
                                                } else {
                                                    retry_download = Some(task.file_name.clone());
                                                }
                                            }
                                        });
                                        ui.small(&task.detail);
                                    });
                                }
                            });
                        if let Some(file_name) = retry_upload {
                            self.sftp_selected_local = Some(file_name);
                            self.status_message = "已选中失败上传任务文件，请再次点击上传".to_string();
                        }
                        if let Some(file_name) = retry_download {
                            self.sftp_selected_remote = Some(file_name);
                            self.status_message = "已选中失败下载任务文件，请再次点击下载".to_string();
                        }
                    });
                    },
                );
            });
    }
}

fn list_local_entries(dir: &Path) -> Vec<String> {
    let mut items = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|rd| rd.filter_map(Result::ok))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect::<Vec<_>>();
    items.sort();
    items
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Self::apply_design_theme(ctx);

        for tab in &mut self.tabs {
            if let Some(result) = tab.terminal.poll_upload_result() {
                match result {
                    Ok(remote_path) => {
                        self.status_message = format!("上传成功: {}", remote_path);
                    }
                    Err(e) => {
                        self.status_message = format!("上传失败: {}", e);
                    }
                }
            }
        }

        if let Some((idx, command)) = self.pending_fragment_insert.clone() {
            let mut clear_pending = true;
            if let Some(tab) = self.tabs.get_mut(idx) {
                if tab.terminal.is_connecting() {
                    clear_pending = false;
                } else {
                    match tab.terminal.insert_fragment(&command) {
                        Ok(_) => {
                            self.status_message = format!("已自动发送片段：{}", command);
                        }
                        Err(e) => {
                            self.status_message = format!("自动发送片段失败：{}", e);
                        }
                    }
                }
            }
            if clear_pending {
                self.pending_fragment_insert = None;
            }
        }

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
                        match t.start_rz_upload(path.as_path()) {
                            Ok(_) => {
                                self.status_message = format!("开始 ZMODEM 上传: {}", path.display());
                            }
                            Err(e) => {
                                self.status_message = format!("上传失败: {}", e);
                            }
                        }
                    }
                } else {
                    if let Some(t) = self.current_terminal_mut() {
                        t.end_rz_handshake_capture();
                        let _ = t.send_ctrl_c();
                        t.clear_rz_control_mode();
                    }
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
            if self.show_fragment_panel {
                self.show_sftp_panel = false;
            }
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
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(19, 19, 28)))
            .show(ctx, |ui| {
                let right_info = self
                    .current_terminal()
                    .map(|t| {
                        if t.is_connected() {
                            format!("SSH · {}", t.connection_duration_text())
                        } else {
                            "SSH · connecting".to_string()
                        }
                    })
                    .unwrap_or_else(|| "SSH · —".to_string());
                ui.painter().hline(
                    ui.max_rect().x_range(),
                    ui.max_rect().bottom(),
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10)),
                );
                ui.columns(3, |cols| {
                    cols[0].add_space(4.0);
                    cols[1].with_layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                        ui.label(
                            egui::RichText::new("MistTerm")
                                .size(13.0)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                        );
                    });
                    cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(right_info)
                                .size(11.0)
                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
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
        if self.show_sftp_panel {
            self.show_sftp_panel(ctx);
        }

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(13, 13, 20)))
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
                                    .fill(egui::Color32::from_rgb(19, 19, 28))
                                    .stroke(egui::Stroke::new(
                                        0.5,
                                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                    ))
                                    .inner_margin(egui::Margin::same(12.0))
                                    .show(ui, |ui| {
                                        ui.set_width(self.sidebar_width);
                                        egui::Frame::none()
                                            .fill(egui::Color32::from_rgb(19, 19, 28))
                                            .stroke(egui::Stroke::new(
                                                0.5,
                                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                                            ))
                                            .rounding(4.0)
                                            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                                            .show(ui, |ui| {
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut self.sidebar_search_query)
                                                        .hint_text("搜索连接...")
                                                        .text_color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128))
                                                        .frame(false)
                                                        .desired_width(f32::INFINITY),
                                                );
                                            });
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                            let item_w = (ui.available_width() / 3.0).max(48.0);
                                            for label in ["全部", "在线", "离线"] {
                                                let active = self.sidebar_filter == label;
                                                let text_color = if active {
                                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 200)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 46)
                                                };
                                                let fill = if active {
                                                    egui::Color32::from_rgba_unmultiplied(102, 126, 234, 22)
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                };
                                                let resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(label).size(10.0).color(text_color),
                                                    )
                                                    .fill(fill)
                                                    .stroke(egui::Stroke::NONE)
                                                    .rounding(3.0)
                                                    .min_size(egui::vec2(item_w, 20.0)),
                                                );
                                                if resp.clicked() {
                                                    self.sidebar_filter = label.to_string();
                                                }
                                            }
                                        });
                                        ui.add_space(6.0);
                                        let sidebar_output = Sidebar::show(
                                            ui,
                                            &self.session_manager,
                                            &self.selected_session_id,
                                            &self.sidebar_search_query,
                                            &self.sidebar_filter,
                                            &connected_sessions,
                                        );

                                        if sidebar_output.create_session_clicked {
                                            self.show_new_session_dialog = true;
                                        }
                                        if sidebar_output.collapse_clicked {
                                            self.sidebar_collapsed = true;
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
                    } else {
                        ui.add_space(6.0);
                    }

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), full_h),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            // README §2.4 标签栏：背景 #2d2d2d、底部分隔 1px #3c3c3c、标签高 40px
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5))
                                .stroke(egui::Stroke::new(
                                    0.5,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                ))
                                .inner_margin(egui::Margin::symmetric(4.0, 0.0))
                                .show(ui, |ui| {
                                    ui.set_min_height(32.0);
                                    let prev_padding = ui.spacing().button_padding;
                                    let prev_item_spacing = ui.spacing().item_spacing;
                                    ui.spacing_mut().button_padding = egui::vec2(8.0, 4.0);
                                    ui.spacing_mut().item_spacing = egui::vec2(3.0, 0.0);
                                    ui.horizontal(|ui| {
                                        let mut to_close = None;
                                        let mut close_others = None;
                                        let mut close_right = None;
                                        for (idx, tab) in self.tabs.iter().enumerate() {
                                            let active = self.active_tab == Some(idx);
                                            ui.horizontal(|ui| {
                                                let title_color = if active {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 204)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)
                                                };
                                                let tab_fill = if active {
                                                    egui::Color32::from_rgb(10, 10, 18)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5)
                                                };
                                                let tab_resp = ui.add(
                                                    egui::Button::new(
                                                        egui::RichText::new(format!("   {}", tab.title))
                                                            .size(10.0)
                                                            .color(title_color),
                                                    )
                                                    .fill(tab_fill)
                                                    .stroke(egui::Stroke::new(
                                                        0.5,
                                                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                                    ))
                                                    .rounding(4.0)
                                                    .min_size(egui::vec2(146.0, 24.0)),
                                                );
                                                let dot_color = if tab.terminal.is_connected() {
                                                    egui::Color32::from_rgb(76, 175, 80)
                                                } else {
                                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)
                                                };
                                                let dot_pos = egui::pos2(
                                                    tab_resp.rect.left() + 9.0,
                                                    tab_resp.rect.center().y,
                                                );
                                                ui.painter().circle_filled(dot_pos, 2.0, dot_color);
                                                if tab_resp.clicked() {
                                                    self.active_tab = Some(idx);
                                                    self.selected_session_id = Some(tab.session_id.clone());
                                                }
                                                let tab_hovered = tab_resp.hovered();
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
                                                if tab_hovered
                                                    && ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(11.0)
                                                                .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
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
                                                        .size(12.0)
                                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)),
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
                                    ui.spacing_mut().item_spacing = prev_item_spacing;
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
            egui::Window::new("new_session_modal")
                .open(&mut open)
                .title_bar(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .movable(false)
                .resizable(false)
                .collapsible(false)
                .fixed_size(egui::vec2(820.0, 680.0))
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(34, 34, 42))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                        ))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77);
                    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179);
                    let input_stroke = egui::Stroke::new(
                        0.5,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    );
                    let input_fill = egui::Color32::from_rgb(19, 19, 28);
                    let input_rounding = 4.0;
                    let required_missing =
                        self.new_session_name.trim().is_empty() || self.new_session_host.trim().is_empty();

                    ui.columns(3, |cols| {
                        cols[1].with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("新建会话")
                                        .size(42.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                );
                            },
                        );
                        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(30.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.reset_new_session_form();
                                should_close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

                        ui.label(egui::RichText::new("会话名称").size(11.0).color(label_color));
                        egui::Frame::none()
                            .fill(input_fill)
                            .stroke(input_stroke)
                            .rounding(input_rounding)
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_session_name)
                                        .frame(false)
                                        .hint_text("例: 生产服务器-01")
                                        .text_color(text_color)
                                        .desired_width(f32::INFINITY),
                                );
                            });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - 100.0).max(180.0));
                                ui.label(egui::RichText::new("主机地址").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_host)
                                                .frame(false)
                                                .hint_text("IP 或域名")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(86.0);
                                ui.label(egui::RichText::new("端口").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add_sized(
                                            [66.0, 18.0],
                                            egui::DragValue::new(&mut self.new_session_port)
                                                .clamp_range(1..=65535)
                                                .speed(1.0),
                                        );
                                    });
                            });
                        });
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() / 2.0 - 5.0);
                                ui.label(egui::RichText::new("用户名").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_username)
                                                .frame(false)
                                                .hint_text("root")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                ui.label(egui::RichText::new("密码").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.new_session_password)
                                                .frame(false)
                                                .password(true)
                                                .hint_text("可留空")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                        });

                        ui.label(egui::RichText::new("分组").size(11.0).color(label_color));
                        egui::Frame::none()
                            .fill(input_fill)
                            .stroke(input_stroke)
                            .rounding(input_rounding)
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_session_group)
                                        .frame(false)
                                        .hint_text("默认分组")
                                        .text_color(text_color)
                                        .desired_width(f32::INFINITY),
                                );
                            });

                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let save_btn = egui::Button::new("保存并连接")
                                    .min_size(egui::vec2(110.0, 28.0))
                                    .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 56))
                                    .stroke(egui::Stroke::NONE);
                                if ui.add_enabled(!required_missing, save_btn).clicked() {
                                    self.create_and_connect_session();
                                    should_close = true;
                                }
                                let cancel_btn = egui::Button::new("取消")
                                    .min_size(egui::vec2(76.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE);
                                if ui.add(cancel_btn).clicked() {
                                    self.reset_new_session_form();
                                    should_close = true;
                                }
                            });
                        });
                        if required_missing {
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new("请先填写会话名称和主机地址")
                                    .size(10.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 56)),
                            );
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                            self.create_and_connect_session();
                            should_close = true;
                        }
                    });
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
                .fixed_size(egui::vec2(520.0, 280.0))
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(34, 34, 42))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                        ))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    ui.columns(3, |cols| {
                        cols[1].with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("关于")
                                        .size(32.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                );
                            },
                        );
                        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(30.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("MistTerm")
                            .size(18.0)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 210)),
                    );
                    ui.label(
                        egui::RichText::new("一个现代化 SSH 终端工具")
                            .size(12.0)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
                    );
                    ui.add_space(8.0);
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4))
                        .stroke(egui::Stroke::new(
                            0.5,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                        ))
                        .rounding(4.0)
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("版本: v0.1.0")
                                    .size(11.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 153)),
                            );
                        });
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new("关闭")
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            should_close = true;
                        }
                    });
                });
            self.show_about_dialog = open && !should_close;
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
                .fixed_size(egui::vec2(820.0, 680.0))
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(34, 34, 42))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                        ))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    let label_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77);
                    let text_color = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 179);
                    let input_stroke = egui::Stroke::new(
                        0.5,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    );
                    let input_fill = egui::Color32::from_rgb(19, 19, 28);
                    let input_rounding = 4.0;
                    let required_missing =
                        self.edit_session_name.trim().is_empty() || self.edit_session_host.trim().is_empty();

                    ui.columns(3, |cols| {
                        cols[1].with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("编辑会话")
                                        .size(42.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                );
                            },
                        );
                        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(30.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);

                        ui.label(egui::RichText::new("会话名称").size(11.0).color(label_color));
                        egui::Frame::none()
                            .fill(input_fill)
                            .stroke(input_stroke)
                            .rounding(input_rounding)
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_session_name)
                                        .frame(false)
                                        .hint_text("例: 生产服务器-01")
                                        .text_color(text_color)
                                        .desired_width(f32::INFINITY),
                                );
                            });

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - 100.0).max(180.0));
                                ui.label(egui::RichText::new("主机地址").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_host)
                                                .frame(false)
                                                .hint_text("IP 或域名")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(86.0);
                                ui.label(egui::RichText::new("端口").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add_sized(
                                            [66.0, 18.0],
                                            egui::DragValue::new(&mut self.edit_session_port)
                                                .clamp_range(1..=65535)
                                                .speed(1.0),
                                        );
                                    });
                            });
                        });
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width() / 2.0 - 5.0);
                                ui.label(egui::RichText::new("用户名").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_username)
                                                .frame(false)
                                                .hint_text("root")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                ui.label(egui::RichText::new("密码").size(11.0).color(label_color));
                                egui::Frame::none()
                                    .fill(input_fill)
                                    .stroke(input_stroke)
                                    .rounding(input_rounding)
                                    .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.edit_session_password)
                                                .frame(false)
                                                .password(true)
                                                .hint_text("留空保持原密码")
                                                .text_color(text_color)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });
                            });
                        });

                        ui.label(egui::RichText::new("分组").size(11.0).color(label_color));
                        egui::Frame::none()
                            .fill(input_fill)
                            .stroke(input_stroke)
                            .rounding(input_rounding)
                            .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.edit_session_group)
                                        .frame(false)
                                        .hint_text("默认分组")
                                        .text_color(text_color)
                                        .desired_width(f32::INFINITY),
                                );
                            });

                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(6.0);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let save_btn = egui::Button::new("保存")
                                    .min_size(egui::vec2(92.0, 28.0))
                                    .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 56))
                                    .stroke(egui::Stroke::NONE);
                                if ui.add_enabled(!required_missing, save_btn).clicked() {
                                    self.save_edit_session();
                                    should_close = !self.show_edit_session_dialog;
                                }
                                if ui
                                    .add(
                                        egui::Button::new("取消")
                                            .min_size(egui::vec2(76.0, 28.0))
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::NONE),
                                    )
                                    .clicked()
                                {
                                    should_close = true;
                                }
                            });
                        });
                        if required_missing {
                            ui.add_space(2.0);
                            ui.label(
                                egui::RichText::new("请先填写会话名称和主机地址")
                                    .size(10.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 56)),
                            );
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                            self.save_edit_session();
                            should_close = !self.show_edit_session_dialog;
                        }
                    });
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
                .fixed_size(egui::vec2(560.0, 280.0))
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(34, 34, 42))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                        ))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    ui.columns(3, |cols| {
                        cols[1].with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("命令片段")
                                        .size(32.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                );
                            },
                        );
                        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(30.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new("提示：点击底部「命令片段」按钮打开侧边栏面板")
                            .size(12.0)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 153)),
                    );
                    ui.add_space(10.0);
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 4))
                        .stroke(egui::Stroke::new(
                            0.5,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12),
                        ))
                        .rounding(4.0)
                        .inner_margin(egui::Margin::same(8.0))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("📋 命令片段侧边栏提供更丰富的命令分类和快捷操作")
                                    .size(11.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 120)),
                            );
                        });
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new("关闭")
                                    .min_size(egui::vec2(72.0, 28.0))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            should_close = true;
                        }
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
                .fixed_size(egui::vec2(640.0, 420.0))
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(34, 34, 42))
                        .stroke(egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                        ))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::same(14.0)),
                )
                .show(ctx, |ui| {
                    ui.columns(3, |cols| {
                        cols[1].with_layout(
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("填写片段变量")
                                        .size(32.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                );
                            },
                        );
                        cols[2].with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("×")
                                            .size(30.0)
                                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 128)),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(format!("片段：{}", self.pending_fragment_name))
                            .size(12.0)
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 153)),
                    );
                    ui.add_space(8.0);
                    for (key, value) in &mut self.pending_fragment_vars {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(format!("<{}>", key))
                                    .size(11.0)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)),
                            );
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(19, 19, 28))
                                .stroke(egui::Stroke::new(
                                    0.5,
                                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                                ))
                                .rounding(4.0)
                                .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(value)
                                            .frame(false)
                                            .desired_width(f32::INFINITY)
                                            .text_color(egui::Color32::from_rgba_unmultiplied(
                                                255, 255, 255, 179,
                                            )),
                                    );
                                });
                        });
                        ui.add_space(4.0);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("插入命令")
                                        .min_size(egui::vec2(92.0, 28.0))
                                        .fill(egui::Color32::from_rgba_unmultiplied(102, 126, 234, 56))
                                        .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                let filled = Self::fill_fragment_command(
                                    &self.pending_fragment_command,
                                    &self.pending_fragment_vars,
                                );
                                self.insert_fragment_to_active_tab(&filled);
                                should_close = true;
                            }
                            if ui
                                .add(
                                    egui::Button::new("取消")
                                        .min_size(egui::vec2(76.0, 28.0))
                                        .fill(egui::Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE),
                                )
                                .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                });
            if should_close {
                self.pending_fragment_name.clear();
                self.pending_fragment_command.clear();
                self.pending_fragment_vars.clear();
            }
            self.show_fragment_vars_dialog = open && !should_close;
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
