//! 主窗口布局 shell：egui 区域注册顺序见 `docs/product/LAYOUT.md`
//!
//! `MistTermApp::update` 负责 tick / 快捷键；本模块编排顶栏、Activity Rail、
//! 右抽屉、中央工作区、Toast 与弹窗层（全平台统一，无常驻底栏）。

use super::*;
use crate::core::session::session_color_tag_rgb;
use crate::core::SESSION_COLOR_TAGS;

impl MistTermApp {
    pub(crate) fn render_workspace_shell(
        &mut self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        theme: &crate::ui::theme::Theme,
    ) {
        // 顶栏：非 macOS 为窗口内菜单；macOS 用系统菜单栏（与 native_menu 安装时机解耦：
        // 用 `cfg` 直接关掉应用内 top_chrome，避免首帧或安装失败时露出一条 chrome_bar 颜色）。
        let pending = self.ssh_pending_import_count();
        let _ = pending; // SSH 待导入改走右下角需确认 Toast，顶栏不再显示 chip
        #[cfg(target_os = "macos")]
        let top_chrome_height = 0.0;
        #[cfg(not(target_os = "macos"))]
        let top_chrome_height = theme.top_chrome_total_height();
        if top_chrome_height > 0.0 {
            egui::TopBottomPanel::top("top_chrome")
                .exact_height(top_chrome_height)
                .frame(theme.frame_top_chrome())
                .show_separator_line(false)
                .show(ctx, |ui| {
                    let bar_bg = ui.max_rect();
                    ui.painter()
                        .rect_filled(bar_bg, 0.0, theme.chrome_bar_fill());
                    let _title_actions = crate::ui::chrome::render_top_chrome_panel(
                        ui,
                        theme,
                        !self.uses_native_menu_bar(),
                        |ui| self.show_application_menu_bar(ui, ctx, theme, frame),
                        0,
                        false,
                    );
                });
        }

        // 左侧 Activity Rail（可完全隐藏；隐藏时仍保留左缘窄恢复条）。
        self.show_activity_rail(ctx, theme);

        // 右侧单抽屉：须先于 Central 注册（Foreground 重绘依赖）。
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

        // SFTP（右侧面板；切换终端标签时重置远端路径并重新拉列表）
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

        // 系统监控：切换终端标签时改为采集当前 SSH 会话（与 SFTP 侧栏一致）
        if self.show_monitor_panel {
            if self.monitor_last_tab != self.active_tab {
                self.monitor_last_tab = self.active_tab;
                self.sync_monitor_panel_to_active_tab();
            }
            self.monitor_panel.show_side_panel(
                ctx,
                theme,
                &mut self.show_monitor_panel,
                &mut self.right_dock_outer_left_x,
                dock_col_w,
            );
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
        egui::CentralPanel::default()
            // 不在 Frame 上铺底色（Central 后绘会盖住右栏）；工作区底色由侧栏/终端列各自 Frame 承担
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
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
                // 列布局垂直铺满到 work 底缘；顶部不留 `bg_body` 缝（tab 条 / 侧栏标题栏直接贴顶）。
                let work_body = egui::Rect::from_min_max(
                    egui::pos2(work_inner.min.x, work_top),
                    egui::pos2(work_inner.max.x, work_bottom),
                );
                // 仅铺中央槽位 bg_body（clip=work，不越过右栏）；右栏正文在 Central 后以 Foreground 绘制
                let work_painter = ui.painter().with_clip_rect(work);
                work_painter.rect_filled(work, 0.0, theme.bg_body_color());
                let seam_y = work.max.y - WORK_BOTTOM_GAP;
                if seam_y > work.min.y {
                    work_painter.hline(work.x_range(), seam_y, theme.divider_stroke());
                }
                ui.allocate_ui_at_rect(work_body, |ui| {
                    ui.set_clip_rect(work);
                    let layout_h = (work_bottom - work_top).max(1.0);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        // 连接栏 / 拖把 / 终端三段紧贴：拖把本身就是 `bg_body` 缝（与右 dock 缝同色等宽），
                        // 不再叠 `region_gap`，否则视觉缝 = 6+1+6=13 与右 dock 5 不一致。
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        ui.set_min_height(layout_h);
                        // 须用已分配子项的右缘，勿用 max_rect.min.x（仍是整行左缘，终端会盖住侧栏）
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

                            let sidebar_rect = egui::Rect::from_min_max(
                                egui::pos2(col_left, work_top),
                                egui::pos2(col_left + self.sidebar_width, work_bottom),
                            );
                            ui.allocate_ui_at_rect(sidebar_rect, |ui| {
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
                                    &self.team_service.current_team_servers(),
                                    Self::id_sidebar_connection_search(),
                                    theme,
                                );
                                let _ = col_actions;

                                if sidebar_output.create_session_clicked {
                                    self.show_new_session_dialog = true;
                                }
                                if sidebar_output.collapse_clicked {
                                    self.sidebar_collapsed = true;
                                    self.sidebar_user_dismissed_responsive = true;
                                }
                                if let Some(session_id) = sidebar_output.selected_session_id {
                                    self.select_session(ctx, &session_id);
                                }
                                if let Some(server_key) = sidebar_output.connect_team_server_key {
                                    self.connect_team_server(ctx, &server_key);
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
                                    self.sidebar_collapsed = true;
                                    self.sidebar_user_dismissed_responsive = true;
                                }
                            });
                            col_left = sidebar_rect.max.x;
                        }

                        if !self.sidebar_collapsed {
                            // 拖把宽度 = `spacing_dock_gap`（5px），同时充当连接栏与终端之间的 `bg_body` 缝隙；
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
                        // 须显式 top_down：父级是 horizontal，allocate_ui_at_rect 会继承横向布局，
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
                                    let terminal_header_row_h = theme.size_tab_bar_row_h();
                                    let tab_row_w = term_col_w;
                                    let tab_row_rect = egui::Rect::from_min_size(
                                        ui.cursor().min,
                                        egui::vec2(tab_row_w, terminal_header_row_h),
                                    );
                                    ui.allocate_ui_at_rect(tab_row_rect, |ui| {
                                        ui.painter().rect_filled(
                                            tab_row_rect,
                                            egui::Rounding::ZERO,
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
                                            let mut to_close = None;
                                            let mut close_others = None;
                                            let mut close_right = None;
                                            let mut disconnect_ssh_idx = None;
                                            let mut reconnect_idx = None;
                                            let mut split_h_idx = None;
                                            let mut split_v_idx = None;
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
                                                let env_color = self
                                                    .session_manager
                                                    .get_session(&tab.primary_session_id())
                                                    .and_then(|s| {
                                                        session_color_tag_rgb(&s.color_tag).map(
                                                            |(r, g, b)| {
                                                                egui::Color32::from_rgb(r, g, b)
                                                            },
                                                        )
                                                    });
                                                let tab_chip = crate::ui::chrome::session_tab_chip(
                                                    ui,
                                                    theme,
                                                    &tab_label,
                                                    active,
                                                    tab.any_connected(),
                                                    false,
                                                    env_color,
                                                );
                                                let tab_resp =
                                                    tab_chip.response.on_hover_text(tab_hover);
                                                if tab_chip.close_clicked {
                                                    to_close = Some(idx);
                                                } else if tab_resp.clicked() {
                                                    self.active_tab = Some(idx);
                                                    self.selected_session_id =
                                                        Some(tab.primary_session_id());
                                                }
                                                tab_resp.context_menu(|ui| {
                                                    crate::ui::chrome::apply_context_menu_style(
                                                        ui, theme,
                                                    );
                                                    if tab.any_connected_or_connecting()
                                                        && crate::ui::chrome::popup_menu_button(
                                                            ui,
                                                            theme,
                                                            crate::i18n::tr(
                                                                ctx,
                                                                "Disconnect SSH (keep output)",
                                                                "断开 SSH（保留输出）",
                                                            ),
                                                        )
                                                        .clicked()
                                                    {
                                                        disconnect_ssh_idx = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                    if crate::ui::chrome::popup_menu_button(
                                                        ui,
                                                        theme,
                                                        crate::i18n::tr(
                                                            ctx,
                                                            "Reconnect this tab",
                                                            "重连此标签",
                                                        ),
                                                    )
                                                    .clicked()
                                                    {
                                                        reconnect_idx = Some(idx);
                                                        ui.close_menu();
                                                    }
                                                    ui.separator();
                                                    if tab.can_split() {
                                                        if crate::ui::chrome::popup_menu_button(
                                                            ui,
                                                            theme,
                                                            crate::i18n::tr(
                                                                ctx,
                                                                "Split left / right",
                                                                "左右分屏",
                                                            ),
                                                        )
                                                        .clicked()
                                                        {
                                                            split_h_idx = Some(idx);
                                                            ui.close_menu();
                                                        }
                                                        if crate::ui::chrome::popup_menu_button(
                                                            ui,
                                                            theme,
                                                            crate::i18n::tr(
                                                                ctx,
                                                                "Split top / bottom",
                                                                "上下分屏",
                                                            ),
                                                        )
                                                        .clicked()
                                                        {
                                                            split_v_idx = Some(idx);
                                                            ui.close_menu();
                                                        }
                                                    }
                                                    if tab.is_split() {
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
                                                            ui.close_menu();
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
                                                            ui.close_menu();
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
                                                        ui.close_menu();
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
                                                        ui.close_menu();
                                                    }
                                                });
                                            }
                                            if crate::ui::chrome::tab_bar_new_tab_button(ui, theme)
                                                .clicked()
                                            {
                                                if self.selected_session_id.is_some() {
                                                    self.open_new_tab_from_selection(ctx);
                                                } else {
                                                    self.show_new_session_dialog = true;
                                                }
                                            }
                                            if let Some(idx) = to_close {
                                                self.request_close_tab_at(idx);
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
                                        ui.spacing_mut().button_padding = prev_padding;
                                        ui.spacing_mut().item_spacing = prev_item_spacing;
                                    });
                                    ui.painter().hline(
                                        tab_row_rect.x_range(),
                                        tab_row_rect.max.y - 0.5,
                                        egui::Stroke::new(1.0, theme.color_panel_header_divider()),
                                    );

                                    let search_h = if self.show_terminal_search {
                                        theme.size_terminal_search_bar_h()
                                    } else {
                                        0.0
                                    };
                                    let term_body_h = (ui.available_height() - search_h).max(1.0);
                                    let terminal_search_open = self.show_terminal_search;
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(term_col_w, term_body_h),
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.set_min_height(term_body_h);
                                            ui.set_max_width(term_col_w);
                                            if let Some(idx) = self.active_tab {
                                                self.maybe_collapse_narrow_split(idx, term_col_w);
                                                let kb_capture =
                                                    self.should_capture_pty_keyboard(ctx);
                                                let active_pane =
                                                    self.tabs.get(idx).map(|t| t.active_pane);
                                                let pane_capture = |pane_idx: usize| {
                                                    active_pane == Some(pane_idx) && kb_capture
                                                };
                                                let mut close_pane_req = None;
                                                if let Some(tab) = self.tabs.get_mut(idx) {
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
                                    if self.show_terminal_search
                                        && self.show_terminal_search_bar(ui, theme)
                                    {
                                        self.show_terminal_search = false;
                                        self.terminal_search_pending_focus = false;
                                    }
                                    if self.command_history_overlay.open {
                                        if let Some(idx) = self.active_tab {
                                            if let Some(tab) = self.tabs.get(idx) {
                                                let rect = tab
                                                    .active_pane()
                                                    .map(|p| p.last_term_rect)
                                                    .unwrap_or(egui::Rect::NOTHING);
                                                match self.command_history_overlay.show(
                                                    ctx,
                                                    theme,
                                                    &self.command_history,
                                                    rect,
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

        if self.show_ai_settings_dialog {
            self.ai_panel.show_settings_dialog(
                ctx,
                theme,
                &mut self.show_ai_settings_dialog,
                &mut self.app_settings,
            );
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

        self.render_new_session_modal(ctx, theme);

        if self.show_about_dialog {
            let mut open = self.show_about_dialog;
            let mut should_close = false;
            let about_title = crate::i18n::tr(ctx, "About", "关于");
            let subtitle = crate::i18n::tr(
                ctx,
                "A modern SSH terminal client.",
                "一个现代化 SSH 终端工具",
            );
            let version_line = format!(
                "{} v{}",
                crate::i18n::tr(ctx, "Version:", "版本："),
                env!("CARGO_PKG_VERSION")
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
                        egui::Frame::none()
                            .fill(theme.color_subtle_inset_fill())
                            .stroke(egui::Stroke::new(1.0, theme.color_overlay_fill_subtle()))
                            .rounding(theme.radius_list_item())
                            .inner_margin(egui::Margin::symmetric(
                                theme.spacing_search_input_x(),
                                theme.spacing_search_input_y(),
                            ))
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
                        ui.add_space(theme.spacing_md());
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
                    });
                });
            self.show_about_dialog = open && !should_close;
        }

        if self.show_preferences_dialog {
            self.show_preferences_modal(ctx, theme);
        }

        self.render_workspace_confirm_modals(ctx, theme);

        if let Some(indices) = self.ssh_import_dialog.show(ctx, theme) {
            self.import_ssh_indices(ctx, &indices);
        }
        self.session_log_dialog
            .show(ctx, theme, &self.session_log_settings);
        let help_shortcuts = crate::ui::app::mistterm_functional_spec_shortcuts(ctx);
        if let Some(error) = self.help_docs_dialog.show(ctx, theme, &help_shortcuts) {
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
                    let form_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);

                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            ui.push_id("edit_session_form", |ui| {
                            Self::modal_header_title_only(ui, theme, crate::i18n::tr(ctx, "Edit session", "编辑会话"));

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

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "SSH private key path", "SSH 私钥路径"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_private_key_path",
                                &mut self.edit_session_private_key_path,
                                crate::i18n::tr(
                                    ctx,
                                    "~/.ssh/id_rsa (empty = password or default keys)",
                                    "~/.ssh/id_rsa（留空则用密码或系统默认密钥）",
                                ),
                                form_w,
                                false,
                            );
                            crate::ui::chrome::form_checkbox(
                                ui,
                                theme,
                                &mut self.edit_session_use_ssh_agent,
                                crate::i18n::tr(
                                    ctx,
                                    "Use SSH agent (ssh-agent / Pageant)",
                                    "使用 SSH Agent（ssh-agent / Pageant）",
                                ),
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "ProxyJump", "跳板 ProxyJump"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_proxy_jump",
                                &mut self.edit_session_proxy_jump,
                                crate::i18n::tr(
                                    ctx,
                                    "bastion or user@bastion:22 (comma-separated hops)",
                                    "bastion 或 user@bastion:22（多跳逗号分隔；匹配已保存会话名）",
                                ),
                                form_w,
                                false,
                            );

                            Self::ui_field_label(ui, theme, crate::i18n::tr(ctx, "ProxyCommand", "代理命令 ProxyCommand"));
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "edit_session_proxy_command",
                                &mut self.edit_session_proxy_command,
                                crate::i18n::tr(
                                    ctx,
                                    "e.g. ssh -W %h:%p jump",
                                    "例：ssh -W %h:%p jump",
                                ),
                                form_w,
                                false,
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Local forwards (-L)",
                                    "本地端口转发 (-L)",
                                ),
                            );
                            ui.add(
                                egui::TextEdit::multiline(&mut self.edit_session_local_forwards_text)
                                    .desired_width(form_w)
                                    .desired_rows(2)
                                    .hint_text(crate::i18n::tr(
                                        ctx,
                                        "8080:127.0.0.1:80 (one per line)",
                                        "8080:127.0.0.1:80（每行一条）",
                                    )),
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Remote forwards (-R)",
                                    "远程端口转发 (-R)",
                                ),
                            );
                            ui.add(
                                egui::TextEdit::multiline(&mut self.edit_session_remote_forwards_text)
                                    .desired_width(form_w)
                                    .desired_rows(2)
                                    .hint_text(crate::i18n::tr(
                                        ctx,
                                        "8080:127.0.0.1:3000 (one per line)",
                                        "8080:127.0.0.1:3000（每行一条）",
                                    )),
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Dynamic forwards (-D / SOCKS5)",
                                    "动态转发 (-D / SOCKS5)",
                                ),
                            );
                            ui.add(
                                egui::TextEdit::multiline(&mut self.edit_session_dynamic_forwards_text)
                                    .desired_width(form_w)
                                    .desired_rows(2)
                                    .hint_text(crate::i18n::tr(
                                        ctx,
                                        "1080 or 0.0.0.0:1080 (one per line)",
                                        "1080 或 0.0.0.0:1080（每行一条）",
                                    )),
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
                            egui::ComboBox::from_id_source("edit_session_color")
                                .selected_text(crate::i18n::session_color_tag(
                                    ctx,
                                    SESSION_COLOR_TAGS
                                        .iter()
                                        .find(|(v, _)| *v == self.edit_session_color_tag.as_str())
                                        .map(|(v, _)| *v)
                                        .unwrap_or_else(|| self.edit_session_color_tag.as_str()),
                                ))
                                .show_ui(ui, |ui| {
                                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
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
                                });

                            ui.label(
                                egui::RichText::new(crate::i18n::tr(ctx, "Connection keep-alive", "连接保活"))
                                    .size(theme.font_size_panel_title())
                                    .strong()
                                    .color(theme.color_form_label()),
                            );
                            crate::ui::chrome::form_checkbox_with_id(
                                ui,
                                theme,
                                "edit_session_keepalive_enabled",
                                &mut self.edit_session_keepalive_enabled,
                                crate::i18n::tr(ctx, "Enable keepalive pings", "启用心跳保持"),
                            );
                            if self.edit_session_keepalive_enabled {
                                ui.horizontal(|ui| {
                                    crate::ui::chrome::form_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Interval (s)", "间隔(秒)"),
                                    );
                                    crate::ui::chrome::form_drag_value_field(
                                        ui,
                                        theme,
                                        egui::Id::new("edit_sess_ka_interval"),
                                        |ui| {
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut self.edit_session_keepalive_interval_secs,
                                                )
                                                .clamp_range(5..=300),
                                            )
                                        },
                                    );
                                    crate::ui::chrome::form_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Max timeouts", "超时次数"),
                                    );
                                    crate::ui::chrome::form_drag_value_field(
                                        ui,
                                        theme,
                                        egui::Id::new("edit_sess_ka_count"),
                                        |ui| {
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut self.edit_session_keepalive_count_max,
                                                )
                                                .clamp_range(1..=20),
                                            )
                                        },
                                    );
                                });
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

                            if required_missing {
                                ui.add_space(theme.spacing_sm());
                                ui.label(
                                    egui::RichText::new(crate::i18n::tr(
                                        ctx,
                                        "Enter session name and host first.",
                                        "请先填写会话名称和主机地址",
                                    ))
                                        .size(theme.font_size_panel_title())
                                        .color(theme.red_a128()),
                                );
                            }

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
                            egui::Frame::none()
                                .fill(theme.color_subtle_inset_fill())
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    theme.color_overlay_fill_subtle(),
                                ))
                                .rounding(theme.radius_list_item())
                                .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x(), theme.spacing_search_input_y()))
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
                                let px = theme.font_size_fragment_dialog_body();
                                let (r, _) =
                                    ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
                                crate::ui::icons::paint_icon(
                                    ui,
                                    r,
                                    crate::ui::icons::IconId::Refresh,
                                    theme.color_body_text_muted(),
                                    px,
                                );
                                if ui
                                    .add(
                                        crate::ui::chrome::panel_toolbar_button_widget(
                                            theme,
                                            egui::RichText::new(crate::i18n::tr(ctx, "Recompute command", "根据变量重算命令"))
                                                .size(theme.font_size_fragment_dialog_body())
                                                .color(theme.color_body_text_muted()),
                                        )
                                        .min_size(egui::vec2(0.0, theme.size_fragment_var_field_min_h())),
                                    )
                                    .clicked()
                                {
                                    self.sync_pending_fragment_command_edit();
                                }
                            });
                            crate::ui::chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Command to run (editable)", "将要执行（可编辑）"),
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
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                                            theme,
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
                                        theme,
                                        crate::ui::icons::IconId::Cross,
                                        crate::i18n::tr(ctx, "Cancel", "取消"),
                                    )
                        .clicked() {
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

        // 快速片段选择器
        if self.quick_selector.open {
            let qsz = layout_util::centered_window_default_size(ctx, 0.40, 0.48);
            let qsz_v = egui::vec2(qsz[0], qsz[1]);
            let q_scroll_max = layout_util::dialog_scroll_max_height(ctx, 220.0);
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

                                for (idx, fragment) in fragments.iter().enumerate() {
                                    // 搜索过滤
                                    if !search_lower.is_empty()
                                        && !fragment.title.to_lowercase().contains(&search_lower)
                                        && !fragment.command.to_lowercase().contains(&search_lower)
                                    {
                                        continue;
                                    }

                                    let is_selected = idx == self.quick_selector.selected_index;

                                    if ui.selectable_label(is_selected, &fragment.title).clicked() {
                                        // 点击执行
                                        self.execute_fragment(ctx, fragment);
                                        self.quick_selector.open = false;
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
                                self.quick_selector.open = false;
                            }
                        });
                    });
                });
        }

        // 变量输入对话框（片段库定义的变量；与命令里的 `<pod>` 等占位符可串联）
        if self.variable_dialog.open {
            let ok_label_static = if self.variable_dialog.paste_after_fill {
                crate::i18n::tr(ctx, "Insert into terminal", "插入终端")
            } else {
                crate::i18n::tr(ctx, "Execute", "执行")
            };

            let var_sz = layout_util::centered_window_default_size(ctx, 0.36, 0.38);
            let var_sz_v = egui::vec2(var_sz[0], var_sz[1]);
            let scroll_h = layout_util::dialog_scroll_max_height(ctx, 240.0);
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
                                                "将要执行的命令（可编辑）",
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
                                                        fragment.title.clone();
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
                                self.variable_dialog.open = false;
                                self.variable_dialog.paste_after_fill = false;
                                self.variable_dialog.last_finalize_error = None;
                            }
                        });
                    });
                });
            ctx.move_to_top(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("mistterm_fragment_variable_dialog"),
            ));
        }
    }
}
