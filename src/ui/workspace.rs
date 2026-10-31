//! 主窗口布局 shell：egui 区域注册顺序见 `docs/product/LAYOUT.md`
//!
//! `MistTermApp::update` 负责 tick / 快捷键；本模块编排顶栏、Activity Rail、
//! 右抽屉、中央工作区、Toast 与弹窗层(全平台统一，无常驻底栏)。

use super::*;
use crate::core::SESSION_COLOR_TAGS;

impl MistTermApp {
    pub(crate) fn render_workspace_shell(
        &mut self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        theme: &crate::ui::theme::Theme,
    ) {
        // 顶栏：
        // - 普通模式：非 macOS 显示窗口内菜单；macOS 用系统菜单栏(应用内 top_chrome 高度 0)。
        // - 沉浸式：Win/Linux 关掉系统边框，自绘标题栏并把会话标签放进去；macOS 仍保留系统标题栏。
        let pending = self.ssh_pending_import_count();
        let _ = pending; // SSH 待导入改走右下角需确认 Toast，顶栏不再显示 chip
        let immersive = self.activity_rail_collapsed;
        self.sync_window_decorations_for_immersive(ctx, immersive);
        let custom_caption = immersive && !cfg!(target_os = "macos");
        // macOS 用 fullsize content view：标签始终画在标题栏那一行(红绿灯右侧)。
        let tabs_in_top = immersive || cfg!(target_os = "macos");
        let top_chrome_height = if tabs_in_top {
            if custom_caption {
                theme.title_bar_height().max(theme.size_tab_bar_row_h())
            } else {
                theme.size_tab_bar_row_h()
            }
        } else if cfg!(target_os = "macos") {
            0.0
        } else {
            theme.top_chrome_total_height()
        };
        if top_chrome_height > 0.0 {
            ctx.root_ui(|root| {
            egui::Panel::top("top_chrome")
                .exact_size(top_chrome_height)
                .frame(theme.frame_top_chrome())
                .show_separator_line(false)
                .show(root, |ui| {
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
            });
        }

        // 左侧 Activity Rail(可完全隐藏；隐藏时仍保留左缘窄恢复条)。
        self.show_activity_rail(ctx, theme);

        // 右侧单抽屉：须先于 Central 注册(Foreground 重绘依赖)。
        self.right_dock_outer_left_x = None;
        let dock_col_w = layout_util::clamp_sidebar_width(self.sidebar_width);

        if self.show_fragment_panel {
            self.show_fragment_panel(ctx, theme, dock_col_w);
        }

        if self.credential_panel.open {
            self.credential_panel.show_side_panel(
                ctx,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.cloud_sync_panel.open {
            self.cloud_sync_panel.show_side_panel(
                ctx,
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.postmortem_dialog.open {
            self.postmortem_dialog.show_side_panel(
                ctx,
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
                theme,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        if self.show_ai_panel {
            self.ai_panel.show_side_panel(
                ctx,
                theme,
                &mut self.show_ai_panel,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
        }

        // 主内容区：侧边栏 + 终端
        ctx.root_ui(|root| {
        egui::CentralPanel::default()
            // 不在 Frame 上铺底色(Central 后绘会盖住右栏)；工作区底色由侧栏/终端列各自 Frame 承担
            .frame(egui::Frame::NONE)
            .show(root, |ui| {
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
                                );
                                let _ = col_actions;

                                if sidebar_output.create_session_clicked {
                                    self.show_new_session_dialog = true;
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
                                        self.open_new_tab_for_session(ctx, &session);
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

                                    let search_h = if self.show_terminal_search {
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
                                        egui::Id::new("ssh_disconnect_banner"),
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
                                    let terminal_search_open = self.show_terminal_search;
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
                                    if self.show_terminal_search
                                        && self.show_terminal_search_bar(ui, theme)
                                    {
                                        self.show_terminal_search = false;
                                        self.terminal_search_pending_focus = false;
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
        });

        self.render_workspace_right_dock_foreground(ctx, theme, top_chrome_height);
        self.sync_ssh_import_action_toast(ctx);
        self.tick_status_toast();
        self.show_status_toast(ctx, theme);

        if self.show_ai_settings_dialog {
            self.ai_panel.show_settings_dialog(
                ctx,
                theme,
                &mut self.show_ai_settings_dialog,
                &mut self.app_settings,
            );
        }

        if self.show_audit_timeline_dialog {
            let mut action = crate::ui::audit_timeline_dialog::AuditTimelineUiAction::None;
            crate::ui::audit_timeline_dialog::show_audit_timeline_modal(
                ctx,
                theme,
                &mut self.show_audit_timeline_dialog,
                &self.audit_timeline,
                &mut self.audit_timeline_host_filter,
                &mut self.audit_timeline_outcome_filter,
                &mut action,
            );
            if matches!(action, crate::ui::audit_timeline_dialog::AuditTimelineUiAction::Clear) {
                self.audit_timeline.clear();
            }
        }

        if self.show_agent_install_dialog {
            crate::ui::agent_install_dialog::show_agent_install_modal(
                ctx,
                theme,
                &mut self.show_agent_install_dialog,
                &mut self.agent_install_copied,
            );
        }

        if self.show_ask_knowledge_dialog {
            let mut action = crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::None;
            crate::ui::ask_knowledge_dialog::show_ask_knowledge_modal(
                ctx,
                theme,
                &mut self.show_ask_knowledge_dialog,
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
                            self.show_ask_knowledge_dialog = false;
                        }
                    }
                }
                crate::ui::ask_knowledge_dialog::AskKnowledgeUiAction::AskModel => {
                    let q = self.ask_knowledge_query.clone();
                    self.show_ask_knowledge_dialog = false;
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
        let lib_saved = self.fragment_library.show_window(
            ctx,
            &mut self.fragment_manager,
            &mut self.fragment_sort_by,
            &fragment_cfg,
            &mut self.fragment_shortcut_store,
            session_for_fragments.as_ref(),
            theme,
        );
        if lib_saved {
            self.fragment_manager.sort(self.fragment_sort_by);
        }

        self.render_new_session_modal(ctx, theme);

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
        if self.paste_history_overlay.open {
            let term_rect = self
                .active_tab
                .and_then(|i| self.tabs.get(i))
                .and_then(|t| t.active_pane())
                .map(|pane| pane.last_term_rect)
                .unwrap_or(egui::Rect::NOTHING);
            let remembering = self.app_settings.remember_paste_history;
            match self.paste_history_overlay.show(
                ctx,
                theme,
                &self.paste_history,
                term_rect,
                remembering,
            ) {
                PasteHistoryAction::Close => {
                    self.paste_history_overlay.open = false;
                }
                PasteHistoryAction::Apply(text) => {
                    self.paste_history_text_to_active(ctx, &text);
                    self.paste_history_overlay.open = false;
                }
                PasteHistoryAction::Delete(id) => {
                    self.paste_history.remove_id(id);
                    if self.paste_history_overlay.selected > 0 {
                        self.paste_history_overlay.selected -= 1;
                    }
                }
                PasteHistoryAction::None => {}
            }
        }

        // #189：启动检测到未提示过的崩溃 → 弹一次提示窗（版本/时间/panic 首行）。
        if self.crash_notice.is_some() {
            self.render_crash_notice_dialog(ctx, theme);
        }

        if self.show_about_dialog {
            let mut open = self.show_about_dialog;
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
                                self.show_licenses_dialog = true;
                            }
                        });
                    });
                });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                should_close = true;
            }
            self.show_about_dialog = open && !should_close;
        }
        self.render_licenses_modal(ctx, theme);
        self.render_update_dialog(ctx, theme);

        if self.show_preferences_dialog {
            self.show_preferences_modal(ctx, theme);
        }

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

        if self.show_edit_session_dialog {
            let mut open = self.show_edit_session_dialog;
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
                        self.edit_session_name.trim().is_empty() || self.edit_session_host.trim().is_empty();

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
                            let form_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Session name", "会话名称"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_name",
                                &mut self.edit_session_name,
                                crate::i18n::tr(ctx, "e.g. prod-server-01", "例：生产服务器-01"),
                                form_w,
                                false,
                            );

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let host_w = (row_w - 98.0).max(160.0);
                                ui.vertical(|ui| {
                                    ui.set_width(host_w);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Host", "主机地址"));
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_host",
                                        &mut self.edit_session_host,
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
                                        &mut self.edit_session_port_str,
                                        &mut self.edit_session_port,
                                        88.0,
                                    );
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let half = ((row_w - 10.0) / 2.0).max(140.0);
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Username", "用户名"));
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_username",
                                        &mut self.edit_session_username,
                                        crate::i18n::tr(ctx, "e.g. root", "如 root"),
                                        half,
                                        false,
                                    );
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Password", "密码"));
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "edit_session_password",
                                        &mut self.edit_session_password,
                                        crate::i18n::tr(
                                            ctx,
                                            "**** keeps the saved password; enter a new value to reset",
                                            "**** 表示沿用原密码；改为新口令以保存新密码",
                                        ),
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
                                &mut self.edit_session_auth_mode,
                                &mut self.edit_session_use_ssh_agent,
                                &mut self.edit_session_private_key_path,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "Jump host", "跳板机"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_proxy_jump",
                                &mut self.edit_session_proxy_jump,
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
                                &mut self.edit_session_proxy_command,
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
                                egui::Id::new("edit_session_local_forwards"),
                                &mut self.edit_session_local_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "8080:127.0.0.1:80 (one per line)",
                                    "8080:127.0.0.1:80(每行一条)",
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
                                egui::Id::new("edit_session_remote_forwards"),
                                &mut self.edit_session_remote_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "8080:127.0.0.1:3000 (one per line)",
                                    "8080:127.0.0.1:3000(每行一条)",
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
                                egui::Id::new("edit_session_dynamic_forwards"),
                                &mut self.edit_session_dynamic_forwards_text,
                                crate::i18n::tr(
                                    ctx,
                                    "1080 or 0.0.0.0:1080 (one per line)",
                                    "1080 或 0.0.0.0:1080(每行一条)",
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
                                egui::Id::new("edit_session_post_connect_command"),
                                &mut self.edit_session_post_connect_command,
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
                                &mut self.edit_session_group,
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
                                        .find(|(v, _)| *v == self.edit_session_color_tag.as_str())
                                        .map(|(v, _)| *v)
                                        .unwrap_or_else(|| self.edit_session_color_tag.as_str()),
                                ),
                                form_w,
                                |ui| {
                                    for (value, _) in SESSION_COLOR_TAGS {
                                        let label = crate::i18n::session_color_tag(ctx, value);
                                        if ui
                                            .selectable_value(
                                                &mut self.edit_session_color_tag,
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
                                &mut self.edit_session_keepalive_enabled,
                                crate::i18n::tr(ctx, "Enable keepalive pings", "空闲时发送心跳，防止被断开"),
                            );
                            if self.edit_session_keepalive_enabled {
                                // 与偏好设置一致：两个步进器各占一行，窄表单不把标签折断（#130）。
                                let every = crate::i18n::tr(ctx, "Interval (s)", "每隔(秒)");
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
                                            egui::Id::new("edit_sess_ka_interval"),
                                            &mut self.edit_session_keepalive_interval_secs,
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
                                            egui::Id::new("edit_sess_ka_count"),
                                            &mut self.edit_session_keepalive_count_max,
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
                                &mut self.edit_session_keepalive_auto_reconnect,
                                crate::i18n::tr(ctx, "Reconnect automatically after disconnect", "断开后自动重连"),
                            );

                            ui.add_space(theme.spacing_sm());
                            self.edit_session_vault.show(
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
                                    should_close = !self.show_edit_session_dialog;
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
                        should_close = !self.show_edit_session_dialog;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        should_close = true;
                    }
                });
            self.show_edit_session_dialog = open && !should_close;
        }

        if self.show_fragments_dialog {
            let mut open = self.show_fragments_dialog;
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
            self.show_fragments_dialog = open && !should_close;
        }

        if self.show_fragment_vars_dialog {
            let mut open = self.show_fragment_vars_dialog;
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
                                    self.pending_fragment_name
                                ))
                                    .size(theme.font_size_fragment_dialog_caption())
                                    .color(theme.color_caption_text()),
                            );
                            ui.add_space(theme.spacing_panel_gap());
                            let var_field_w = layout_util::finite_content_width(ui);
                            for (key, value) in &mut self.pending_fragment_vars {
                                crate::ui::chrome::form_field_label(
                                    ui,
                                    theme,
                                    &format!("<{}>", key),
                                );
                                crate::ui::chrome::form_singleline_field(
                                    ui,
                                    theme,
                                    egui::Id::new(("pending_frag_var", key.as_str())),
                                    value,
                                    "",
                                    var_field_w,
                                    false,
                                );
                                ui.add_space(theme.spacing_panel_gap());
                            }
                            ui.separator();
                            ui.horizontal(|ui| {
                                if crate::ui::chrome::panel_action_button_with_icon_ex(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Refresh,
                                    crate::i18n::tr(ctx, "Recompute command", "根据变量重算命令"),
                                    true,
                                )
                                .clicked()
                                {
                                    self.sync_pending_fragment_command_edit();
                                }
                            });
                            crate::ui::chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Command to run (editable)", "将要执行(可编辑)"),
                            );
                            crate::ui::chrome::form_multiline_field(
                                ui,
                                theme,
                                egui::Id::new("pending_frag_cmd_edit"),
                                &mut self.pending_fragment_command_edit,
                                var_field_w,
                                4,
                                false,
                            );
                            ui.add_space(theme.spacing_sm());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                                    let insert_label = match self.fragment_vars_completion {
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
                                                match self.fragment_vars_completion {
                                                    FragmentVarsCompletion::PasteInsertStats => {
                                                        if let Some(id) =
                                                            self.pending_fragment_id.clone()
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
                                                                        self.pending_fragment_id
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
                                                                    self.pending_fragment_id.clone()
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
                                                        self.quick_selector.open = false;
                                                    }
                                                }
                                                should_close = true;
                                            }
                                            Err(e) => {
                                                self.notify_error(
                                                    crate::i18n::localize_fragment_expr_error(
                                                        crate::i18n::language(ctx),
                                                        &e,
                                                    ),
                                                );
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
                self.pending_fragment_id = None;
                self.pending_fragment_name.clear();
                self.pending_fragment_command.clear();
                self.pending_fragment_vars.clear();
            }
            self.show_fragment_vars_dialog = open && !should_close;
        }

        // 快速片段选择器
        if self.quick_selector.open {
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
                            egui::Id::new("quick_fragment_search"),
                            &mut self.quick_selector.search_query,
                            crate::i18n::tr(ctx, "Search snippets…", "搜索片段…"),
                            q_search_w,
                        );

                        ui.add_space(theme.spacing_md());

                        // 片段列表
                        egui::ScrollArea::vertical()
                            .max_height(q_scroll_max)
                            .show(ui, |ui| {
                                let fragments: Vec<_> = self.fragment_manager.list().to_vec();
                                let search_lower = self.quick_selector.search_query.to_lowercase();
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
                                            idx == self.quick_selector.selected_index;
                                        let title = crate::i18n::fragment_display_title(
                                            ui.ctx(),
                                            fragment,
                                        );

                                        if ui.selectable_label(is_selected, title).clicked() {
                                            self.execute_fragment(ctx, fragment);
                                            self.quick_selector.open = false;
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
                self.quick_selector.open = false;
            }
        }

        // 变量输入对话框(片段库定义的变量；与命令里的 `<pod>` 等占位符可串联)
        if self.variable_dialog.open {
            let ok_label_static = if self.variable_dialog.paste_after_fill {
                crate::i18n::tr(ctx, "Insert into terminal", "插入终端")
            } else {
                crate::i18n::tr(ctx, "Execute", "执行")
            };

            let var_sz = layout_util::centered_window_default_size(ctx, 0.36, 0.38);
            let var_sz_v = egui::vec2(var_sz[0], var_sz[1]);
            let scroll_h = layout_util::dialog_scroll_max_height(ctx, 240.0);
            let mut should_close = false;
            crate::ui::chrome::modal_window("fragment_variable_modal", theme, ctx)
                .id(egui::Id::new("mistterm_fragment_variable_dialog"))
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
                            &self.variable_dialog.fragment_title,
                        ));
                        ui.add_space(theme.spacing_sm());
                        egui::ScrollArea::vertical()
                            .max_height(scroll_h)
                            .show(ui, |ui| {
                                if let Some(fragment_id) = &self.variable_dialog.fragment_id {
                                    if let Some(fragment) = self.fragment_manager.get(fragment_id) {
                                        for var in &fragment.variables {
                                            ui.label(
                                                egui::RichText::new(&var.description)
                                                    .size(theme.font_size_fragment_dialog_body())
                                                    .color(theme.text_primary()),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{} <{}>",
                                                    crate::i18n::tr(
                                                        ui.ctx(),
                                                        "Placeholder",
                                                        "占位符"
                                                    ),
                                                    var.name
                                                ))
                                                .size(theme.font_size_fragment_dialog_caption())
                                                .color(theme.text_tertiary()),
                                            );
                                            let value = self
                                                .variable_dialog
                                                .values
                                                .entry(var.name.clone())
                                                .or_default();
                                            let var_w = layout_util::finite_content_width(ui);
                                            crate::ui::chrome::form_singleline_field(
                                                ui,
                                                theme,
                                                egui::Id::new(("var_dialog", var.name.as_str())),
                                                value,
                                                var.default_value.as_deref().unwrap_or(""),
                                                var_w,
                                                false,
                                            );
                                            ui.add_space(theme.spacing_md());
                                        }
                                        ui.separator();
                                        if crate::ui::chrome::panel_action_icon_button(
                                            ui,
                                            theme,
                                            crate::ui::icons::IconId::Refresh,
                                            crate::i18n::tr(
                                                ui.ctx(),
                                                "Rewrite command using fields above",
                                                "用上方变量重写命令",
                                            ),
                                        )
                                        .clicked()
                                        {
                                            self.variable_dialog.last_finalize_error = None;
                                            self.variable_dialog.command_edit = self
                                                .build_fragment_command_preview(
                                                    fragment,
                                                    &self.variable_dialog.values,
                                                );
                                        }
                                        crate::ui::chrome::form_field_label(
                                            ui,
                                            theme,
                                            crate::i18n::tr(
                                                ui.ctx(),
                                                "Command to run (editable)",
                                                "将要执行的命令(可编辑)",
                                            ),
                                        );
                                        crate::ui::chrome::form_multiline_field(
                                            ui,
                                            theme,
                                            egui::Id::new("var_dialog_cmd_edit"),
                                            &mut self.variable_dialog.command_edit,
                                            layout_util::finite_content_width(ui),
                                            5,
                                            false,
                                        );
                                    }
                                }
                            });
                        if let Some(ref err) = self.variable_dialog.last_finalize_error {
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
                                let paste = self.variable_dialog.paste_after_fill;
                                if let Some(fid) = self.variable_dialog.fragment_id.clone() {
                                    if let Some(fragment) = self.fragment_manager.get(&fid).cloned()
                                    {
                                        match self.finalize_fragment_command_text(
                                            &self.variable_dialog.command_edit,
                                            &self.variable_dialog.values,
                                        ) {
                                            Ok(cmd) => {
                                                self.variable_dialog.last_finalize_error = None;
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
                                                        self.quick_selector.open = false;
                                                    }
                                                } else {
                                                    self.pending_fragment_id = Some(fid.clone());
                                                    self.pending_fragment_name =
                                                        crate::i18n::fragment_display_title(
                                                            ctx, &fragment,
                                                        );
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
                                                let localized =
                                                    crate::i18n::localize_fragment_expr_error(
                                                        crate::i18n::language(ctx),
                                                        &e,
                                                    );
                                                self.notify_error(localized.clone());
                                                self.variable_dialog.last_finalize_error =
                                                    Some(localized);
                                            }
                                        }
                                    } else {
                                        self.notify_auto(
                                            crate::i18n::tr(
                                                ctx,
                                                "Snippet no longer exists (removed from library?)",
                                                "找不到该片段(可能已从库中删除)",
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
                                self.variable_dialog.open = false;
                                self.variable_dialog.paste_after_fill = false;
                                self.variable_dialog.last_finalize_error = None;
                            }
                        });
                    });
                });
            if should_close {
                self.variable_dialog.open = false;
                self.variable_dialog.paste_after_fill = false;
                self.variable_dialog.last_finalize_error = None;
            }
            ctx.move_to_top(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("mistterm_fragment_variable_dialog"),
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
        let close_id = egui::Id::new("agent_audit_banner_close");
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
            let mut open_another_sid: Option<String> = None;
            let mut open_another_local = false;
            let mut split_h_idx = None;
            let mut split_v_idx = None;
            let mut broadcast_idx = None;
            let mut unsplit_idx = None;
            let mut close_pane_tab = None;
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
                        if sid.starts_with("local:")
                            || tab.panes.iter().any(|p| p.terminal.is_local_shell())
                        {
                            Some(theme.session_tab_accent_from_id(&sid))
                        } else {
                            None
                        }
                    });
                let tab_chip = crate::ui::chrome::session_tab_chip(
                    ui,
                    theme,
                    &tab_label,
                    active,
                    tab.any_connected(),
                    tab.needs_attention,
                    false,
                    env_color,
                );
                let tab_hover = if tab.needs_attention && !active {
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
                    let is_local = sid.starts_with("local:")
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
                        if is_local {
                            open_another_local = true;
                        } else {
                            open_another_sid = Some(sid.clone());
                        }
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
            self.show_tab_bar_new_tab_picker(ui, theme, ctx);
            if let Some(idx) = to_activate {
                self.switch_tab_to_index(idx);
            }
            if let Some(idx) = to_close {
                self.request_close_tab_at(idx);
            }
            if open_another_local {
                self.open_local_shell_tab(ctx);
            } else if let Some(sid) = open_another_sid {
                if let Some(session) = self.session_manager.get_session(&sid).cloned() {
                    self.open_new_tab_for_session(ctx, &session);
                }
            }
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
                                let host_line = if session.port != 22 {
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
                self.open_new_tab_for_session(ctx, &session);
            }
        } else if open_local {
            self.open_local_shell_tab(ctx);
        } else if open_new_connection {
            self.show_new_session_dialog = true;
        }
    }

}
