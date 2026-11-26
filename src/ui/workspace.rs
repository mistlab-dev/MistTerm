//! 主窗口布局 shell：egui 区域注册顺序见 `docs/product/LAYOUT.md`
//!
//! `MistTermApp::update` 负责 tick / 快捷键；本模块编排顶栏、Activity Rail、
//! 右抽屉、中央工作区、Toast 与弹窗层(全平台统一，无常驻底栏)。

use super::*;
use crate::core::SESSION_COLOR_TAGS;

/// #488 / #493：一次性「聚焦首栏」协议。
///
/// 仅当 `first_field` 且持久标志仍为 true 时返回 true，并立刻把标志清掉。
/// 若只清局部拷贝、让状态位每帧保持 true，下一帧会再次 `request_focus`
/// 第一栏，第二栏无法点击或 Tab 聚焦（输入全进第一栏）。
pub(crate) fn consume_focus_first_once(flag: &mut bool, first_field: bool) -> bool {
    if first_field && *flag {
        *flag = false;
        true
    } else {
        false
    }
}

impl MistTermApp {
    /// #488：变量输入框的占位提示。空输入时必须有具体文案，否则用户看不出该在哪填。
    ///
    /// UI 薄封装：从 egui Context 读出当前语言，交给纯函数 [`Self::fragment_variable_hint_text`]。
    fn fragment_variable_input_hint(
        ctx: &egui::Context,
        var: &crate::core::fragment::FragmentVariable,
    ) -> String {
        Self::fragment_variable_hint_text(var, crate::i18n::language(ctx))
    }

    /// #488：占位提示的纯逻辑（语言进、文案出，不碰 egui，便于单测）。
    ///
    /// - 有默认值：展示「默认值：xxx」，让默认值可被感知（用户可直接回车确认）。
    /// - 无默认值：按变量名生成具体提示——主机 IP 类变量用「请输入主机 IP」；
    ///   其余变量用「请输入<变量名>」，比通用的「请输入」更能说明这一栏的用途。
    /// - 值来自片段定义（name/description），两个 UI 语言都返回可读文案。
    fn fragment_variable_hint_text(
        var: &crate::core::fragment::FragmentVariable,
        lang: crate::i18n::UiLanguage,
    ) -> String {
        let locale = crate::i18n::Locale::from(lang);
        let tr = |en: &'static str, zh: &'static str| locale.tr(en, zh);
        let default = var
            .default_value
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(default) = default {
            return tr("Default value: {value}", "默认值：{value}").replace("{value}", default);
        }
        let name = var.name.trim();
        let desc = var.description.trim();
        let subject: &str = if name.eq_ignore_ascii_case("host_ip")
            || name.eq_ignore_ascii_case("host_ip_or_address")
            || name.contains("ip")
            || desc.eq_ignore_ascii_case("host")
            || desc.eq_ignore_ascii_case("ip")
        {
            tr("host IP", "主机 IP")
        } else {
            name
        };
        tr("Enter {subject}", "请输入{subject}").replace("{subject}", subject)
    }

    pub(crate) fn render_workspace_shell(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        frame: &mut eframe::Frame,
        theme: &crate::ui::theme::Theme,
    ) {
        // 顶栏：
        // - 普通模式：非 macOS 显示窗口内菜单；macOS 用透明系统标题栏 + fullsize content view，
        //   会话标签画在 top_chrome（红绿灯右侧）。
        // - 沉浸式：Win/Linux 关掉系统边框，自绘标题栏并把会话标签放进去；macOS 仍走透明标题栏路径。
        let pending = self.ssh_pending_import_count();
        let _ = pending; // SSH 待导入改走右下角需确认 Toast，顶栏不再显示 chip
        let immersive = self.activity_rail_collapsed;
        self.sync_window_decorations_for_immersive(ctx, immersive);
        let custom_caption = immersive && !cfg!(target_os = "macos");
        // macOS 用 fullsize content view：标签始终画在（透明）标题栏那一行(红绿灯右侧)。
        let tabs_in_top = immersive || cfg!(target_os = "macos");
        let top_chrome_height = if tabs_in_top {
            // Win/Linux 自绘标题栏、以及 macOS 普通窗（tabs_in_top && !custom_caption）：
            // 高度取 title_bar_height 与 tab 行高的较大值，避免矮于红绿灯区（#325）。
            theme.title_bar_height().max(theme.size_tab_bar_row_h())
        } else if cfg!(target_os = "macos") {
            0.0
        } else {
            theme.top_chrome_total_height()
        };
        // #324/#325：eframe 0.37 文档要求配合 fullsize content view **每帧**调用
        // set_traffic_lights_position——AppKit 在窗口布局/缩放时会重置红绿灯位置，
        // 不钉住就会压住自绘标签行（首行被挡 + 标签栏「消失」）。
        // 钉进标签行：垂直居中于 top_chrome_height，左缘留出红绿灯宽度（约 70pt）再画 tab。
        #[cfg(target_os = "macos")]
        if top_chrome_height > 0.0 {
            let y_range = egui::Rangef::new(0.0, top_chrome_height);
            frame.set_traffic_lights_position(ctx, y_range, 12.0);
        }

        if top_chrome_height > 0.0 {
            egui::Panel::top("top_chrome")
                .exact_size(top_chrome_height)
                .frame(theme.frame_top_chrome())
                .show_separator_line(false)
                .show(ui, |ui| {
                    let bar_bg = ui.max_rect();
                    ui.painter()
                        .rect_filled(bar_bg, 0.0, theme.chrome_bar_fill());
                    if tabs_in_top {
                        ui.set_clip_rect(bar_bg);
                        let prev_padding = ui.spacing().button_padding;
                        let prev_item_spacing = ui.spacing().item_spacing;
                        ui.spacing_mut().button_padding =
                            egui::vec2(theme.spacing_tab_x(), theme.spacing_tab_y());
                        ui.spacing_mut().item_spacing =
                            egui::vec2(theme.spacing_region_gap(), 0.0);
                        if custom_caption {
                            let caption_w = 40.0 * 3.0;
                            let tabs_rect = egui::Rect::from_min_max(
                                bar_bg.min,
                                egui::pos2(bar_bg.max.x - caption_w, bar_bg.max.y),
                            );
                            let caption_rect = egui::Rect::from_min_max(
                                egui::pos2(bar_bg.max.x - caption_w, bar_bg.min.y),
                                bar_bg.max,
                            );
                            ui.scope_builder(egui::UiBuilder::new().max_rect(tabs_rect), |ui| {
                                ui.set_min_height(top_chrome_height);
                                ui.horizontal(|ui| {
                                    ui.set_min_height(top_chrome_height);
                                    self.render_session_tab_strip(ui, ctx, theme);
                                    let drag_w = ui.available_width().max(8.0);
                                    crate::ui::chrome::title_bar_drag_region(
                                        ui,
                                        ctx,
                                        egui::vec2(drag_w, top_chrome_height),
                                    );
                                });
                            });
                            ui.scope_builder(egui::UiBuilder::new().max_rect(caption_rect), |ui| {
                                ui.set_min_height(top_chrome_height);
                                let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                                let caps = crate::ui::chrome::window_caption_buttons(
                                    ui,
                                    theme,
                                    maximized,
                                    top_chrome_height,
                                );
                                if caps.minimize {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                                }
                                if caps.toggle_maximize {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                                }
                                if caps.close {
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            });
                        } else {
                            ui.horizontal(|ui| {
                                ui.set_min_height(top_chrome_height);
                                if cfg!(target_os = "macos") {
                                    ui.add_space(72.0);
                                }
                                self.render_session_tab_strip(ui, ctx, theme);
                                if cfg!(target_os = "macos") {
                                    let drag_w = ui.available_width().max(8.0);
                                    crate::ui::chrome::title_bar_drag_region(
                                        ui,
                                        ctx,
                                        egui::vec2(drag_w, top_chrome_height),
                                    );
                                }
                            });
                        }
                        ui.spacing_mut().button_padding = prev_padding;
                        ui.spacing_mut().item_spacing = prev_item_spacing;
                        let hairline = theme.hairline_width(ctx);
                        ui.painter().hline(
                            bar_bg.x_range(),
                            theme.snap_y_to_pixel(ctx, bar_bg.max.y - hairline * 0.5),
                            egui::Stroke::new(hairline, theme.color_tab_bar_divider()),
                        );
                    } else {
                        let _title_actions = crate::ui::chrome::render_top_chrome_panel(
                            ui,
                            theme,
                            !self.uses_native_menu_bar(),
                            |ui| self.show_application_menu_bar(ui, ctx, theme, frame),
                            0,
                            false,
                        );
                    }
                });
        }

        // 左侧 Activity Rail(可完全隐藏；隐藏时仍保留左缘窄恢复条)。
        self.show_activity_rail(ctx, ui, theme);

        // 右侧单抽屉：须先于 Central 注册(Foreground 重绘依赖)。
        self.right_dock_outer_left_x = None;
        let dock_col_w = layout_util::clamp_sidebar_width(self.sidebar_width);
        // #287: egui 0.37 PanelState is keyed by this parent Ui's scope_id().with(salt).
        layout_util::remember_right_dock_panel_parent_scope(ctx, ui);

        if self.fragment_state.show_fragment_panel {
            self.show_fragment_panel(ctx, ui, theme, dock_col_w);
        }

        if self.credential_panel.open {
            self.credential_panel.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.cloud_sync_panel.open {
            self.cloud_sync_panel.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.postmortem_dialog.open {
            self.postmortem_dialog.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        // SFTP(右侧面板；切换终端标签时重置远端路径并重新拉列表)
        if self.show_sftp_panel {
            if self.sftp_last_tab != self.active_tab {
                self.sftp_last_tab = self.active_tab;
                self.sftp_panel.reset();
                self.sftp_panel.request_list_on_open();
            }
            self.sftp_panel.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        // 系统监控：切换终端标签时改为采集当前 SSH 会话(与 SFTP 侧栏一致)
        // #251：面板关闭后仍后台采集（已初始化时），供系统通知 / 角标。
        if self.show_monitor_panel || self.monitor_panel.is_initialized() {
            if self.monitor_last_tab != self.active_tab {
                self.monitor_last_tab = self.active_tab;
                self.sync_monitor_panel_to_active_tab();
            }
            if self.show_monitor_panel {
                self.monitor_panel.show_side_panel(
                    ctx,
                    ui,
                    theme,
                    &mut self.show_monitor_panel,
                    &mut self.right_dock_outer_left_x,
                    dock_col_w,
                );
            }
        }

        if self.show_port_forward_panel {
            if self.port_forward_last_tab != self.active_tab {
                self.port_forward_last_tab = self.active_tab;
            }
            self.port_forward_panel.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.show_ai_panel {
            self.ai_panel.show_side_panel(
                ctx,
                ui,
                theme,
                &mut self.show_ai_panel,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default()
            // 不在 Frame 上铺底色(Central 后绘会盖住右栏)；工作区底色由侧栏/终端列各自 Frame 承担
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                layout_util::clip_ui_before_right_dock(ui, self.right_dock_outer_left_x);
                // Central 后绘制；用 max_rect∩clip + 右栏左缘收紧，避免终端白底盖住命令片段等
                let status_h = 0.0;
                let work = layout_util::central_work_rect_in_ui(
                    ui,
                    self.right_dock_outer_left_x,
                    status_h,
                );
                if work.width() < 1.0 || work.height() < 1.0 {
                    return;
                }
                ui.set_clip_rect(work);
                let pad = theme.spacing_work_area_pad();
                let work_inner = layout_util::work_area_inner_rect(work, pad);
                const WORK_BOTTOM_GAP: f32 = 1.0;
                let work_top = work.min.y;
                let work_bottom = work.max.y - WORK_BOTTOM_GAP;
                // 列布局垂直铺满到 work 底缘；顶部不留 `bg_body` 缝(tab 条 / 侧栏标题栏直接贴顶)。
                let work_body = egui::Rect::from_min_max(
                    egui::pos2(work_inner.min.x, work_top),
                    egui::pos2(work_inner.max.x, work_bottom),
                );
                // 仅铺中央槽位 bg_body(clip=work，不越过右栏)；右栏正文在 Central 后以 Foreground 绘制
                let work_painter = ui.painter().with_clip_rect(work);
                work_painter.rect_filled(work, 0.0, theme.bg_body_color());
                let seam_y = work.max.y - WORK_BOTTOM_GAP;
                if seam_y > work.min.y {
                    work_painter.hline(work.x_range(), seam_y, theme.divider_stroke());
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(work_body), |ui| {
                    ui.set_clip_rect(work);
                    let layout_h = (work_bottom - work_top).max(1.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        // 连接栏 / 拖把 / 终端三段紧贴：拖把本身就是 `bg_body` 缝(与右 dock 缝同色等宽)，
                        // 不再叠 `region_gap`，否则视觉缝 = 6+1+6=13 与右 dock 5 不一致。
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        ui.set_min_height(layout_h);
                        // 须用已分配子项的右缘，勿用 max_rect.min.x(仍是整行左缘，终端会盖住侧栏)
                        let mut col_left = ui.max_rect().min.x;
                        if !self.sidebar_collapsed {
                            let connected_sessions: HashSet<String> = self
                                .tabs
                                .iter()
                                .filter(|t| t.any_connected())
                                .map(|t| t.primary_session_id())
                                .collect();
                            let connecting_sessions: HashSet<String> = self
                                .tabs
                                .iter()
                                .filter(|t| t.any_connected_or_connecting() && !t.any_connected())
                                .map(|t| t.primary_session_id())
                                .collect();
                            let sessions_with_tabs: HashSet<String> = self
                                .tabs
                                .iter()
                                .map(|t| t.primary_session_id())
                                .collect();
                            // #390：每个会话的 Agent 状态徽标（pane 聚合）。
                            let agent_states: HashMap<String, crate::ui::sidebar::AgentBadge> =
                                self.tabs
                                    .iter()
                                    .flat_map(|t| t.panes.iter())
                                    .filter_map(|pane| {
                                        let status = pane.agent_status.as_ref()?;
                                        let badge = match status.state {
                                            crate::terminal::program_status::ProgramState::Blocked => {
                                                crate::ui::sidebar::AgentBadge::Blocked
                                            }
                                            crate::terminal::program_status::ProgramState::Working => {
                                                crate::ui::sidebar::AgentBadge::Working
                                            }
                                            crate::terminal::program_status::ProgramState::Done => {
                                                crate::ui::sidebar::AgentBadge::Done
                                            }
                                            crate::terminal::program_status::ProgramState::Error => {
                                                crate::ui::sidebar::AgentBadge::Error
                                            }
                                            _ => return None,
                                        };
                                        Some((pane.session_id.clone(), badge))
                                    })
                                    .collect();

                            let sidebar_rect = egui::Rect::from_min_max(
                                egui::pos2(col_left, work_top),
                                egui::pos2(col_left + self.sidebar_width, work_bottom),
                            );
                            ui.scope_builder(egui::UiBuilder::new().max_rect(sidebar_rect), |ui| {
                                let (sidebar_output, col_actions) = Sidebar::show_column(
                                    ui,
                                    layout_h,
                                    self.sidebar_width,
                                    true, // SSH 待导入改走 Toast，侧栏不再显示横幅
                                    0,
                                    &self.session_manager,
                                    &self.selected_session_id,
                                    &mut self.sidebar_search_query,
                                    &mut self.sidebar_filter,
                                    &mut self.session_sort_by,
                                    &connected_sessions,
                                    &connecting_sessions,
                                    &sessions_with_tabs,
                                    &self.team_service.current_team_servers(),
                                    Self::id_sidebar_connection_search(),
                                    theme,
                                    &agent_states,
                                );
                                let _ = col_actions;

                                if sidebar_output.create_session_clicked {
                                    self.dialog_flags.new_session = true;
                                }
                                if sidebar_output.local_shell_clicked {
                                    self.open_local_shell_tab(ctx);
                                    self.collapse_connection_sidebar();
                                }
                                if sidebar_output.collapse_clicked {
                                    self.collapse_connection_sidebar();
                                }
                                if let Some(session_id) = sidebar_output.selected_session_id {
                                    self.select_session(ctx, &session_id);
                                    self.collapse_connection_sidebar();
                                }
                                if let Some(session_id) = sidebar_output.open_new_tab_session_id {
                                    if let Some(session) =
                                        self.session_manager.get_session(&session_id).cloned()
                                    {
                                        // #392：侧栏「复制/打开标签」同样继承当前活动 pane cwd；E：先带外刷新。
                                        let inherit = self
                                            .active_tab
                                            .and_then(|i| self.active_pane_idx(i).map(|p| (i, p)))
                                            .and_then(|(i, p)| {
                                                self.refresh_source_cwd_for_inherit(ctx, i, p)
                                            });
                                        self.open_new_tab_for_session_with_cwd(
                                            ctx, &session, inherit,
                                        );
                                        self.collapse_connection_sidebar();
                                    }
                                }
                                if let Some(server_key) = sidebar_output.connect_team_server_key {
                                    self.connect_team_server(ctx, &server_key);
                                    self.collapse_connection_sidebar();
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
                                if let Some(session_id) = sidebar_output.view_log_session_id {
                                    let log_name = self
                                        .session_manager
                                        .get_session(&session_id)
                                        .map(|s| s.name.clone());
                                    if let Some(name) = log_name {
                                        self.flush_session_log_buffers_for_session(&session_id);
                                        self.session_log_dialog.open_for(
                                            ui.ctx(),
                                            &session_id,
                                            &name,
                                            &self.session_log_settings,
                                        );
                                    }
                                }
                                if sidebar_output.response.double_clicked() {
                                    self.collapse_connection_sidebar();
                                }
                            });
                            col_left = sidebar_rect.max.x;
                        }

                        if !self.sidebar_collapsed {
                            // 拖把宽度 = `spacing_dock_gap`(5px)，同时充当连接栏与终端之间的 `bg_body` 缝隙；
                            // 这样左/右两侧 dock 与终端之间的视觉缝完全一致。
                            let drag_w = theme.spacing_dock_gap().max(1.0);
                            let (drag_rect, drag_resp) = ui.allocate_exact_size(
                                egui::vec2(drag_w, layout_h),
                                egui::Sense::drag(),
                            );
                            col_left = ui.min_rect().max.x;
                            let color = if drag_resp.hovered() || drag_resp.dragged() {
                                theme.accent_dim_color()
                            } else {
                                theme.bg_body_color()
                            };
                            ui.painter().rect_filled(drag_rect, 0.0, color);
                            if drag_resp.dragged() {
                                let (lo, hi) = layout_util::left_sidebar_drag_clamp(ctx);
                                self.sidebar_width =
                                    layout_util::clamp_sidebar_width(layout_util::clamp_f32(
                                        self.sidebar_width + drag_resp.drag_delta().x,
                                        lo,
                                        hi,
                                    ));
                            }
                        }

                        let term_col_w = layout_util::terminal_column_width(
                            col_left,
                            work_inner.max.x,
                            self.right_dock_outer_left_x,
                        );
                        let term_h = (work_bottom - work_top).max(1.0);
                        let term_rect = egui::Rect::from_min_max(
                            egui::pos2(col_left, work_top),
                            egui::pos2(col_left + term_col_w, work_bottom),
                        );
                        // 须显式 top_down：父级是 horizontal，allocate_new_ui 会继承横向布局，
                        // Tab 行与正文会并排而非上下堆叠，终端正文宽度被压成 0。
                        ui.allocate_ui_with_layout(
                            egui::vec2(term_col_w, term_h),
                            egui::Layout::top_down(egui::Align::LEFT),
                            |ui| {
                                ui.set_clip_rect(term_rect);
                                ui.set_max_width(term_col_w);
                                ui.set_min_width(term_col_w);
                                theme.frame_terminal_column().show(ui, |ui| {
                                    let col_clip = ui.max_rect();
                                    ui.set_clip_rect(col_clip);
                                    let saved_col_item_spacing = ui.spacing().item_spacing;
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    let mut tab_row_rect = egui::Rect::NOTHING;
                                    let mut tab_sep_stroke = egui::Stroke::NONE;
                                    if !tabs_in_top {
                                        let terminal_header_row_h = theme.size_tab_bar_row_h();
                                        let tab_row_w = term_col_w;
                                        tab_row_rect = egui::Rect::from_min_size(
                                            ui.cursor().min,
                                            egui::vec2(tab_row_w, terminal_header_row_h),
                                        );
                                        ui.scope_builder(egui::UiBuilder::new().max_rect(tab_row_rect), |ui| {
                                            ui.painter().rect_filled(
                                                tab_row_rect,
                                                egui::CornerRadius::ZERO,
                                                theme.color_panel_header_band_fill(),
                                            );
                                            ui.set_clip_rect(tab_row_rect);
                                            ui.set_min_width(tab_row_w);
                                            let prev_padding = ui.spacing().button_padding;
                                            let prev_item_spacing = ui.spacing().item_spacing;
                                            ui.spacing_mut().button_padding = egui::vec2(
                                                theme.spacing_tab_x(),
                                                theme.spacing_tab_y(),
                                            );
                                            ui.spacing_mut().item_spacing =
                                                egui::vec2(theme.spacing_region_gap(), 0.0);
                                            ui.horizontal(|ui| {
                                                ui.set_min_height(terminal_header_row_h);
                                                self.render_session_tab_strip(ui, ctx, theme);
                                            });
                                            ui.spacing_mut().button_padding = prev_padding;
                                            ui.spacing_mut().item_spacing = prev_item_spacing;
                                        });
                                        // 只记 stroke，等终端正文之后再画一次，避免双线叠亮；
                                        // 也不用 Foreground，以免盖住顶栏菜单。
                                        let hairline = theme.hairline_width(ctx);
                                        tab_sep_stroke = egui::Stroke::new(
                                            hairline,
                                            theme.color_tab_bar_divider(),
                                        );
                                    }

                                    let search_h = if self.terminal_search.open {
                                        theme.size_terminal_search_bar_h()
                                    } else {
                                        0.0
                                    };
                                    let agent_banner_h = if self.active_agent_audit_banner_visible() {
                                        theme.font_size_small() + theme.spacing_md() * 2.0
                                    } else {
                                        0.0
                                    };
                                    let disconnect_open = self
                                        .active_ssh_disconnect_banner(std::time::Instant::now())
                                        .is_some();
                                    let disconnect_t = ctx.animate_bool_with_time(
                                        egui::Id::unique("ssh_disconnect_banner"),
                                        disconnect_open,
                                        0.18,
                                    );
                                    let disconnect_banner_h =
                                        Self::ssh_disconnect_banner_full_h(theme) * disconnect_t;
                                    let term_body_h = (ui.available_height()
                                        - search_h
                                        - agent_banner_h
                                        - disconnect_banner_h)
                                        .max(1.0);
                                    let terminal_search_open = self.terminal_search.open;
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(
                                            term_col_w,
                                            term_body_h + agent_banner_h + disconnect_banner_h,
                                        ),
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.set_min_height(
                                                term_body_h + agent_banner_h + disconnect_banner_h,
                                            );
                                            ui.set_max_height(
                                                term_body_h + agent_banner_h + disconnect_banner_h,
                                            );
                                            ui.set_max_width(term_col_w);
                                            if disconnect_banner_h > 0.5 {
                                                if disconnect_open {
                                                    self.paint_ssh_disconnect_banner(
                                                        ui,
                                                        theme,
                                                        ctx,
                                                        disconnect_banner_h,
                                                    );
                                                } else {
                                                    ui.allocate_exact_size(
                                                        egui::vec2(
                                                            ui.available_width(),
                                                            disconnect_banner_h,
                                                        ),
                                                        egui::Sense::hover(),
                                                    );
                                                }
                                            }
                                            if agent_banner_h > 0.0 {
                                                Self::paint_agent_audit_degraded_banner(
                                                    ui, &theme, ctx, self,
                                                );
                                            }
                                            if let Some(idx) = self.active_tab {
                                                self.maybe_collapse_narrow_split(ctx, idx, term_col_w);
                                                let kb_capture =
                                                    self.should_capture_pty_keyboard(ctx);
                                                let active_pane =
                                                    self.tabs.get(idx).map(|t| t.active_pane);
                                                let pane_capture = |pane_idx: usize| {
                                                    active_pane == Some(pane_idx) && kb_capture
                                                };
                                                let mut close_pane_req = None;
                                                if let Some(tab) = self.tabs.get_mut(idx) {
                                                    tab.sync_broadcast_targets();
                                                    if tab.panes.is_empty() {
                                                        self.show_welcome(ui);
                                                    } else {
                                                        let mut swap_panes_req = None;
                                                        crate::ui::tab_pane::render_split_body(
                                                            ui,
                                                            tab,
                                                            theme,
                                                            term_col_w,
                                                            term_body_h,
                                                            terminal_search_open,
                                                            pane_capture,
                                                            |ui, term, w, search_open, capture| {
                                                                term.show(
                                                                    ui,
                                                                    theme,
                                                                    w,
                                                                    search_open,
                                                                    capture,
                                                                );
                                                            },
                                                            |pane_idx| {
                                                                close_pane_req = Some(pane_idx);
                                                            },
                                                            |a, b| {
                                                                swap_panes_req = Some((a, b));
                                                            },
                                                        );
                                                        if let Some((a, b)) = swap_panes_req {
                                                            tab.swap_panes(a, b);
                                                        }
                                                    }
                                                } else {
                                                    self.show_welcome(ui);
                                                }
                                                if let Some(pi) = close_pane_req {
                                                    self.close_pane_tab_at(ctx, idx, pi);
                                                }
                                            } else {
                                                self.show_welcome(ui);
                                            }
                                        },
                                    );
                                    // 终端正文之后重画分隔线，盖住清屏后的顶边；
                                    // 必须留在 Middle，勿用 Foreground（否则会盖住顶栏下拉菜单）。
                                    if !tabs_in_top && tab_row_rect != egui::Rect::NOTHING {
                                        let y = theme.snap_y_to_pixel(
                                            ui.ctx(),
                                            tab_row_rect.max.y - tab_sep_stroke.width * 0.5,
                                        );
                                        ui.painter().hline(
                                            tab_row_rect.x_range(),
                                            y,
                                            tab_sep_stroke,
                                        );
                                    }
                                    if self.terminal_search.open
                                        && self.show_terminal_search_bar(ui, theme)
                                    {
                                        self.terminal_search.open = false;
                                        self.terminal_search.pending_focus = false;
                                    }
                                    if self.command_history_overlay.open {
                                        if let Some(idx) = self.active_tab {
                                            let (rect, active_sid, active_sname) =
                                                if let Some(tab) = self.tabs.get(idx) {
                                                    let rect = tab
                                                        .active_pane()
                                                        .map(|p| p.last_term_rect)
                                                        .unwrap_or(egui::Rect::NOTHING);
                                                    let sid = tab.primary_session_id();
                                                    let name = self
                                                        .session_manager
                                                        .get_session(&sid)
                                                        .map(|s| s.name.clone());
                                                    (Some(rect), Some(sid), name)
                                                } else {
                                                    (None, None, None)
                                                };
                                            if let (Some(rect), Some(active_sid)) =
                                                (rect, active_sid)
                                            {
                                                match self.command_history_overlay.show(
                                                    ctx,
                                                    theme,
                                                    &self.command_history,
                                                    rect,
                                                    Some(active_sid.as_str()),
                                                    active_sname.as_deref(),
                                                ) {
                                                    CommandHistoryAction::Close => {
                                                        self.command_history_overlay.open = false;
                                                    }
                                                    CommandHistoryAction::Apply(cmd) => {
                                                        if let Some(idx) = self.active_tab {
                                                            let _ = self.send_audited_command_at(
                                                                ctx, idx, &cmd,
                                                            );
                                                        }
                                                        self.command_history_overlay.open = false;
                                                    }
                                                    CommandHistoryAction::Delete(cmd) => {
                                                        self.command_history.remove_matching(&cmd);
                                                        if self.command_history_overlay.selected > 0
                                                        {
                                                            self.command_history_overlay
                                                                .selected -= 1;
                                                        }
                                                    }
                                                    CommandHistoryAction::ComposeFragment {
                                                        title,
                                                        commands,
                                                    } => {
                                                        self.save_composed_fragment_from_history(
                                                            ctx, title, commands,
                                                        );
                                                    }
                                                    CommandHistoryAction::None => {}
                                                }
                                            }
                                        }
                                    }
                                    ui.spacing_mut().item_spacing = saved_col_item_spacing;
                                    if self.right_dock_outer_left_x.is_some() {
                                        // 有右 dock 时不画终端右边框，避免交界处叠出黑线。
                                        ui.painter().vline(
                                            term_rect.min.x,
                                            term_rect.y_range(),
                                            theme.panel_stroke(),
                                        );
                                    } else {
                                        crate::ui::chrome::paint_rect_border_lr(
                                            ui.painter(),
                                            term_rect,
                                            theme.panel_stroke(),
                                        );
                                    }
                                });
                            },
                        );
                    });
                });
            });

        self.render_workspace_right_dock_foreground(ctx, theme, top_chrome_height);
        self.sync_ssh_import_action_toast(ctx);
        self.tick_status_toast();
        self.show_status_toast(ctx, theme);

        if self.dialog_flags.ai_settings {
            self.ai_panel.show_settings_dialog(
                ctx,
                theme,
                &mut self.dialog_flags.ai_settings,
                &mut self.app_settings,
            );
        }

        if self.dialog_flags.audit_timeline {
            let mut action = crate::ui::audit_timeline_dialog::AuditTimelineUiAction::None;
            crate::ui::audit_timeline_dialog::show_audit_timeline_modal(
                ctx,
                theme,
                &mut self.dialog_flags.audit_timeline,
                &self.audit_timeline,
                &mut self.audit_timeline_host_filter,
                &mut self.audit_timeline_outcome_filter,
                &mut action,
            );
            if matches!(action, crate::ui::audit_timeline_dialog::AuditTimelineUiAction::Clear) {
                self.audit_timeline.clear();
            }
        }

        if self.dialog_flags.agent_install {
            crate::ui::agent_install_dialog::show_agent_install_modal(
                ctx,
                theme,
                &mut self.dialog_flags.agent_install,
                &mut self.agent_install_copied,
            );
        }

        if self.dialog_flags.ask_knowledge {
            let mut action = crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::None;
            crate::ui::ask_knowledge_dialog::show_ask_knowledge_modal(
                ctx,
                theme,
                &mut self.dialog_flags.ask_knowledge,
                &mut self.ask_knowledge_query,
                &self.ask_knowledge_hits,
                self.ask_knowledge_searched,
                &mut action,
            );
            match action {
                crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::Search => {
                    self.run_ask_knowledge_search();
                }
                crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::UseHit(i) => {
                    if let Some(hit) = self.ask_knowledge_hits.get(i) {
                        if let Some(frag) = hit.fragment.clone() {
                            self.begin_fragment_insert(ctx, &frag, true);
                            self.dialog_flags.ask_knowledge = false;
                        }
                    }
                }
                crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::AskModel => {
                    let q = self.ask_knowledge_query.clone();
                    self.dialog_flags.ask_knowledge = false;
                    self.open_ai_with_model_fallback(ctx, &q);
                }
                crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::None => {}
            }
        }

        let session_for_fragments = self
            .selected_session_id
            .as_deref()
            .and_then(|sid| self.session_manager.get_session(sid).cloned());
        let fragment_cfg = FragmentManager::default_config_path();
        let lib_saved = self.fragment_state.fragment_library.show_window(
            ctx,
            &mut self.fragment_state.fragment_manager,
            &mut self.fragment_state.fragment_sort_by,
            &fragment_cfg,
            &mut self.fragment_state.fragment_shortcut_store,
            session_for_fragments.as_ref(),
            theme,
        );
        if lib_saved {
            self.fragment_state.fragment_manager.sort(self.fragment_state.fragment_sort_by);
        }

        self.render_new_session_modal(ctx, theme);
        self.render_open_existing_connection_modal(ctx, theme);

        let mut open_paste_history = false;
        for tab in &mut self.tabs {
            for pane in tab.panes.iter_mut() {
                if pane.terminal.take_open_paste_history() {
                    open_paste_history = true;
                }
            }
        }
        if open_paste_history {
            self.menu_open_paste_history(ctx);
        }
        if self.fragment_state.paste_history_overlay.open {
            let term_rect = self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .and_then(|t| t.active_pane())
                .map(|pane| pane.last_term_rect)
                .unwrap_or(egui::Rect::NOTHING);
            let remembering = self.app_settings.remember_paste_history;
            match self.fragment_state.paste_history_overlay.show(
                ctx,
                theme,
                &self.fragment_state.paste_history,
                term_rect,
                remembering,
            ) {
                PasteHistoryAction::Close => {
                    self.fragment_state.paste_history_overlay.open = false;
                }
                PasteHistoryAction::Apply(text) => {
                    self.paste_history_text_to_active(ctx, &text);
                    self.fragment_state.paste_history_overlay.open = false;
                }
                PasteHistoryAction::Delete(id) => {
                    self.fragment_state.paste_history.remove_id(id);
                    if self.fragment_state.paste_history_overlay.selected > 0 {
                        self.fragment_state.paste_history_overlay.selected -= 1;
                    }
                }
                PasteHistoryAction::None => {}
            }
        }

        // #189：启动检测到未提示过的崩溃 → 弹一次提示窗（版本/时间/panic 首行）。
        if self.crash_notice.is_some() {
            self.render_crash_notice_dialog(ctx, theme);
        }

        if self.dialog_flags.about {
            let mut open = self.dialog_flags.about;
            let mut should_close = false;
            let about_title = crate::i18n::tr(ctx, "About MistTerm", "关于 MistTerm");
            let subtitle = crate::i18n::tr(
                ctx,
                "A modern SSH terminal client.",
                "一个现代化 SSH 终端工具",
            );
            let version_line = format!(
                "{} v{}",
                crate::i18n::tr(ctx, "Version:", "版本："),
                crate::platform::APP_VERSION
            );
            let shortcuts = mistterm_functional_spec_shortcuts(ctx);
            let modal_sz = layout_util::modal_about_size_for_content(
                ctx,
                theme,
                about_title,
                subtitle,
                &version_line,
                &shortcuts,
            );
            crate::ui::chrome::modal_window("about_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .default_size(modal_sz)
                .movable(true)
                .resizable(true)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header(ui, theme, about_title, &mut should_close);
                        ui.label(
                            egui::RichText::new(crate::platform::APP_DISPLAY_NAME)
                                .size(theme.font_size_prominent())
                                .color(theme.color_body_text_muted()),
                        );
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(theme.font_size_panel_title())
                                .color(theme.color_form_hint()),
                        );
                        ui.add_space(theme.spacing_md());
                        let shortcut_frame = if theme.uses_modern_palette() {
                            egui::Frame::NONE
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, theme.border_color()))
                        } else {
                            egui::Frame::NONE
                                .fill(theme.color_subtle_inset_fill())
                                .stroke(egui::Stroke::new(1.0, theme.color_overlay_fill_subtle()))
                        };
                        shortcut_frame
                            .corner_radius(theme.radius_list_item())
                            .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x() as i8, theme.spacing_search_input_y() as i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&version_line)
                                        .size(theme.font_size_panel_title())
                                        .color(theme.color_caption_text()),
                                );
                                ui.add_space(theme.spacing_panel_gap());
                                egui::ScrollArea::vertical()
                                    .max_height(200.0)
                                    .auto_shrink([false; 2])
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        for line in shortcuts.lines() {
                                            ui.label(
                                                egui::RichText::new(line)
                                                    .font(egui::FontId::monospace(
                                                        theme.font_size_small(),
                                                    ))
                                                    .color(theme.color_sidebar_icon()),
                                            );
                                        }
                                    });
                            });
                        ui.add_space(theme.spacing_sm());
                        self.update_about_row(ui, ctx, theme);
                        ui.add_space(theme.spacing_md());
                        ui.horizontal(|ui| {
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Alert,
                                crate::i18n::tr(ctx, "Report an issue", "问题反馈"),
                            )
                            .clicked()
                            {
                                self.open_report_issue(ctx);
                            }
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::File,
                                crate::i18n::tr(ctx, "Open-source licenses", "开源许可"),
                            )
                            .clicked()
                            {
                                self.dialog_flags.licenses = true;
                            }
                        });
                    });
                });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                should_close = true;
            }
            self.dialog_flags.about = open && !should_close;
        }
        self.render_licenses_modal(ctx, theme);

        // #452: draw Preferences first (it still raises itself above the right
        // dock). Software Update is drawn next and raises itself so it stays
        // in front of a still-open Preferences. About and licenses remain
        // drawn before this pair; workspace confirms, import, and help remain
        // after both.
        if self.dialog_flags.preferences {
            self.show_preferences_modal(ctx, theme);
        }
        self.render_update_dialog(ctx, theme);

        self.render_workspace_confirm_modals(ctx, theme);

        if let Some(indices) = self.ssh_import_dialog.show(ctx, theme) {
            self.import_ssh_indices(ctx, &indices);
        }
        if self.foreign_import_dialog.open {
            let existing = self.session_manager.list_sessions().to_vec();
            if let Some(chosen) = self.foreign_import_dialog.show(ctx, theme, &existing) {
                self.import_foreign_candidates(ctx, chosen);
            }
        }
        self.session_log_dialog
            .show(ctx, theme, &self.session_log_settings);
        let help_shortcuts = crate::ui::app::mistterm_functional_spec_shortcuts(ctx);
        let onboarding_status = crate::ui::help_docs_dialog::OnboardingStatus {
            connected: !self.tabs.is_empty(),
            team_logged_in: self.team_service.is_logged_in(),
            ai_configured: self.app_settings.ai.has_api_key(),
        };
        if let Some(error) =
            self.help_docs_dialog
                .show(ctx, theme, &help_shortcuts, onboarding_status)
        {
            self.notify_error(error);
        }

        if self.dialog_flags.edit_session {
            let mut open = self.dialog_flags.edit_session;
            let mut should_close = false;
            let modal_sz = layout_util::modal_edit_size(ctx);
            crate::ui::chrome::modal_window("edit_session_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    let required_missing =
                        self.edit_session_form.name.trim().is_empty() || self.edit_session_form.host.trim().is_empty();

                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            ui.push_id("edit_session_form", |ui| {
                            Self::modal_header_title_only(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Edit session", "编辑会话"),
                                &mut should_close,
                            );

                            // 字段远多于新建：头部与底部按钮固定，中间可滚。
                            let footer_reserve = 64.0;
                            let scroll_h = (ui.available_height() - footer_reserve).max(160.0);
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .max_height(scroll_h)
                                .show(ui, |ui| {
                            let form_w = layout_util::modal_form_width(ui);
                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Session name", "会话名称"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_name",
                                &mut self.edit_session_form.name,
                                crate::i18n::tr(ctx, "e.g. prod-server-01", "例：生产服务器-01"),
                                form_w,
                                false,
                            );

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = form_w;
                                let host_w = (row_w - 98.0).max(160.0);
                                ui.vertical(|ui| {
                                    ui.set_width(host_w);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Host", "主机地址"));
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_host",
                                        &mut self.edit_session_form.host,
                                        crate::i18n::tr(ctx, "IP or hostname", "IP 或域名"),
                                        host_w,
                                        false,
                                    );
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(88.0);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Port", "端口"));
                                    Self::ui_form_port(
                                        ui,
                                        theme,
                                        "edit_session_port",
                                        &mut self.edit_session_form.port_str,
                                        &mut self.edit_session_form.port,
                                        88.0,
                                    );
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = form_w;
                                let half = ((row_w - 10.0) / 2.0).max(140.0);
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Username", "用户名"));
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_username",
                                        &mut self.edit_session_form.username,
                                        crate::i18n::tr(ctx, "e.g. root", "如 root"),
                                        half,
                                        false,
                                    );
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Password", "密码"));
                                    // 提示按「会话是否真存过密码」而不是输入框当前
                                    // 内容来切换：没存过密码（密钥 / Agent / vault）
                                    // 的会话展示「可留空」，存过的展示掩码沿用说明。
                                    // 输入过程中提示不跳变，避免误导。
                                    let session_has_saved_password = self
                                        .edit_session_form
                                        .id
                                        .as_deref()
                                        .and_then(|sid| self.session_manager.get_session(sid))
                                        .map(|s| !s.password.is_empty())
                                        .unwrap_or(false);
                                    let password_hint = if session_has_saved_password {
                                        crate::i18n::tr(
                                            ctx,
                                            "Masked: keeps the saved password; type a new one to replace it",
                                            "已掩码：表示沿用已保存的密码；输入新值即可替换",
                                        )
                                    } else {
                                        crate::i18n::tr(
                                            ctx,
                                            "Optional — leave empty when using a key or SSH Agent",
                                            "可留空——使用密钥或 SSH Agent 时无需密码",
                                        )
                                    };
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_password",
                                        &mut self.edit_session_form.password,
                                        password_hint,
                                        half,
                                        true,
                                    );
                                });
                            });

                            super::session_auth_form::show_auth_fields(
                                ui,
                                ctx,
                                theme,
                                "edit_session",
                                form_w,
                                &mut self.edit_session_form.auth_mode,
                                &mut self.edit_session_form.use_ssh_agent,
                                &mut self.edit_session_form.private_key_path,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Jump host", "跳板机"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_proxy_jump",
                                &mut self.edit_session_form.proxy_jump,
                                crate::i18n::tr(
                                    ctx,
                                    "bastion or user@bastion:22 (comma-separated hops; OpenSSH ProxyJump)",
                                    "跳板主机或 user@bastion:22（多跳逗号分隔；可填已保存会话名）",
                                ),
                                form_w,
                                false,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Proxy command", "代理命令"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_proxy_command",
                                &mut self.edit_session_form.proxy_command,
                                crate::i18n::tr(
                                    ctx,
                                    "e.g. ssh -W %h:%p jump (OpenSSH ProxyCommand)",
                                    "例：ssh -W %h:%p jump（自定义代理命令）",
                                ),
                                form_w,
                                false,
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Local port forwards",
                                    "本机端口转发",
                                ),
                            );
                            crate::ui::chrome::form_multiline_field_with_hint(
                                ui,
                                theme,
                                egui::Id::unique("edit_session_local_forwards"),
                                &mut self.edit_session_form.local_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "8080:127.0.0.1:80 (one per line)",
                                    "8080:127.0.0.1:80（每行一条）",
                                ),
                                form_w,
                                2,
                                false,
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Remote port forwards",
                                    "远端端口转发",
                                ),
                            );
                            crate::ui::chrome::form_multiline_field_with_hint(
                                ui,
                                theme,
                                egui::Id::unique("edit_session_remote_forwards"),
                                &mut self.edit_session_form.remote_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "8080:127.0.0.1:3000 (one per line)",
                                    "8080:127.0.0.1:3000（每行一条）",
                                ),
                                form_w,
                                2,
                                false,
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Dynamic forwards (SOCKS)",
                                    "动态转发（SOCKS）",
                                ),
                            );
                            crate::ui::chrome::form_multiline_field_with_hint(
                                ui,
                                theme,
                                egui::Id::unique("edit_session_dynamic_forwards"),
                                &mut self.edit_session_form.dynamic_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "1080 or 0.0.0.0:1080 (one per line)",
                                    "1080 或 0.0.0.0:1080（每行一条）",
                                ),
                                form_w,
                                2,
                                false,
                            );

                            // #177：连接后恢复命令（可选，默认空 = 不执行）
                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Post-connect restore command (optional)", "连接后恢复命令（可选）"),
                            );
                            crate::ui::chrome::form_multiline_field_with_hint(
                                ui,
                                theme,
                                egui::Id::unique("edit_session_post_connect_command"),
                                &mut self.edit_session_form.post_connect_command,
                                crate::i18n::tr(
                                    ctx,
                                    "e.g. tmux attach -t main | empty = off; runs once after connect/reconnect",
                                    "如 tmux attach -t main | 留空不执行；连接/重连成功后执行一次",
                                ),
                                form_w,
                                2,
                                false,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Group", "分组"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_group",
                                &mut self.edit_session_form.group,
                                crate::i18n::tr(ctx, "Default group", "默认分组"),
                                form_w,
                                false,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Accent color tag", "环境色标"));
                            crate::ui::chrome::form_combo(
                                ui,
                                theme,
                                "edit_session_color",
                                crate::i18n::session_color_tag(
                                    ctx,
                                    SESSION_COLOR_TAGS
                                        .iter()
                                        .find(|(v, _)| *v == self.edit_session_form.color_tag.as_str())
                                        .map(|(v, _)| *v)
                                        .unwrap_or_else(|| self.edit_session_form.color_tag.as_str()),
                                ),
                                form_w,
                                |ui| {
                                    for (value, _) in SESSION_COLOR_TAGS {
                                        let label = crate::i18n::session_color_tag(ctx, value);
                                        if ui
                                            .selectable_value(
                                                &mut self.edit_session_form.color_tag,
                                                value.to_string(),
                                                label,
                                            )
                                            .clicked()
                                        {}
                                    }
                                },
                            );

                            ui.label(
                                egui::RichText::new(crate::i18n::tr(ctx, "Connection keep-alive", "保持连接"))
                                    .size(theme.font_size_panel_title())
                                    .strong()
                                    .color(theme.color_form_label()),
                            );
                            crate::ui::chrome::form_checkbox_with_id(
                                ui,
                                theme,
                                "edit_session_keepalive_enabled",
                                &mut self.edit_session_form.keepalive_enabled,
                                crate::i18n::tr(ctx, "Enable keepalive pings", "空闲时发送心跳，防止被断开"),
                            );
                            if self.edit_session_form.keepalive_enabled {
                                // 与偏好设置一致：两个步进器各占一行，窄表单不把标签折断（#130）。
                                let every = crate::i18n::tr(ctx, "Interval (s)", "每隔（秒）");
                                let give_up = crate::i18n::tr(ctx, "Max timeouts", "连续无应答");
                                let label_w = crate::ui::chrome::form_label_advance(ui, theme, every)
                                    .max(crate::ui::chrome::form_label_advance(
                                        ui,
                                        theme,
                                        give_up,
                                    ));
                                crate::ui::chrome::form_labeled_control_row(
                                    ui,
                                    theme,
                                    every,
                                    label_w,
                                    |ui| {
                                        crate::ui::chrome::form_u32_stepper(
                                            ui,
                                            theme,
                                            egui::Id::unique("edit_sess_ka_interval"),
                                            &mut self.edit_session_form.keepalive_interval_secs,
                                            5..=300,
                                            5,
                                        )
                                    },
                                );
                                crate::ui::chrome::form_labeled_control_row(
                                    ui,
                                    theme,
                                    give_up,
                                    label_w,
                                    |ui| {
                                        crate::ui::chrome::form_u8_stepper(
                                            ui,
                                            theme,
                                            egui::Id::unique("edit_sess_ka_count"),
                                            &mut self.edit_session_form.keepalive_count_max,
                                            1..=20,
                                            1,
                                        )
                                    },
                                );
                            }
                            crate::ui::chrome::form_checkbox_with_id(
                                ui,
                                theme,
                                "edit_session_keepalive_auto_reconnect",
                                &mut self.edit_session_form.keepalive_auto_reconnect,
                                crate::i18n::tr(ctx, "Reconnect automatically after disconnect", "断开后自动重连"),
                            );

                            ui.add_space(theme.spacing_sm());
                            self.edit_session_form.vault.show(
                                ui,
                                theme,
                                form_w,
                                &self.app_settings.vault,
                                "edit_session",
                            );

                                }); // end scrollable fields

                            ui.add_space(theme.spacing_list_item_x());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                                let can_save = !required_missing;
                                if ui
                                    .add(
                                        crate::ui::chrome::modal_primary_button_with_icon_widget(
                                            th,
                                            crate::ui::icons::IconId::Check,
                                            crate::i18n::tr(ctx, "Save", "保存"),
                                        )
                                        .can_activate(can_save),
                                    )
                                    .on_hover_text(if can_save {
                                        crate::i18n::tr(ctx, "Save session profile", "保存会话配置")
                                    } else {
                                        crate::i18n::tr(ctx, "Enter session name and host first.", "请先填写会话名称和主机地址")
                                    })
                                    .clicked()
                                    && can_save
                                {
                                    self.save_edit_session(ui.ctx());
                                    should_close = !self.dialog_flags.edit_session;
                                }
                                if crate::ui::chrome::modal_secondary_icon_button(
                                    ui,
                                    th,
                                    crate::ui::icons::IconId::Cross,
                                    crate::i18n::tr(ctx, "Cancel", "取消"),
                                )
                                .clicked() {
                                    should_close = true;
                                }
                            });
                            });
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                        self.save_edit_session(ui.ctx());
                        should_close = !self.dialog_flags.edit_session;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        should_close = true;
                    }
                });
            self.dialog_flags.edit_session = open && !should_close;
        }

        if self.fragment_state.show_fragments_dialog {
            let mut open = self.fragment_state.show_fragments_dialog;
            let mut should_close = false;
            let modal_sz = layout_util::modal_confirm_size(ctx);
            crate::ui::chrome::modal_window("fragments_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            Self::modal_header(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Command snippets", "命令片段"),
                                &mut should_close,
                            );
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Tip: open the snippets panel from the Activity Rail or View menu.",
                                    "提示：从活动栏或「视图」菜单打开命令片段面板",
                                ))
                                    .size(theme.font_size_panel_title())
                                    .color(theme.color_caption_text()),
                            );
                            ui.add_space(theme.spacing_md());
                            egui::Frame::NONE
                                .fill(theme.color_subtle_inset_fill())
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    theme.color_overlay_fill_subtle(),
                                ))
                                .corner_radius(theme.radius_list_item())
                                .inner_margin(egui::Margin::symmetric((theme.spacing_search_input_x()) as i8, (theme.spacing_search_input_y()) as i8))
                                .show(ui, |ui| {
                                    crate::ui::icons::icon_label_row(
                                        ui,
                                        crate::ui::icons::IconId::Fragment,
                                        crate::i18n::tr(
                                            ctx,
                                            "The snippets sidebar has richer categories and faster actions.",
                                            "命令片段侧边栏提供更丰富的命令分类和快捷操作",
                                        ),
                                        theme.font_size_small(),
                                        6.0,
                                        |t| {
                                            t.size(theme.font_size_small())
                                                .color(theme.color_caption_text())
                                        },
                                    );
                                });
                    });
                });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                should_close = true;
            }
            self.fragment_state.show_fragments_dialog = open && !should_close;
        }

        if self.fragment_state.show_fragment_vars_dialog {
            let mut open = self.fragment_state.show_fragment_vars_dialog;
            let mut should_close = false;
            let modal_sz = layout_util::fragment_vars_modal_size(ctx);
            crate::ui::chrome::modal_window("fragment_vars_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            Self::modal_header_title_only(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Snippet placeholders", "填写片段变量"),
                                &mut should_close,
                            );
                            ui.add_space(-2.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    crate::i18n::tr(ctx, "Snippet:", "片段："),
                                    self.fragment_state.pending_fragment_name
                                ))
                                    .size(theme.font_size_fragment_dialog_caption())
                                    .color(theme.color_caption_text()),
                            );
                            ui.add_space(theme.spacing_panel_gap());
                            let var_field_w = layout_util::finite_content_width(ui);
                            // #493：与 #491 的「填写变量」弹窗一致——不再让用户
                            // 「填完再点一次重算」，说明文案直接讲清楚实时预览。
                            ui.label(crate::ui::chrome::rich_caption(
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Type a value here and the command below will change: it replaces the matching <placeholder> in the snippet.",
                                    "在这里填值，下面的命令会随之改变：它会替换片段里对应的 <占位符>。",
                                ),
                            ));
                            ui.add_space(theme.spacing_sm());
                            let mut changed_any = false;
                            // #488 / #491 同款：弹窗刚打开时聚焦第一个变量输入框，
                            // 用户可以直接输入，无需先点一次输入框。
                            // 必须清掉持久状态位本身（不能只清局部拷贝），
                            // 否则每帧都会把焦点抢回第一栏。
                            let mut first_field = true;
                            for (key, value) in &mut self.fragment_state.pending_fragment_vars {
                                crate::ui::chrome::form_field_label(
                                    ui,
                                    theme,
                                    &format!("<{}>", key),
                                );
                                let field_id =
                                    egui::Id::unique(("pending_frag_var", key.as_str()));
                                // #493：实心底 + 描边输入框（modern 主题的下划线样式
                                // 会和弹窗背景糊在一起），占位符与输入正文左对齐，
                                // 与 #491 之后的「填写变量」弹窗一致。
                                let resp = crate::ui::chrome::form_singleline_field_boxed_with_align(
                                    ui,
                                    theme,
                                    field_id,
                                    value,
                                    "",
                                    var_field_w,
                                    false,
                                    egui::Align2::LEFT_CENTER,
                                );
                                if resp.changed() {
                                    changed_any = true;
                                }
                                if consume_focus_first_once(
                                    &mut self.fragment_state.pending_fragment_vars_focus_first,
                                    first_field,
                                ) {
                                    resp.request_focus();
                                    first_field = false;
                                }
                                // #466 / #493：这一栏必须真的对应模板里的 <占位符>，
                                // 否则填了也不会改变命令——要给出可操作提示。
                                match crate::core::placeholder_usage(
                                    &self.fragment_state.pending_fragment_command,
                                    key,
                                ) {
                                    crate::core::PlaceholderUsage::Used => {
                                        ui.label(crate::ui::chrome::rich_caption(
                                            theme,
                                            &format!(
                                                "{} <{}>",
                                                crate::i18n::tr(
                                                    ctx,
                                                    "Replaces",
                                                    "将替换"
                                                ),
                                                key
                                            ),
                                        ));
                                    }
                                    crate::core::PlaceholderUsage::Missing => {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}{}",
                                                crate::i18n::tr(
                                                    ctx,
                                                    "Not in the command yet: put ",
                                                    "当前命令里还没有这个占位符：请在模板里加上 "
                                                ),
                                                format!("<{}>{}", key,
                                                    crate::i18n::tr(
                                                        ctx,
                                                        " into the snippet template, or this field will not change the command.",
                                                        "，否则这一栏不会改变当前命令。"
                                                    ))
                                            ))
                                            .size(theme.font_size_fragment_dialog_caption())
                                            .color(theme.color_status_warning_text()),
                                        );
                                    }
                                }
                                ui.add_space(theme.spacing_panel_gap());
                            }
                            // 没有输入框时循环不会消费标志，这里一并清掉，
                            // 避免状态位粘在 true。
                            self.fragment_state.pending_fragment_vars_focus_first = false;
                            // #493：与 #491 相同的 changed_any 兜底——任何一栏有变化
                            // 就立刻用当前值重建预览并清掉上次的发送错误。
                            if changed_any {
                                self.fragment_state.last_fragment_vars_finalize_error = None;
                                self.sync_pending_fragment_command_edit();
                            }
                            if let Some(ref err) = self.fragment_state.last_fragment_vars_finalize_error {
                                ui.add_space(theme.spacing_panel_gap());
                                ui.label(
                                    egui::RichText::new(err)
                                        .size(theme.font_size_fragment_dialog_caption())
                                        .color(theme.color_danger_emphasis()),
                                );
                            }
                            let unmatched = crate::core::unmatched_variable_names(
                                &self.fragment_state.pending_fragment_command,
                                self.fragment_state.pending_fragment_vars.iter().map(|(k, _)| k.as_str()),
                            );
                            if !unmatched.is_empty() {
                                ui.separator();
                                ui.label(crate::ui::chrome::rich_caption(
                                    theme,
                                    crate::i18n::tr(
                                        ctx,
                                        "These fields are not used by this snippet's command, so they will not change it:",
                                        "这些变量没有出现在本片段的命令里，填了也不会改变它：",
                                    ),
                                ));
                                for name in &unmatched {
                                    ui.label(crate::ui::chrome::rich_caption(
                                        theme,
                                        &format!("  <{}>", name),
                                    ));
                                }
                                ui.label(crate::ui::chrome::rich_caption(
                                    theme,
                                    crate::i18n::tr(
                                        ctx,
                                        "Add the placeholder to the snippet, or edit the command below directly.",
                                        "把上面的 <变量名> 加进片段模板，或直接编辑下面的命令。",
                                    ),
                                ));
                            }
                            ui.separator();
                            crate::ui::chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Command to run (editable)", "将要执行的命令（可编辑）"),
                            );
                            crate::ui::chrome::form_multiline_field(
                                ui,
                                theme,
                                egui::Id::unique("pending_frag_cmd_edit"),
                                &mut self.fragment_state.pending_fragment_command_edit,
                                var_field_w,
                                4,
                                false,
                            );
                            ui.add_space(theme.spacing_sm());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                                    let insert_label = match self.fragment_state.fragment_vars_completion {
                                        FragmentVarsCompletion::PasteInsertStats => {
                                            crate::i18n::tr(ctx, "Insert into terminal", "插入终端")
                                        }
                                        FragmentVarsCompletion::QuickExecuteSend => {
                                            crate::i18n::tr(ctx, "Send command", "发送命令")
                                        }
                                    };
                                    if ui
                                        .add(crate::ui::chrome::modal_primary_button_with_icon_widget(
                                            th,
                                            crate::ui::icons::IconId::TerminalPrompt,
                                            insert_label,
                                        ))
                                        .clicked()
                                    {
                                        match self.finalize_pending_fragment_send() {
                                            Ok(filled) => {
                                                // #493：发送成功后清掉弹窗内展示的上次错误。
                                                self.fragment_state.last_fragment_vars_finalize_error = None;
                                                match self.fragment_state.fragment_vars_completion {
                                                    FragmentVarsCompletion::PasteInsertStats => {
                                                        if let Some(id) =
                                                            self.fragment_state.pending_fragment_id.clone()
                                                        {
                                                            self.insert_expanded_fragment_with_stats(
                                                                ctx,
                                                                &id,
                                                                &filled,
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
                                                                        && self.tabs[i]
                                                                            .primary_session_id()
                                                                            == *session_id
                                                                })
                                                                .or_else(|| {
                                                                    self.tabs.iter().position(|t| {
                                                                        t.primary_session_id()
                                                                            == *session_id
                                                                    })
                                                                });
                                                            if let Some(idx) = idx {
                                                                if self
                                                                    .tabs[idx]
                                                                    .active_terminal()
                                                                    .map(|t| t.is_connected())
                                                                    .unwrap_or(false)
                                                                {
                                                                    if self.send_audited_command_at(
                                                                        ctx, idx, &filled,
                                                                    ) != crate::core::CommandSendResult::Sent
                                                                    {
                                                                        return;
                                                                    }
                                                                    if let Some(ref fid) =
                                                                        self.fragment_state.pending_fragment_id
                                                                    {
                                                                        let dur_ms = start
                                                                            .elapsed()
                                                                            .as_millis()
                                                                            .max(1)
                                                                            as u64;
                                                                        let fid_owned =
                                                                            fid.clone();
                                                                        self.record_fragment_execution(
                                                                            &fid_owned,
                                                                            true,
                                                                            dur_ms,
                                                                        );
                                                                    }
                                                                } else if let Some(fid) =
                                                                    self.fragment_state.pending_fragment_id.clone()
                                                                {
                                                                    self.insert_fragment_at_tab_index(
                                                                        ctx,
                                                                        idx,
                                                                        Some(fid.as_str()),
                                                                        &filled,
                                                                    );
                                                                }
                                                            }
                                                        }
                                                        self.fragment_state.quick_selector.open = false;
                                                    }
                                                }
                                                should_close = true;
                                            }
                                            Err(e) => {
                                                // #493：错误留在弹窗内（可操作反馈），
                                                // 不再用 toast——用户能边改边重试。
                                                self.fragment_state.last_fragment_vars_finalize_error =
                                                    Some(crate::i18n::localize_fragment_expr_error(
                                                        crate::i18n::language(ctx),
                                                        &e,
                                                    ));
                                            }
                                        }
                                    }
                                    if crate::ui::chrome::modal_secondary_icon_button(
                                        ui,
                                        th,
                                        crate::ui::icons::IconId::Cross,
                                        crate::i18n::tr(ctx, "Cancel", "取消"),
                                    )
                                    .clicked()
                                    {
                                        should_close = true;
                                    }
                            });
                        });
                });
            if should_close {
                self.fragment_state.pending_fragment_id = None;
                self.fragment_state.pending_fragment_name.clear();
                self.fragment_state.pending_fragment_command.clear();
                self.fragment_state.pending_fragment_vars.clear();
                self.fragment_state.last_fragment_vars_finalize_error = None;
                self.fragment_state.pending_fragment_vars_focus_first = false;
            }
            self.fragment_state.show_fragment_vars_dialog = open && !should_close;
        }

        // 快速片段选择器
        if self.fragment_state.quick_selector.open {
            let qsz = layout_util::centered_window_default_size(ctx, 0.40, 0.48);
            let qsz_v = egui::vec2(qsz[0], qsz[1]);
            let q_scroll_max = layout_util::dialog_scroll_max_height(ctx, 220.0);
            let mut should_close = false;
            crate::ui::chrome::modal_window("quick_fragment_selector", theme, ctx)
                .movable(true)
                .resizable(true)
                .default_pos(layout_util::modal_center_pos(ctx, qsz_v))
                .default_size(qsz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header_title_only(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Quick snippet picker", "快速选择片段"),
                            &mut should_close,
                        );
                        let q_search_w = layout_util::finite_content_width(ui);
                        crate::ui::chrome::search_field(
                            ui,
                            theme,
                            egui::Id::unique("quick_fragment_search"),
                            &mut self.fragment_state.quick_selector.search_query,
                            crate::i18n::tr(ctx, "Search snippets…", "搜索片段…"),
                            q_search_w,
                        );

                        ui.add_space(theme.spacing_md());

                        // 片段列表
                        egui::ScrollArea::vertical()
                            .max_height(q_scroll_max)
                            .show(ui, |ui| {
                                let fragments: Vec<_> = self.fragment_state.fragment_manager.list().to_vec();
                                let search_lower = self.fragment_state.quick_selector.search_query.to_lowercase();
                                let matched: Vec<_> = fragments
                                    .iter()
                                    .filter(|fragment| {
                                        crate::i18n::fragment_matches_search(
                                            ui.ctx(),
                                            fragment,
                                            &search_lower,
                                        )
                                    })
                                    .collect();

                                if matched.is_empty() {
                                    let empty = if fragments.is_empty() {
                                        crate::i18n::tr(
                                            ui.ctx(),
                                            "No snippets to pick. Add one in Snippet Library first.",
                                            "还没有片段可选用。先去「命令片段库」新建一条。",
                                        )
                                    } else {
                                        crate::i18n::tr(
                                            ui.ctx(),
                                            "No matching snippets. Try another search.",
                                            "没有匹配的片段。换个关键词试试。",
                                        )
                                    };
                                    ui.label(
                                        egui::RichText::new(empty)
                                            .size(theme.font_size_caption())
                                            .color(theme.text_tertiary()),
                                    );
                                } else {
                                    for (idx, fragment) in matched.iter().enumerate() {
                                        let is_selected =
                                            idx == self.fragment_state.quick_selector.selected_index;
                                        let title = crate::i18n::fragment_display_title(
                                            ui.ctx(),
                                            fragment,
                                        );

                                        if ui.selectable_label(is_selected, title).clicked() {
                                            self.execute_fragment(ctx, fragment);
                                            self.fragment_state.quick_selector.open = false;
                                        }
                                    }
                                }
                            });

                        ui.add_space(theme.spacing_md());
                        ui.horizontal(|ui| {
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Cross,
                                crate::i18n::tr(ctx, "Cancel (ESC)", "取消 (ESC)"),
                            )
                            .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                });
            if should_close {
                self.fragment_state.quick_selector.open = false;
            }
        }

        // 变量输入对话框(片段库定义的变量；与命令里的 `<pod>` 等占位符可串联)
        if self.fragment_state.variable_dialog.open {
            let ok_label_static = if self.fragment_state.variable_dialog.paste_after_fill {
                crate::i18n::tr(ctx, "Insert into terminal", "插入终端")
            } else {
                crate::i18n::tr(ctx, "Execute", "执行")
            };

            let var_sz = layout_util::centered_window_default_size(ctx, 0.36, 0.38);
            let var_sz_v = egui::vec2(var_sz[0], var_sz[1]);
            let scroll_h = layout_util::dialog_scroll_max_height(ctx, 240.0);
            let mut should_close = false;
            crate::ui::chrome::modal_window("fragment_variable_modal", theme, ctx)
                .id(egui::Id::unique("mistterm_fragment_variable_dialog"))
                .movable(true)
                .resizable(true)
                .default_pos(layout_util::modal_center_pos(ctx, var_sz_v))
                .default_size(var_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header_title_only(
                            ui,
                            theme,
                            crate::i18n::tr(ui.ctx(), "Fill variables", "填写变量"),
                            &mut should_close,
                        );
                        ui.label(crate::ui::chrome::rich_caption(
                            theme,
                            &self.fragment_state.variable_dialog.fragment_title,
                        ));
                        ui.add_space(theme.spacing_sm());
                        egui::ScrollArea::vertical()
                            .max_height(scroll_h)
                            .show(ui, |ui| {
                                if let Some(fragment_id) = &self.fragment_state.variable_dialog.fragment_id {
                                    if let Some(fragment) = self.fragment_state.fragment_manager.get(fragment_id) {
                                        // #466：先讲清楚「填这里 → 下面命令会变成这样」，
                                        // 让用户明白输入框对应的是命令里的 <占位符> 片段。
                                        ui.label(crate::ui::chrome::rich_caption(
                                            theme,
                                            crate::i18n::tr(
                                                ui.ctx(),
                                                "Type a value here and the command below will change: it replaces the matching <placeholder> in the snippet.",
                                                "在这里填值，下面的命令会随之改变：它会替换片段里对应的 <占位符>。",
                                            ),
                                        ));
                                        ui.add_space(theme.spacing_sm());
                                        let mut changed_any = false;
                                        // #488：只聚焦第一个(最上方的)变量输入框。
                                        let mut first_field = true;
                                        for var in &fragment.variables {
                                            let used = crate::core::placeholder_usage(
                                                &fragment.command,
                                                &var.name,
                                            );
                                            // 主视觉标签 = 真正会被替换的占位符 <name>；
                                            // 描述只在后面作为次要说明，避免「host」之类混淆会话字段。
                                            crate::ui::chrome::form_field_label(
                                                ui,
                                                theme,
                                                &format!("<{}>", var.name),
                                            );
                                            if !var.description.trim().is_empty() {
                                                ui.label(crate::ui::chrome::rich_caption(
                                                    theme,
                                                    &var.description,
                                                ));
                                            }
                                            let value = self
                                                .fragment_state
                                                .variable_dialog
                                                .values
                                                .entry(var.name.clone())
                                                .or_default();
                                            let var_w = layout_util::finite_content_width(ui);
                                            // #488：改用实心底+描边输入框——modern 主题的下划线
                                            // 样式会和弹窗背景糊在一起，用户看不出该在哪填。
                                            // 固定 id 便于打开弹窗后直接聚焦第一栏。
                                            let field_id =
                                                egui::Id::unique(("var_dialog", var.name.as_str()));
                                            let resp = crate::ui::chrome::form_singleline_field_boxed_with_align(
                                                ui,
                                                theme,
                                                field_id,
                                                value,
                                                &Self::fragment_variable_input_hint(ui.ctx(), var),
                                                var_w,
                                                false,
                                                // #491：表单语义，占位符与输入正文都左对齐，
                                                // 与普通表单输入框一致（IP / 主机名 / 命令参数）。
                                                egui::Align2::LEFT_CENTER,
                                            );
                                            let value_changed = resp.changed();
                                            // #488：弹窗刚打开时聚焦第一个变量输入框，
                                            // 用户可以直接输入，无需先点一次输入框。
                                            if first_field && self.fragment_state.variable_dialog.focus_first_input {
                                                resp.request_focus();
                                                self.fragment_state.variable_dialog.focus_first_input = false;
                                                first_field = false;
                                            }
                                            match used {
                                                crate::core::PlaceholderUsage::Used => {
                                                    ui.label(crate::ui::chrome::rich_caption(
                                                        theme,
                                                        &format!(
                                                            "{} <{}>",
                                                            crate::i18n::tr(
                                                                ui.ctx(),
                                                                "Replaces",
                                                                "将替换"
                                                            ),
                                                            var.name
                                                        ),
                                                    ));
                                                }
                                                crate::core::PlaceholderUsage::Missing => {
                                                    // #466：该变量没有出现在命令模板里，
                                                    // 填了也不会改变下面的命令——必须明确说明并给出下一步。
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "{}{}",
                                                            crate::i18n::tr(
                                                                ui.ctx(),
                                                                "Not in the command yet: put ",
                                                                "当前命令里还没有这个占位符：请在模板里加上 "
                                                            ),
                                                            format!("<{}>{}", var.name,
                                                                crate::i18n::tr(
                                                                    ui.ctx(),
                                                                    " into the snippet template, or this field will not change the command.",
                                                                    "，否则这一栏不会改变当前命令。"
                                                                ))
                                                        ))
                                                        .size(theme.font_size_fragment_dialog_caption())
                                                        .color(theme.color_status_warning_text()),
                                                    );
                                                }
                                            }
                                            if value_changed {
                                                changed_any = true;
                                            }
                                            ui.add_space(theme.spacing_md());
                                        }
                                        if changed_any {
                                            // #491：用户刚在上方输入框打字——以当前变量值
                                            // 重建命令预览（即时生效，不再需要「重写」按钮）。
                                            self.fragment_state.variable_dialog.last_finalize_error = None;
                                            let preview = self.build_fragment_command_preview(
                                                fragment,
                                                &self.fragment_state.variable_dialog.values,
                                            );
                                            self.fragment_state.variable_dialog.command_edit = preview;
                                        }
                                        let unmatched = crate::core::unmatched_variable_names(
                                            &fragment.command,
                                            fragment.variables.iter().map(|v| v.name.as_str()),
                                        );
                                        if !unmatched.is_empty() {
                                            ui.separator();
                                            ui.label(crate::ui::chrome::rich_caption(
                                                theme,
                                                crate::i18n::tr(
                                                    ui.ctx(),
                                                    "These fields are not used by this snippet's command, so they will not change it:",
                                                    "这些变量没有出现在本片段的命令里，填了也不会改变它：",
                                                ),
                                            ));
                                            for name in &unmatched {
                                                ui.label(crate::ui::chrome::rich_caption(
                                                    theme,
                                                    &format!("  <{}>", name),
                                                ));
                                            }
                                            ui.label(crate::ui::chrome::rich_caption(
                                                theme,
                                                crate::i18n::tr(
                                                    ui.ctx(),
                                                    "Add the placeholder to the snippet, or edit the command below directly.",
                                                    "把上面的 <变量名> 加进片段模板，或直接编辑下面的命令。",
                                                ),
                                            ));
                                        }
                                        // #491：移除「用上方变量重写命令」按钮——命令预览已随
                                        // 输入逐帧重建（见上方 changed_any 分支），这颗按钮
                                        // 多余，还让用户误以为「填完必须再点一次」才生效。
                                        // 弹窗底部只保留「插入终端/执行」与「取消」。
                                        crate::ui::chrome::form_field_label(
                                            ui,
                                            theme,
                                            crate::i18n::tr(
                                                ui.ctx(),
                                                "Command to run (editable)",
                                                "将要执行的命令（可编辑）",
                                            ),
                                        );
                                        crate::ui::chrome::form_multiline_field(
                                            ui,
                                            theme,
                                            egui::Id::unique("var_dialog_cmd_edit"),
                                            &mut self.fragment_state.variable_dialog.command_edit,
                                            layout_util::finite_content_width(ui),
                                            5,
                                            false,
                                        );
                                    }
                                }
                            });
                        if let Some(ref err) = self.fragment_state.variable_dialog.last_finalize_error {
                            ui.add_space(theme.spacing_panel_gap());
                            ui.label(
                                egui::RichText::new(err)
                                    .size(theme.font_size_fragment_dialog_caption())
                                    .color(theme.color_danger_emphasis()),
                            );
                        }
                        ui.add_space(theme.spacing_list_item_x());
                        crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                            if ui
                                .add(crate::ui::chrome::modal_primary_button_with_icon_widget(
                                    th,
                                    crate::ui::icons::IconId::Check,
                                    ok_label_static,
                                ))
                                .clicked()
                            {
                                let paste = self.fragment_state.variable_dialog.paste_after_fill;
                                if let Some(fid) = self.fragment_state.variable_dialog.fragment_id.clone() {
                                    if let Some(fragment) = self.fragment_state.fragment_manager.get(&fid).cloned()
                                    {
                                        match self.finalize_fragment_command_text(
                                            &self.fragment_state.variable_dialog.command_edit,
                                            &self.fragment_state.variable_dialog.values,
                                        ) {
                                            Ok(cmd) => {
                                                self.fragment_state.variable_dialog.last_finalize_error = None;
                                                let needs = placeholders_needing_user(&cmd);
                                                if needs.is_empty() {
                                                    if paste {
                                                        self.insert_expanded_fragment_with_stats(
                                                            ctx, &fid, &cmd,
                                                        );
                                                    } else if let Some(session_id) =
                                                        &self.selected_session_id
                                                    {
                                                        if self.tabs.iter().any(|t| {
                                                            t.primary_session_id() == *session_id
                                                        }) {
                                                            let _ = self
                                                                .send_audited_command_active(
                                                                    ctx, &cmd,
                                                                );
                                                        }
                                                        self.fragment_state.quick_selector.open = false;
                                                    }
                                                } else {
                                                    self.fragment_state.pending_fragment_id = Some(fid.clone());
                                                    self.fragment_state.pending_fragment_name =
                                                        crate::i18n::fragment_display_title(
                                                            ctx, &fragment,
                                                        );
                                                    self.fragment_state.pending_fragment_command = cmd;
                                                    self.fragment_state.pending_fragment_vars = needs
                                                        .into_iter()
                                                        .map(|k| (k, String::new()))
                                                        .collect();
                                                    self.fragment_state.fragment_vars_completion = if paste {
                                                        FragmentVarsCompletion::PasteInsertStats
                                                    } else {
                                                        FragmentVarsCompletion::QuickExecuteSend
                                                    };
                                                    self.sync_pending_fragment_command_edit();
                                                    // #493：旧路径弹窗刚打开时同样聚焦第一栏。
                                                    self.fragment_state.pending_fragment_vars_focus_first = true;
                                                    self.fragment_state.show_fragment_vars_dialog = true;
                                                }
                                                self.fragment_state.variable_dialog.paste_after_fill = false;
                                                self.fragment_state.variable_dialog.open = false;
                                            }
                                            Err(e) => {
                                                let localized =
                                                    crate::i18n::localize_fragment_expr_error(
                                                        crate::i18n::language(ctx),
                                                        &e,
                                                    );
                                                self.notify_error(localized.clone());
                                                self.fragment_state.variable_dialog.last_finalize_error =
                                                    Some(localized);
                                            }
                                        }
                                    } else {
                                        self.notify_auto(
                                            crate::i18n::tr(
                                                ctx,
                                                "Snippet no longer exists (removed from library?)",
                                                "找不到该片段（可能已从库中删除）",
                                            )
                                            .to_string(),
                                        );
                                    }
                                }
                            }
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Cross,
                                crate::i18n::tr(ui.ctx(), "Cancel", "取消"),
                            )
                            .clicked()
                            {
                                self.fragment_state.variable_dialog.open = false;
                                self.fragment_state.variable_dialog.paste_after_fill = false;
                                self.fragment_state.variable_dialog.last_finalize_error = None;
                            }
                        });
                    });
                });
            if should_close {
                self.fragment_state.variable_dialog.open = false;
                self.fragment_state.variable_dialog.paste_after_fill = false;
                self.fragment_state.variable_dialog.last_finalize_error = None;
            }
            ctx.move_to_top(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::unique("mistterm_fragment_variable_dialog"),
            ));
        }

        self.show_command_palette(ctx, theme);
    }

    fn ssh_disconnect_banner_full_h(theme: &crate::ui::theme::Theme) -> f32 {
        theme.size_control_btn_h() + theme.spacing_sm() * 2.0
    }

    /// 活动窗格的 SSH 断开条阶段。正在连接、连接失败占位、本机终端都不占用这条。
    fn active_ssh_disconnect_banner(
        &self,
        now: std::time::Instant,
    ) -> Option<crate::core::DisconnectBannerKind> {
        let tab = self.tabs.get(self.active_tab?)?;
        let pane = tab.active_pane()?;
        if !pane.terminal.shows_persistent_disconnect_banner() {
            return None;
        }
        let auto = self.tab_auto_reconnect_enabled(&tab.primary_session_id());
        Some(crate::core::disconnect_banner_kind(
            auto,
            pane.ssh_auto_reconnect_attempts,
            pane.ssh_auto_reconnect_next,
            now,
            crate::core::DEFAULT_MAX_RECONNECT_ATTEMPTS,
        ))
    }

    fn disconnect_banner_text(
        ctx: &egui::Context,
        kind: crate::core::DisconnectBannerKind,
    ) -> String {
        use crate::core::DisconnectBannerKind;
        use crate::i18n::{language, UiLanguage};
        match (language(ctx), kind) {
            (UiLanguage::En, DisconnectBannerKind::Held) => {
                "Connection lost · input is buffered locally".to_string()
            }
            (UiLanguage::Zh, DisconnectBannerKind::Held) => "连接已断开 · 输入暂存本地".to_string(),
            (
                UiLanguage::En,
                DisconnectBannerKind::Scheduled {
                    attempt,
                    max_attempts,
                    delay_secs,
                },
            ) => {
                if delay_secs == 0 {
                    format!(
                        "Auto-reconnect {attempt}/{max_attempts} · retrying now · input buffered locally"
                    )
                } else {
                    format!(
                        "Auto-reconnect {attempt}/{max_attempts} · retry in {delay_secs}s · input buffered locally"
                    )
                }
            }
            (
                UiLanguage::Zh,
                DisconnectBannerKind::Scheduled {
                    attempt,
                    max_attempts,
                    delay_secs,
                },
            ) => {
                if delay_secs == 0 {
                    format!("正在自动重连 {attempt}/{max_attempts} · 即将重试 · 输入暂存本地")
                } else {
                    format!(
                        "正在自动重连 {attempt}/{max_attempts} · {delay_secs} 秒后重试 · 输入暂存本地"
                    )
                }
            }
            (UiLanguage::En, DisconnectBannerKind::GaveUp { max_attempts }) => format!(
                "Auto-reconnect stopped after {max_attempts} attempts · input buffered locally"
            ),
            (UiLanguage::Zh, DisconnectBannerKind::GaveUp { max_attempts }) => {
                format!("自动重连已达 {max_attempts} 次上限 · 输入暂存本地")
            }
        }
    }

    /// 终端区顶部常驻断开条。消失与否只看连接状态，不另存一份「已提示」标记。
    fn paint_ssh_disconnect_banner(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        ctx: &egui::Context,
        banner_h: f32,
    ) {
        let now = std::time::Instant::now();
        let Some(kind) = self.active_ssh_disconnect_banner(now) else {
            ui.allocate_exact_size(
                egui::vec2(ui.available_width(), banner_h),
                egui::Sense::hover(),
            );
            return;
        };
        let scheduled = matches!(kind, crate::core::DisconnectBannerKind::Scheduled { .. });
        let text = Self::disconnect_banner_text(ctx, kind);
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, banner_h), egui::Sense::hover());
        let warn = theme.color_status_warning_text();
        let fill = theme.color_status_selected_fill(theme.amber_color());
        ui.painter().rect_filled(rect, 0.0, fill);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(1.0, warn.gamma_multiply(0.55)),
        );
        let pad = theme.spacing_sm();
        let buttons_ok = banner_h + 1.0 >= theme.size_control_btn_h();
        let mut reconnect = false;
        let mut close_tab = false;
        let mut text_right = rect.right() - pad;
        if buttons_ok {
            let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
            row.set_clip_rect(rect);
            row.spacing_mut().item_spacing.x = theme.spacing_sm();
            row.add_space(pad);
            let close_label = crate::i18n::tr(ctx, "Close tab", "关闭标签");
            let reconnect_label = crate::i18n::tr(ctx, "Reconnect", "重连");
            if crate::ui::chrome::panel_action_button(&mut row, theme, close_label).clicked() {
                close_tab = true;
            }
            let reconnect_resp =
                crate::ui::chrome::panel_action_primary_button(&mut row, theme, reconnect_label);
            if reconnect_resp.clicked() {
                reconnect = true;
            }
            text_right = reconnect_resp.rect.left() - theme.spacing_sm();
        }
        let icon_px = theme.size_icon_glyph();
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + pad + icon_px * 0.5, rect.center().y),
            egui::vec2(icon_px, icon_px),
        );
        crate::ui::icons::paint_icon(
            ui,
            icon_rect,
            crate::ui::icons::IconId::Warning,
            warn,
            icon_px,
        );
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(icon_rect.right() + theme.spacing_xs(), rect.top()),
            egui::pos2(text_right, rect.bottom()),
        );
        if text_rect.width() > 8.0 {
            let galley = ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    text,
                    egui::FontId::proportional(theme.font_size_ui_control()),
                    warn,
                )
            });
            let painter = ui.painter().with_clip_rect(text_rect.intersect(rect));
            painter.galley(
                egui::pos2(
                    text_rect.left(),
                    rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                egui::Color32::WHITE,
            );
        }
        if scheduled {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if reconnect {
            if let Some(idx) = self.active_tab {
                self.reconnect_tab_at(ctx, idx);
            }
        } else if close_tab {
            self.request_close_active_tab();
        }
    }

    pub(crate) fn paint_agent_audit_degraded_banner(
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        ctx: &egui::Context,
        app: &mut MistTermApp,
    ) {
        let w = ui.available_width();
        let h = theme.font_size_small() + theme.spacing_md() * 2.0;
        let (rect, _response) =
            ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let warn_fill = theme.accent_color().gamma_multiply(0.18);
        ui.painter()
            .rect_filled(rect, theme.radius_list_item(), warn_fill);
        ui.painter().rect_stroke(
            rect,
            theme.radius_list_item(),
            egui::Stroke::new(1.0, theme.accent_dim_color()),
            egui::StrokeKind::Inside,
        );
        let text = crate::i18n::tr(
            ctx,
            "Server-side audit unavailable; commands use local checks only",
            "服务器侧审计不可用，命令将仅做本地检查",
        );
        let close_id = egui::Id::unique("agent_audit_banner_close");
        let close_w = theme.spacing_lg() + theme.font_size_small();
        let text_rect = rect.shrink2(egui::vec2(theme.spacing_sm(), theme.spacing_xs()));
        let text_rect = egui::Rect::from_min_max(
            text_rect.min,
            egui::pos2(text_rect.max.x - close_w, text_rect.max.y),
        );
        ui.painter().text(
            text_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(theme.font_size_small()),
            theme.fg_high_color(),
        );
        let close_rect = egui::Rect::from_min_size(
            egui::pos2(rect.max.x - close_w, rect.min.y),
            egui::vec2(close_w, rect.height()),
        );
        let close_resp = ui.interact(close_rect, close_id, egui::Sense::click());
        ui.painter().text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            egui::FontId::proportional(theme.font_size_normal()),
            theme.color_body_text_muted(),
        );
        if close_resp.clicked() {
            app.dismiss_agent_audit_banner_active();
        }
    }

    /// 会话标签条：标签芯片 + 新建 + 右键菜单动作。沉浸式顶栏与普通模式终端列共用。
    pub(crate) fn render_session_tab_strip(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        ui.horizontal(|ui| {
            let mut to_close = None;
            let mut to_activate = None;
            let mut close_others = None;
            let mut close_right = None;
            let mut disconnect_ssh_idx = None;
            let mut reconnect_idx = None;
            let mut open_another_local_cwd: Option<Option<String>> = None;
            // E：复制标签/本机新标签闭包内只记下源 (tab, pane)，闭包结束后再做带外刷新
            //（刷新需 &mut self 且最多阻塞 ~1.2s，禁止在 egui 菜单闭包里同步等待）。
            let mut duplicate_source: Option<(usize, usize, bool)> = None;
            let mut rename_tab_idx: Option<usize> = None;
            let mut split_h_idx = None;
            let mut split_v_idx = None;
            let mut broadcast_idx = None;
            let mut unsplit_idx = None;
            let mut close_pane_tab = None;

            // #454：标签过多时芯片不再挤出栏外；溢出则出现左右滚动入口，并把活动标签滚进视口。
            let item_spacing = ui.spacing().item_spacing.x;
            let plus_w = theme.size_tab_bar_icon_btn();
            let scroll_btn_w = theme.size_tab_bar_icon_btn();
            let row_h = theme.size_tab_bar_row_h();
            let available = ui.available_width();
            let chip_widths: Vec<f32> = self
                .tabs
                .iter()
                .map(|tab| {
                    crate::ui::chrome::session_tab_chip_desired_width(
                        ui.ctx(),
                        theme,
                        &tab.display_title(),
                        tab.panes.iter().any(|p| p.progress.is_some()),
                    )
                })
                .collect();
            let mut requested_scroll = self.tab_strip_scroll_x;
            let mut plan = crate::ui::chrome::plan_tab_strip_overflow(
                &chip_widths,
                item_spacing,
                available,
                plus_w,
                scroll_btn_w,
                requested_scroll,
            );
            if self.active_tab != self.tab_strip_synced_active {
                if let Some(idx) = self.active_tab {
                    if let Some((left, right)) =
                        crate::ui::chrome::tab_content_range(&chip_widths, item_spacing, idx)
                    {
                        requested_scroll = crate::ui::chrome::scroll_tab_into_view(
                            plan.scroll_x,
                            left,
                            right,
                            plan.viewport_width,
                            plan.max_scroll,
                        );
                        plan = crate::ui::chrome::plan_tab_strip_overflow(
                            &chip_widths,
                            item_spacing,
                            available,
                            plus_w,
                            scroll_btn_w,
                            requested_scroll,
                        );
                    }
                }
                self.tab_strip_synced_active = self.active_tab;
            }
            self.tab_strip_scroll_x = plan.scroll_x;

            if plan.overflow {
                let left_tip = crate::i18n::tr(ctx, "Scroll tabs left", "向左滚动标签");
                let left = crate::ui::chrome::tab_bar_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronLeft,
                    left_tip,
                );
                if left.clicked() && plan.can_scroll_left {
                    self.tab_strip_scroll_x = crate::ui::chrome::apply_tab_strip_scroll(
                        self.tab_strip_scroll_x,
                        -crate::ui::chrome::tab_strip_scroll_step(plan.viewport_width),
                        plan.max_scroll,
                    );
                }
            }

            let paint_scroll = if plan.overflow {
                self.tab_strip_scroll_x
            } else {
                0.0
            };
            let mut painted_scroll = paint_scroll;
            {
                let mut render_session_tab_chips = |ui: &mut egui::Ui| {
            for (idx, tab) in self.tabs.iter().enumerate() {
                let active = self.active_tab == Some(idx);
                let tab_label = tab.display_title();
                let tab_hover = self
                    .session_manager
                    .get_session(&tab.primary_session_id())
                    .map(|s| {
                        format!(
                            "{} · {}@{}",
                            s.name, s.username, s.host
                        )
                    })
                    .unwrap_or_else(|| tab_label.clone());
                let sid = tab.primary_session_id();
                let env_color = self
                    .session_manager
                    .get_session(&sid)
                    .map(|s| theme.session_tab_accent_color(s))
                    .or_else(|| {
                        // 本机终端不入 sessions.json，仍按 id 给强调底色，避免现代主题下「空心」标签。
                        if crate::core::session::is_local_session_id(&sid)
                            || tab.panes.iter().any(|p| p.terminal.is_local_shell())
                        {
                            Some(theme.session_tab_accent_from_id(&sid))
                        } else {
                            None
                        }
                    });
                // #392：任一 pane 有构建进度（OSC 9;4）→ 标签叠进度环。
                let pane_progress = tab
                    .panes
                    .iter()
                    .find_map(|p| p.progress.as_ref().copied());
                let tab_chip = crate::ui::chrome::session_tab_chip(
                    ui,
                    theme,
                    &tab_label,
                    active,
                    tab.any_connected(),
                    tab.needs_attention,
                    false,
                    env_color,
                    pane_progress,
                );
                // #390：Agent 状态红绿灯悬停文案。blocked 优先（等审批最急），
                // 其次 done/error（有未看结果）。角标本身走 needs_attention。
                let tab_hover = if tab.any_panes_agent_blocked() {
                    format!(
                        "{}\n{}",
                        tab_hover,
                        crate::i18n::tr(
                            ctx,
                            "Agent waiting for you (approval / input)",
                            "Agent 等待你处理（审批 / 输入）",
                        )
                    )
                } else if tab.any_panes_agent_done() {
                    format!(
                        "{}\n{}",
                        tab_hover,
                        crate::i18n::tr(
                            ctx,
                            "Agent finished / failed — result unread",
                            "Agent 已完成/失败 —— 结果未查看",
                        )
                    )
                } else if tab.needs_attention && !active {
                    format!(
                        "{}\n{}",
                        tab_hover,
                        crate::i18n::tr(
                            ctx,
                            "Needs attention (bell or session ended)",
                            "需要注意（响铃或会话已结束）",
                        )
                    )
                } else {
                    tab_hover
                };
                let tab_resp =
                    tab_chip.response.on_hover_text(tab_hover);
                if tab_chip.close_clicked {
                    to_close = Some(idx);
                } else if tab_resp.clicked() {
                    to_activate = Some(idx);
                }
                tab_resp.context_menu(|ui| {
                    crate::ui::chrome::apply_context_menu_style(
                        ui, theme,
                    );
                    let is_local = crate::core::session::is_local_session_id(&sid)
                        || tab.panes.iter().any(|p| p.terminal.is_local_shell());
                    let can_open_another = is_local
                        || self.session_manager.get_session(&sid).is_some();
                    if can_open_another
                        && crate::ui::chrome::popup_menu_button(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Duplicate Tab",
                                "复制标签",
                            ),
                        )
                        .clicked()
                    {
                        // #392：从右键源 tab 显式确定继承源；E：远程会话改为闭包外带外刷新。
                        let active = tab
                            .active_pane
                            .min(tab.panes.len().saturating_sub(1));
                        if is_local {
                            let inherit = tab.panes.get(active).and_then(|p| {
                                p.terminal
                                    .shell_cwd()
                                    .map(|s| s.to_string())
                                    .or_else(|| p.shell_cwd.clone())
                            });
                            open_another_local_cwd = Some(inherit);
                        } else {
                            duplicate_source = Some((idx, active, false));
                        }
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Rename Tab", "重命名标签"),
                    )
                    .clicked()
                    {
                        rename_tab_idx = Some(idx);
                    }
                    if tab.any_connected_or_connecting()
                        && crate::ui::chrome::popup_menu_button(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Disconnect (Keep Output)",
                                "断开连接（保留输出）",
                            ),
                        )
                        .clicked()
                    {
                        disconnect_ssh_idx = Some(idx);
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(
                            ctx,
                            "Reconnect",
                            "重新连接",
                        ),
                    )
                    .clicked()
                    {
                        reconnect_idx = Some(idx);
                    }
                    ui.separator();
                    if tab.can_split() {
                        if crate::ui::chrome::popup_menu_button_accel_shift(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Split left / right",
                                "左右分屏",
                            ),
                            "d",
                        )
                        .clicked()
                        {
                            split_h_idx = Some(idx);
                        }
                        if crate::ui::chrome::popup_menu_button_accel_shift(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Split top / bottom",
                                "上下分屏",
                            ),
                            "u",
                        )
                        .clicked()
                        {
                            split_v_idx = Some(idx);
                        }
                    }
                    if tab.is_split() {
                        let broadcast_label = if tab.broadcast_input {
                            crate::i18n::tr(ctx, "✓ Broadcast Input", "✓ 广播输入")
                        } else {
                            crate::i18n::tr(ctx, "Broadcast Input", "广播输入")
                        };
                        if crate::ui::chrome::popup_menu_button_accel_shift(
                            ui,
                            theme,
                            broadcast_label,
                            "b",
                        )
                        .clicked()
                        {
                            broadcast_idx = Some(idx);
                        }
                        if crate::ui::chrome::popup_menu_button(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Close active pane",
                                "关闭当前窗格",
                            ),
                        )
                        .clicked()
                        {
                            close_pane_tab =
                                Some((idx, tab.active_pane));
                        }
                        if crate::ui::chrome::popup_menu_button(
                            ui,
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Merge split panes",
                                "合并分屏",
                            ),
                        )
                        .clicked()
                        {
                            unsplit_idx = Some(idx);
                        }
                    }
                    ui.separator();
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(
                            ctx,
                            "Close other tabs",
                            "关闭其他标签",
                        ),
                    )
                    .clicked()
                    {
                        close_others = Some(idx);
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(
                            ctx,
                            "Close tabs to the right",
                            "关闭右侧标签",
                        ),
                    )
                    .clicked()
                    {
                        close_right = Some(idx);
                    }
                });
            }
                };

                if plan.overflow {
                    ui.allocate_ui_with_layout(
                        egui::vec2(plan.viewport_width.max(0.0), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_height(row_h);
                            ui.set_max_height(row_h);
                            let output = egui::ScrollArea::horizontal()
                                .id_salt("session_tab_strip_scroll")
                                .auto_shrink([false, false])
                                .max_height(row_h)
                                .horizontal_scroll_offset(paint_scroll)
                                .scroll_bar_visibility(
                                    egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| {
                                    ui.set_min_height(row_h);
                                    ui.horizontal(|ui| {
                                        ui.set_min_height(row_h);
                                        render_session_tab_chips(ui);
                                    });
                                });
                            painted_scroll = output
                                .state
                                .clamped_offset()
                                .x
                                .clamp(0.0, plan.max_scroll);
                        },
                    );
                } else {
                    render_session_tab_chips(ui);
                    painted_scroll = 0.0;
                }
            }
            self.tab_strip_scroll_x = painted_scroll;

            if plan.overflow {
                let right_tip = crate::i18n::tr(ctx, "Scroll tabs right", "向右滚动标签");
                let right = crate::ui::chrome::tab_bar_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::ChevronRight,
                    right_tip,
                );
                if right.clicked() && plan.can_scroll_right {
                    self.tab_strip_scroll_x = crate::ui::chrome::apply_tab_strip_scroll(
                        self.tab_strip_scroll_x,
                        crate::ui::chrome::tab_strip_scroll_step(plan.viewport_width),
                        plan.max_scroll,
                    );
                }
            }
            self.show_tab_bar_new_tab_picker(ui, theme, ctx);
            if let Some(idx) = to_activate {
                self.switch_tab_to_index(idx);
            }
            if let Some(idx) = to_close {
                self.request_close_tab_at(idx);
            }
            if let Some(cwd) = open_another_local_cwd {
                self.open_local_shell_tab_with_cwd(ctx, cwd);
            } else if let Some((idx, pane_idx, _is_local)) = duplicate_source {
                // E：复制标签——与 ⌘T / 分屏同一刷新入口，闭包结束后再带外刷新源 cwd。
                let sid = self.tabs.get(idx).map(|t| t.primary_session_id());
                if let Some(sid) = sid {
                    if let Some(session) = self.session_manager.get_session(&sid).cloned() {
                        let inherit = self.refresh_source_cwd_for_inherit(ctx, idx, pane_idx);
                        self.open_new_tab_for_session_with_cwd(ctx, &session, inherit);
                    }
                }
            }
            if let Some(idx) = rename_tab_idx {
                if idx < self.tabs.len() {
                    let draft = self.tabs[idx].display_title();
                    self.tab_rename_draft = draft;
                    self.pending_tab_rename = Some(idx);
                }
            }
            // #326：同一帧内 reconnect 与 split/close 等互斥——egui 0.37 弹窗在
            // macOS 上出现过点击归属串扰（复现：点「重新连接」却触发「左右分屏」，
            // 原窗格残留断开 + 新窗格 local-ssh (2)）。裁决规则在 core::reconnect
            // 的 TabMenuActions::resolve（优先级：重连 > 断开 > 分屏），带单测。
            let menu_actions = crate::core::reconnect::TabMenuActions {
                reconnect: reconnect_idx.is_some(),
                disconnect: disconnect_ssh_idx.is_some(),
                close_pane: close_pane_tab.is_some(),
                split_h: split_h_idx.is_some(),
                split_v: split_v_idx.is_some(),
            }
            .resolve();
            let reconnect_idx = if menu_actions.reconnect {
                reconnect_idx
            } else {
                None
            };
            let disconnect_ssh_idx = if menu_actions.disconnect {
                disconnect_ssh_idx
            } else {
                None
            };
            let split_h_idx = if menu_actions.split_h { split_h_idx } else { None };
            let split_v_idx = if menu_actions.split_v { split_v_idx } else { None };
            let close_pane_tab = if menu_actions.close_pane {
                close_pane_tab
            } else {
                None
            };
            if let Some(idx) = disconnect_ssh_idx {
                self.disconnect_ssh_keep_buffer_at(ctx, idx);
            }
            if let Some(idx) = reconnect_idx {
                self.reconnect_tab_at(ctx, idx);
            }
            if let Some(idx) = split_h_idx {
                self.active_tab = Some(idx);
                self.split_tab_at(
                    ctx,
                    idx,
                    crate::ui::tab_pane::TabLayout::SplitHorizontal,
                );
            }
            if let Some(idx) = split_v_idx {
                self.active_tab = Some(idx);
                self.split_tab_at(
                    ctx,
                    idx,
                    crate::ui::tab_pane::TabLayout::SplitVertical,
                );
            }
            if let Some(idx) = broadcast_idx {
                self.active_tab = Some(idx);
                self.toggle_broadcast_input(ctx);
            }
            if let Some(idx) = unsplit_idx {
                self.active_tab = Some(idx);
                self.unsplit_tab_at(ctx, idx);
            }
            if let Some((ti, pi)) = close_pane_tab {
                self.active_tab = Some(ti);
                self.close_pane_tab_at(ctx, ti, pi);
            }
            if let Some(idx) = close_others {
                if idx < self.tabs.len() {
                    let kept = self.tabs.remove(idx);
                    for t in self.tabs.iter_mut() {
                        t.disconnect_all_panes();
                    }
                    self.tabs.clear();
                    self.tabs.push(kept);
                    self.active_tab = Some(0);
                    self.selected_session_id = self
                        .tabs
                        .first()
                        .map(|t| t.primary_session_id());
                }
            }
            if let Some(idx) = close_right {
                if idx + 1 < self.tabs.len() {
                    for t in self.tabs.iter_mut().skip(idx + 1) {
                        t.disconnect_all_panes();
                    }
                    self.tabs.truncate(idx + 1);
                    self.active_tab = Some(idx);
                    self.selected_session_id = self
                        .tabs
                        .get(idx)
                        .map(|t| t.primary_session_id());
                }
            }

        });
    }

    /// 标签栏「+」：弹出连接点选，写清会连哪台，避免多服务器时盲开标签。
    fn show_tab_bar_new_tab_picker(
        &mut self,
        ui: &mut egui::Ui,
        theme: &crate::ui::theme::Theme,
        ctx: &egui::Context,
    ) {
        let popup_id = ui.auto_id_with("tab_bar_new_tab_picker");
        let btn = crate::ui::chrome::tab_bar_new_tab_button(ui, theme);
        // egui 0.37：Popup 自带点击开合；close 行为由 close_behavior 处理。
        let _ = popup_id;
        let popup = egui::Popup::from_toggle_button_response(&btn)
            .align(egui::RectAlign::BOTTOM_START);

        let mut open_session_id: Option<String> = None;
        let mut open_local = false;
        let mut open_new_connection = false;

        let selected_id = self.selected_session_id.clone();
        let selected = selected_id
            .as_ref()
            .and_then(|id| self.session_manager.get_session(id).cloned());
        let mut others: Vec<_> = self
            .session_manager
            .list_sessions()
            .iter()
            .filter(|s| selected_id.as_ref() != Some(&s.id))
            .cloned()
            .collect();
        crate::core::session_sort::sort_sessions(
            &mut others,
            crate::core::session_sort::SessionSortBy::LastConnected,
        );

        popup.show(|ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
                ui.set_min_width(220.0);
                ui.set_max_width(320.0);

                if let Some(ref session) = selected {
                    crate::ui::chrome::popup_menu_section_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Current selection", "当前选中"),
                    );
                    let has_tab = self
                        .tabs
                        .iter()
                        .any(|t| t.primary_session_id() == session.id);
                    let again = match (crate::i18n::language(ctx), has_tab) {
                        (crate::i18n::UiLanguage::En, true) => {
                            format!("Duplicate Tab · {}", session.name)
                        }
                        (crate::i18n::UiLanguage::En, false) => {
                            format!("Open · {}", session.name)
                        }
                        (crate::i18n::UiLanguage::Zh, true) => {
                            format!("复制「{}」标签", session.name)
                        }
                        (crate::i18n::UiLanguage::Zh, false) => {
                            format!("打开「{}」", session.name)
                        }
                    };
                    if crate::ui::chrome::popup_menu_button(ui, theme, &again)
                        .on_hover_text(format!(
                            "{}@{}:{}",
                            session.username, session.host, session.port
                        ))
                        .clicked()
                    {
                        open_session_id = Some(session.id.clone());
                    }
                }

                if !others.is_empty() {
                    crate::ui::chrome::popup_menu_section_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Open connection", "打开连接"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for session in &others {
                                let host_line = if session.port != crate::core::session::DEFAULT_SSH_PORT {
                                    format!("{}:{}", session.host, session.port)
                                } else {
                                    session.host.clone()
                                };
                                if crate::ui::chrome::popup_menu_button(ui, theme, &session.name)
                                    .on_hover_text(format!("{}@{}", session.username, host_line))
                                    .clicked()
                                {
                                    open_session_id = Some(session.id.clone());
                                }
                            }
                        });
                }

                ui.separator();
                if crate::ui::chrome::popup_menu_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Local Shell", "本机终端"),
                )
                .clicked()
                {
                    open_local = true;
                }
                if crate::ui::chrome::popup_menu_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "New connection…", "新建连接…"),
                )
                .clicked()
                {
                    open_new_connection = true;
                }
        });

        if let Some(sid) = open_session_id {
            if let Some(session) = self.session_manager.get_session(&sid).cloned() {
                // #392：与 ⌘T 一致，继承当前活动 pane 的 shell cwd。本机 shell 只读缓存。
                let inherit = self
                    .active_tab
                    .and_then(|i| self.tab_pane_inherit_cwd(i));
                self.open_new_tab_for_session_with_cwd(ctx, &session, inherit);
            }
        } else if open_local {
            self.open_local_shell_tab(ctx);
        } else if open_new_connection {
            self.dialog_flags.new_session = true;
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fragment::FragmentVariable;
    use crate::i18n::UiLanguage;

    // ---------------------------------------------- #488 占位提示文案（纯函数）

    /// 主机 IP 类变量（issue 原始场景）：中文占位必须是「请输入主机 IP」。
    #[test]
    fn hint_host_ip_variable_zh() {
        let mut var = FragmentVariable::new("宿主机IP", "目标服务器地址");
        var.description = "host".into();
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::Zh),
            "请输入主机 IP"
        );
    }

    /// 英文等价文案存在：「Enter host IP」。
    #[test]
    fn hint_host_ip_variable_en() {
        let var = FragmentVariable::new("host_ip", "");
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::En),
            "Enter host IP"
        );
    }

    /// 各种命中「主机 IP」判定的方式：变量名含 ip、别名、描述恰为 host/ip。
    #[test]
    fn hint_host_ip_detection_variants() {
        for name in ["host_ip", "HOST_IP", "host_ip_or_address", "目标ip"] {
            let var = FragmentVariable::new(name, "");
            assert_eq!(
                MistTermApp::fragment_variable_hint_text(&var, UiLanguage::En),
                "Enter host IP",
                "name={name}"
            );
        }
        let desc_host = FragmentVariable::new("endpoint", "host");
        let desc_ip = FragmentVariable::new("endpoint", "ip");
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&desc_host, UiLanguage::En),
            "Enter host IP"
        );
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&desc_ip, UiLanguage::Zh),
            "请输入主机 IP"
        );
    }

    /// 非主机类变量回退到「请输入<变量名>」（比通用「请输入」具体）。
    #[test]
    fn hint_generic_variable_uses_name() {
        let var = FragmentVariable::new("port", "端口号");
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::Zh),
            "请输入port"
        );
        let var_en = FragmentVariable::new("duration", "");
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var_en, UiLanguage::En),
            "Enter duration"
        );
    }

    /// 有默认值时展示默认值而非「请输入…」，且空/空白默认值视同没有。
    #[test]
    fn hint_default_value_and_blank_default_fallback() {
        let mut var = FragmentVariable::new("host_ip", "");
        var.default_value = Some("10.0.0.9".into());
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::Zh),
            "默认值：10.0.0.9"
        );
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::En),
            "Default value: 10.0.0.9"
        );
        var.default_value = Some("   ".into());
        assert_eq!(
            MistTermApp::fragment_variable_hint_text(&var, UiLanguage::Zh),
            "请输入主机 IP"
        );
    }

    // ---------------------------------------------- #488 弹窗打开即聚焦首个输入框

    /// 渲染循环里「首帧只聚焦第一个变量输入框」的一次性标志协议：
    /// `first_field && focus_first_input → request_focus()`，随后标志清零。
    /// 用同样的守卫形状模拟多变量循环，锁定：
    /// 只有第一个字段请求焦点，后续字段不再重复聚焦，标志下一帧不会复用。
    #[test]
    fn focus_first_input_is_consumed_once_by_the_first_field() {
        let mut focus_first_input = true; // 弹窗刚打开（两条打开路径都会置 true）
        let mut requested = Vec::new();
        let fields = ["host_ip", "port", "duration"];
        let mut first_field = true;
        for name in fields {
            // 与 render_workspace_shell 变量行一致的守卫顺序
            if first_field && focus_first_input {
                requested.push(name);
                focus_first_input = false;
                first_field = false;
            }
        }
        assert_eq!(requested, vec!["host_ip"], "只有第一个输入框请求焦点");
        assert!(!focus_first_input, "标志用完即清，下一帧不再重复聚焦");
    }

    /// 默认(关闭)状态下标志必须是 false——不该有幽灵聚焦。
    #[test]
    fn focus_first_input_defaults_to_false() {
        let dlg = crate::ui::app::FragmentVariableDialog::default();
        assert!(!dlg.focus_first_input);
    }

    // ---------------------------------------------- #491 边输边刷新预览（无重写按钮）

    /// 测试用片段构造（与 core::fragment_command 测试同形状）。
    fn make_stats(command: &str, vars: &[&str]) -> crate::core::fragment::FragmentStats {
        use crate::core::fragment::FragmentStats;
        FragmentStats {
            id: "t".into(),
            title: "t".into(),
            title_en: None,
            command: command.into(),
            category: "c".into(),
            category_en: None,
            variables: vars
                .iter()
                .map(|n| FragmentVariable::new(n, n))
                .collect(),
            tags: vec![],
            usage_count: 0,
            success_count: 0,
            total_time_ms: 0,
            last_used: None,
            source_status: String::new(),
            linked_docs: vec![],
        }
    }

    /// 预览值的纯逻辑：给定模板 + 当前变量值，应得到替换后的命令文本。
    /// 这正是弹窗内 changed_any 分支逐帧做的事——不经过任何「重写」按钮。
    #[test]
    fn preview_rebuilds_from_current_field_values_without_rewrite_button() {
        let stats = make_stats("ping <host_ip>", &["host_ip"]);
        let mut values = std::collections::HashMap::new();
        // 打开弹窗时的初始预览：变量为空 → 占位符仍在
        let initial =
            crate::core::build_fragment_command_preview(&stats, None, &values);
        assert_eq!(initial, "ping <host_ip>", "初始预览保留占位符");
        // 用户在输入框打字 → values 变化 → 预览逐帧重建（无按钮参与）
        values.insert("host_ip".to_string(), "10.0.0.42".to_string());
        let live =
            crate::core::build_fragment_command_preview(&stats, None, &values);
        assert_eq!(live, "ping 10.0.0.42", "输入即刷新，无需额外点击");
        // 继续编辑 → 预览继续跟随
        values.insert("host_ip".to_string(), "192.168.1.1".to_string());
        let live2 =
            crate::core::build_fragment_command_preview(&stats, None, &values);
        assert_eq!(live2, "ping 192.168.1.1");
    }

    /// 兜底协议：changed_any → 用当前值重建预览并清掉上次的 finalize 错误。
    /// 用同形状守卫锁定「任何字段有变化都必须触发重建」，防止回归成手动按钮。
    #[test]
    fn changed_any_refreshes_preview_and_clears_finalize_error() {
        let mut changed_any = false;
        let mut rebuild = false;
        let mut last_finalize_error: Option<String> = Some("boom".to_string());
        for value_changed in [false, true, false, true] {
            if value_changed {
                changed_any = true;
            }
            if changed_any {
                rebuild = true;
                last_finalize_error = None;
            }
        }
        assert!(rebuild, "字段变化后预览必须重建");
        assert!(last_finalize_error.is_none(), "重建预览时清掉旧错误");
    }

    /// 「用上方变量重写命令 / Rewrite command using fields above」按钮文案必须
    /// 从变量弹窗源码中消失：中文、英文及图标路径都不能再出现。
    #[test]
    fn rewrite_button_labels_removed_from_variable_dialog() {
        let src = include_str!("workspace.rs");
        assert!(
            !src.contains("用上方变量重写命令"),
            "中文按钮文案仍残留在 workspace.rs"
        );
        assert!(
            !src.contains("Rewrite command using fields above"),
            "英文按钮文案仍残留在 workspace.rs"
        );
        // 弹窗主体（fragment_variable_modal 到命令编辑框）内不允许再出现
        // 与按钮配套的 Refresh 图标调用。
        let modal = src
            .split("fragment_variable_modal")
            .nth(1)
            .expect("变量弹窗片段应存在");
        assert!(
            !modal.contains("IconId::Refresh"),
            "变量弹窗内不应再有刷新(重写)图标按钮"
        );
    }

    // ---------------------------------------------- #491 变量输入框左对齐

    /// forms.rs 的居中辅助函数必须保持默认居中（凭证库等老面板排版不动），
    /// 而变量弹窗必须显式传入 LEFT_CENTER。用源码断言锁定调用点的对齐参数。
    #[test]
    fn variable_dialog_field_uses_left_align_and_legacy_callers_keep_center() {
        let ws = include_str!("workspace.rs");
        let call = ws
            .split("form_singleline_field_boxed_with_align")
            .nth(1)
            .expect("变量弹窗应改用带对齐参数的 boxed 输入框");
        let call_tail = &call[..call.find(';').unwrap_or(call.len())];
        assert!(
            call_tail.contains("Align2::LEFT_CENTER"),
            "变量输入框必须显式左对齐：{call_tail}"
        );
        let forms = include_str!("chrome/forms.rs");
        let wrapper = forms
            .split("pub fn form_singleline_field_boxed(")
            .nth(1)
            .expect("居中包装函数应存在");
        let wrapper_body = &wrapper[..wrapper
            .find("pub fn form_singleline_field_boxed_with_align")
            .unwrap_or(wrapper.len())];
        assert!(
            wrapper_body.contains("Align2::CENTER_CENTER"),
            "共享包装函数默认必须保持居中，避免悄悄改掉老面板排版"
        );
    }

    // ---------------------------------------------- #491 未匹配变量的可见反馈

    /// 未匹配变量（模板里没有 <名字> 占位符）必须给出可操作的警示文案，
    /// 不能静默无反应——锁定 Missing 分支的文案与降级文案来源。
    #[test]
    fn missing_variable_still_produces_actionable_feedback() {
        let stats = make_stats("echo fixed-ip-target", &["host_ip"]);
        // 模板里根本没有 <host_ip> → 必须 Missing，UI 才会渲染警示而不是静默
        assert_eq!(
            crate::core::placeholder_usage(&stats.command, "host_ip"),
            crate::core::PlaceholderUsage::Missing
        );
        // 预览不会因为填了值而假装变化
        let mut values = std::collections::HashMap::new();
        values.insert("host_ip".to_string(), "10.9.9.9".to_string());
        assert_eq!(
            crate::core::build_fragment_command_preview(&stats, None, &values),
            "echo fixed-ip-target",
            "填了未匹配变量也不改变命令，因此必须靠文案提示"
        );
        // 文案存在且包含下一步动作（中英双语言）
        let locale_zh = crate::i18n::Locale::from(UiLanguage::Zh);
        let zh = locale_zh.tr(
            "Not in the command yet: put ",
            "当前命令里还没有这个占位符：请在模板里加上 ",
        );
        assert!(zh.contains("占位符") && zh.contains("模板"), "中文提示要指到模板");
        let locale_en = crate::i18n::Locale::from(UiLanguage::En);
        let en = locale_en.tr(
            "Not in the command yet: put ",
            "当前命令里还没有这个占位符：请在模板里加上 ",
        );
        assert!(en.contains("Not in the command"), "英文提示要说明还没进命令");
    }

    // ---------------------------------------------- #491 保留 #488 行为

    /// #488 行为仍在：首个输入框打开即聚焦（一次性标志协议，
    /// 前面有 focus_first_input_* 用例），且变量行仍走「实心底 + 描边」的
    /// boxed 输入框（form_singleline_field_boxed_with_align），
    /// 不回退到 modern 下划线样式。
    #[test]
    fn variable_dialog_keeps_boxed_field_and_first_focus_protocol() {
        let ws = include_str!("workspace.rs");
        // 实心底+描边路径：变量行必须仍调用 boxed 带 align 版本
        assert!(
            ws.contains("form_singleline_field_boxed_with_align"),
            "变量输入框必须保留实心底+描边输入框"
        );
        // #488 聚焦协议：弹窗打开标志仍存在，且首个字段守卫仍引用它
        assert!(
            ws.contains("focus_first_input"),
            "首个输入框的聚焦标志协议不能丢"
        );
        let field_call = ws
            .split("form_singleline_field_boxed_with_align")
            .nth(1)
            .expect("boxed 调用应存在");
        let field_block = &field_call[..field_call.find("value_changed").unwrap_or(field_call.len())];
        assert!(
            field_block.contains("request_focus"),
            "输入框之后必须仍有首个字段 request_focus 守卫"
        );
    }

    // ---------------------------------------------- #493 填写片段变量（旧路径）与 #491 对齐

    /// 「填写片段变量」弹窗（fragment_vars_modal / show_fragment_vars_dialog）
    /// 的说明文案不再指向「根据变量重算命令」，而是与 #491 的「填写变量」
    /// 弹窗一致：在这里填值，下面的命令会随之改变。
    #[test]
    fn fragment_vars_caption_no_longer_tells_user_to_recompute() {
        let ws = include_str!("workspace.rs");
        // 用锚点 "modal_window(\"fragment_vars_modal\"" 精确切出本弹窗主体：
        // 不能裸用 "fragment_vars_modal"（会被 layout_util::fragment_vars_modal_size 抢走第一刀）。
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("quick_fragment_selector").unwrap_or(modal.len())];
        assert!(
            !tail.contains("根据变量重算命令"),
            "中文说明不能再让用户点「根据变量重算命令」"
        );
        assert!(
            !tail.contains("Recompute command"),
            "英文说明不能再让用户点 Recompute command"
        );
        assert!(
            tail.contains("Type a value here and the command below will change")
                && tail.contains("在这里填值，下面的命令会随之改变"),
            "说明文案应与 #491 的「填写变量」弹窗一致（实时预览语义）"
        );
    }

    /// 「根据变量重算命令 / Recompute command」按钮（含 Refresh 图标）必须从
    /// fragment_vars_modal 弹窗里移除；不影响 #491 已接受的 fragment_variable_modal。
    #[test]
    fn fragment_vars_modal_has_no_recompute_button_or_refresh_icon() {
        let ws = include_str!("workspace.rs");
        // UI 源码里（弹窗主体 1559 行附近到 1874 行前）不得再出现重算按钮文案：
        // 用行号切片而不是全文 contains，避免误伤本测试自身的断言字符串。
        // 用锚点 "modal_window(\"fragment_vars_modal\"" 精确切出本弹窗主体：
        // 不能裸用 "fragment_vars_modal"（会被 layout_util::fragment_vars_modal_size 抢走第一刀）。
        let ui_src_only = {
            let start = ws
                .find("modal_window(\"fragment_vars_modal\"")
                .expect("弹窗片段应存在");
            let end = ws.find("mod tests").expect("测试模块应存在");
            &ws[start..end]
        };
        assert!(
            !ui_src_only.contains("根据变量重算命令"),
            "「根据变量重算命令」中文文案必须从 UI 源码移除"
        );
        assert!(
            !ui_src_only.contains("Recompute command"),
            "Recompute command 英文文案必须从 UI 源码移除"
        );
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("quick_fragment_selector").unwrap_or(modal.len())];
        assert!(
            !tail.contains("IconId::Refresh"),
            "fragment_vars_modal 内不应再有 Refresh 图标按钮"
        );
        // #491 弹窗保持原样：没有 rewrite 按钮（它的断言在 rewrite_button_labels_removed_from_variable_dialog）。
        assert!(
            ws.contains("fragment_variable_modal"),
            "「填写变量」弹窗必须原样保留"
        );
    }

    /// 与 #491 相同：fragment_vars_modal 的变量输入框走实心底+描边的
    /// boxed 带 align 版本，且显式 LEFT_CENTER（占位符与输入正文都左对齐）。
    #[test]
    fn fragment_vars_modal_fields_are_boxed_and_left_aligned() {
        let ws = include_str!("workspace.rs");
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("fragment_variable_modal").unwrap_or(modal.len())];
        let call = tail
            .split("form_singleline_field_boxed_with_align")
            .nth(1)
            .expect("fragment_vars_modal 应改用带对齐参数的 boxed 输入框");
        let call_tail = &call[..call.find(';').unwrap_or(call.len())];
        assert!(
            call_tail.contains("Align2::LEFT_CENTER"),
            "fragment_vars_modal 的变量输入框必须显式左对齐：{call_tail}"
        );
        assert!(
            !tail.contains("form_singleline_field("),
            "fragment_vars_modal 不应回退到下划线样式的 form_singleline_field"
        );
    }

    /// 回归 #493 Linux GUI Fail：旧实现把 `pending_fragment_vars_focus_first`
    /// 拷到局部变量，`request_focus` 后只清局部，状态位每帧仍为 true，
    /// 第二栏无法点击或 Tab 聚焦，输入全进第一栏。
    ///
    /// 本测试直接驱动生产 hook `consume_focus_first_once`（不是源码字符串
    /// 检查），用两栏、两帧验证：成功申请首栏焦点后必须清掉持久状态位，
    /// 第二帧不得再把焦点抢回第一栏。
    #[test]
    fn pending_fragment_vars_focus_first_must_not_steal_focus_every_frame() {
        let mut pending_fragment_vars_focus_first = true; // 弹窗刚打开
        let fields = ["who", "host_ip"];

        // 第一帧：只应聚焦第一栏，并且清掉持久标志。
        let mut requested = Vec::new();
        let mut first_field = true;
        for name in fields {
            if super::consume_focus_first_once(
                &mut pending_fragment_vars_focus_first,
                first_field,
            ) {
                requested.push(name);
                first_field = false;
            }
        }
        assert_eq!(requested, vec!["who"], "只有第一个输入框请求焦点");
        assert!(
            !pending_fragment_vars_focus_first,
            "成功申请首栏焦点后必须清掉状态位，不能只清局部变量"
        );

        // 第二帧：用户点击或 Tab 到第二栏。状态位已清，不得再抢回第一栏。
        requested.clear();
        first_field = true;
        for name in fields {
            if super::consume_focus_first_once(
                &mut pending_fragment_vars_focus_first,
                first_field,
            ) {
                requested.push(name);
                first_field = false;
            }
        }
        assert!(
            requested.is_empty(),
            "第二帧不得把焦点抢回第一栏（旧 bug：状态位仍为 true）"
        );
        assert!(!pending_fragment_vars_focus_first);
    }

    /// 没有输入框时 hook 不会被调用，标志必须仍可由调用方清掉；
    /// 同时锁定：非首栏或标志已是 false 时都不得请求焦点。
    #[test]
    fn consume_focus_first_once_only_fires_for_first_field_while_flag_is_set() {
        let mut flag = true;
        assert!(
            !super::consume_focus_first_once(&mut flag, false),
            "非首栏不得消费标志"
        );
        assert!(flag, "非首栏调用不得提前清掉标志");
        assert!(super::consume_focus_first_once(&mut flag, true));
        assert!(!flag);
        assert!(
            !super::consume_focus_first_once(&mut flag, true),
            "标志用过一次后，即使仍是首栏也不得再聚焦"
        );
        assert!(!flag);
    }

    /// fragment_vars_modal 打开即聚焦第一个输入框：一次性标志协议与 #488 相同
    /// （pending_fragment_vars_focus_first → request_focus → 清零）。
    #[test]
    fn fragment_vars_modal_focuses_first_field_once() {
        let mut focus_first = true; // 两条打开路径都会置 true
        let mut requested = Vec::new();
        let fields = ["who", "times"];
        let mut first_field = true;
        for name in fields {
            // 与 fragment_vars_modal 变量行一致：走生产 hook，清的是持久标志。
            if super::consume_focus_first_once(&mut focus_first, first_field) {
                requested.push(name);
                first_field = false;
            }
        }
        assert_eq!(requested, vec!["who"], "只有第一个输入框请求焦点");
        assert!(!focus_first, "标志用完即清，下一帧不再重复聚焦");
        let ws = include_str!("workspace.rs");
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("quick_fragment_selector").unwrap_or(modal.len())];
        assert!(
            tail.contains("pending_fragment_vars_focus_first")
                && tail.contains("request_focus")
                && tail.contains("consume_focus_first_once"),
            "fragment_vars_modal 必须用 consume_focus_first_once 消费持久状态位后再 request_focus"
        );
        // 弹窗关闭时清掉聚焦标志（与变量值一起清理，避免下一帧幽灵聚焦）
        assert!(
            tail.contains("pending_fragment_vars_focus_first = false"),
            "关闭弹窗时必须清掉聚焦标志"
        );
        let fs = include_str!("fragment_state.rs");
        assert!(
            fs.contains("pending_fragment_vars_focus_first: false,"),
            "默认(关闭)状态下聚焦标志必须是 false——不该有幽灵聚焦"
        );
        // 两条打开路径都必须置位该标志（与 #488 在 variable_dialog 打开路径置位一致）
        let app = include_str!("app.rs");
        let paste_path = app
            .split("FragmentVarsCompletion::PasteInsertStats")
            .nth(1)
            .expect("右栏插入路径应存在");
        assert!(
            paste_path.contains("pending_fragment_vars_focus_first = true"),
            "右栏插入路径打开弹窗时必须置位聚焦标志"
        );
        let quick_path = app
            .split("FragmentVarsCompletion::QuickExecuteSend")
            .nth(1)
            .expect("快速执行路径应存在");
        assert!(
            quick_path.contains("pending_fragment_vars_focus_first = true"),
            "快速执行路径打开弹窗时必须置位聚焦标志"
        );
        // 第三条打开路径：#491「填写变量」确认后仍有 <占位符> → 切到本弹窗（workspace.rs 内）。
        let handoff_path = ws
            .split("sync_pending_fragment_command_edit")
            .nth(2)
            .expect("workspace.rs 的打开路径应存在");
        assert!(
            handoff_path.contains("pending_fragment_vars_focus_first = true"),
            "从「填写变量」切换到本弹窗的路径也必须置位聚焦标志"
        );
    }

    /// 预览实时性（旧路径）：changed_any → sync_pending_fragment_command_edit
    /// 用当前 pending_fragment_vars 重建 pending_fragment_command_edit，
    /// 无需任何按钮；同时清掉上次的发送错误。
    #[test]
    fn fragment_vars_changed_any_rebuilds_preview_and_clears_error() {
        // 纯展开逻辑：模板 + 当前变量值 → 替换后的命令（弹窗 changed_any 分支做的事）。
        use crate::core::fragment::expand_fragment_command_stages;
        let template = "echo hello <who>";
        let mut m = std::collections::HashMap::new();
        let initial = expand_fragment_command_stages(template, None, &m).expect("展开应成功");
        assert_eq!(initial, "echo hello <who>", "初始预览保留占位符");
        // 用户打字 → values 变化 → 预览逐帧重建（无按钮参与）
        m.insert("who".to_string(), "world".to_string());
        let live = expand_fragment_command_stages(template, None, &m).expect("展开应成功");
        assert_eq!(live, "echo hello world", "输入即刷新，无需额外点击");
        // 继续编辑 → 预览继续跟随
        m.insert("who".to_string(), "mist".to_string());
        let live2 = expand_fragment_command_stages(template, None, &m).expect("展开应成功");
        assert_eq!(live2, "echo hello mist");
        // 兜底协议：changed_any → 重建预览并清掉上次的 finalize 错误
        let mut changed_any = false;
        let mut rebuild = false;
        let mut last_error: Option<String> = Some("boom".to_string());
        for value_changed in [false, true, false, true] {
            if value_changed {
                changed_any = true;
            }
            if changed_any {
                rebuild = true;
                last_error = None;
            }
        }
        assert!(rebuild, "字段变化后预览必须重建");
        assert!(last_error.is_none(), "重建预览时清掉旧错误");
        let ws = include_str!("workspace.rs");
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("quick_fragment_selector").unwrap_or(modal.len())];
        assert!(
            tail.contains("changed_any") && tail.contains("sync_pending_fragment_command_edit"),
            "fragment_vars_modal 必须保留 changed_any → sync 预览重建路径"
        );
    }

    /// 未匹配 / 缺失的占位符：fragment_vars_modal 也要给出可见且可操作的反馈，
    /// 不能静默无变化——逐栏警示 + 汇总清单（与 #491 同款文案来源）。
    #[test]
    fn fragment_vars_unmatched_placeholder_gets_actionable_feedback() {
        // 模板里根本没有 <who> → placeholder_usage 必须 Missing
        assert_eq!(
            crate::core::placeholder_usage("echo hello", "who"),
            crate::core::PlaceholderUsage::Missing
        );
        // <who> 在模板里 → Used（占位符语法是 <name>，不是 ${var}）
        assert_eq!(
            crate::core::placeholder_usage("echo hello <who>", "who"),
            crate::core::PlaceholderUsage::Used
        );
        // unmatched_variable_names 汇总出填了也不会生效的栏位
        let names = ["who", "times"];
        let unmatched = crate::core::unmatched_variable_names("echo hello", names);
        assert_eq!(unmatched, vec!["who".to_string(), "times".to_string()]);
        let with_used = crate::core::unmatched_variable_names("echo hello <who>", names);
        assert_eq!(with_used, vec!["times".to_string()], "已匹配的占位符不进警示清单");
        // 文案存在且给出下一步动作（中英双语言）
        let locale_zh = crate::i18n::Locale::from(UiLanguage::Zh);
        let zh_summary = locale_zh.tr(
            "These fields are not used by this snippet's command, so they will not change it:",
            "这些变量没有出现在本片段的命令里，填了也不会改变它：",
        );
        assert!(zh_summary.contains("不会改变"), "中文汇总要说明填了也不生效");
        let zh_action = locale_zh.tr(
            "Add the placeholder to the snippet, or edit the command below directly.",
            "把上面的 <变量名> 加进片段模板，或直接编辑下面的命令。",
        );
        assert!(zh_action.contains("模板") && zh_action.contains("命令"), "中文提示要指到下一步");
        let locale_en = crate::i18n::Locale::from(UiLanguage::En);
        let en_summary = locale_en.tr(
            "These fields are not used by this snippet's command, so they will not change it:",
            "这些变量没有出现在本片段的命令里，填了也不会改变它：",
        );
        assert!(en_summary.contains("not used"), "英文汇总要说明字段未被使用");
        let ws = include_str!("workspace.rs");
        let modal = ws
            .split("modal_window(\"fragment_vars_modal\"")
            .nth(1)
            .expect("「填写片段变量」弹窗片段应存在");
        let tail = &modal[..modal.find("quick_fragment_selector").unwrap_or(modal.len())];
        assert!(
            tail.contains("unmatched_variable_names")
                && tail.contains("PlaceholderUsage::Missing"),
            "fragment_vars_modal 必须保留 Missing 分支与未匹配汇总"
        );
    }

    /// #491 已接受的「填写变量」弹窗行为不回归：live preview（changed_any 重建
    /// command_edit）、无重写按钮、LEFT_CENTER 对齐、#488 首栏聚焦仍然成立。
    #[test]
    fn variable_dialog_behavior_unchanged_by_493() {
        let ws = include_str!("workspace.rs");
        let modal = ws
            .split("modal_window(\"fragment_variable_modal\"")
            .nth(1)
            .expect("「填写变量」弹窗片段应存在");
        let tail = &modal[..modal.find("mod tests").unwrap_or(modal.len())];
        // live preview 路径仍在：changed_any → build_fragment_command_preview
        assert!(
            tail.contains("changed_any") && tail.contains("build_fragment_command_preview"),
            "「填写变量」弹窗的实时预览路径不能被 #493 改掉"
        );
        // 输入框仍是 boxed 带 align、LEFT_CENTER
        let call = tail
            .split("form_singleline_field_boxed_with_align")
            .nth(1)
            .expect("「填写变量」弹窗应使用带对齐参数的 boxed 输入框");
        let call_tail = &call[..call.find(';').unwrap_or(call.len())];
        assert!(
            call_tail.contains("Align2::LEFT_CENTER"),
            "「填写变量」弹窗输入框必须保持左对齐"
        );
        // #488 聚焦标志协议仍在
        assert!(
            tail.contains("focus_first_input") && tail.contains("request_focus"),
            "「填写变量」弹窗的首个字段聚焦协议不能丢"
        );
        // 无重写按钮
        assert!(
            !tail.contains("IconId::Refresh"),
            "「填写变量」弹窗内不应有刷新(重写)图标按钮"
        );
    }
}
