//! 监控面板 UI
//!
//! 实时显示服务器资源使用状态

use eframe::egui;
use egui_plot::{AxisBools, Corner, Legend, Line, LineStyle, Plot, PlotPoints, VLine};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use crate::i18n::{self, Locale, UiLanguage};
use crate::monitor::{Monitor, ServerStats, format_bytes};
use crate::ui::layout_util;
use crate::ui::theme::Theme;

/// 监控面板组件
pub struct MonitorPanel {
    /// 监控器(None 表示未初始化)
    monitor: Option<Monitor>,
    /// 是否自动刷新（默认开启，与产品稿「自动刷新 5s」一致）
    auto_refresh: bool,
    /// 刷新间隔(秒)
    refresh_interval_secs: f32,
    /// CPU 告警阈值（%）
    alert_cpu_pct: f32,
    /// 内存告警阈值（%）
    alert_mem_pct: f32,
    /// 磁盘告警阈值（%）
    alert_disk_pct: f32,
    /// 上次 UI 刷新时间(秒,`egui` input time)
    last_ui_refresh: f64,
    /// 最后一次错误
    last_error: Option<String>,
    /// 经 shell 泵串行执行的 `exec` 结果通道(未完成时 UI 仍可交互)
    pending_raw: Option<Receiver<Result<String, String>>>,
    /// 本帧 `SidePanel` 槽位矩形（`ui.max_rect()`，与布局占位一致）
    last_panel_slot_rect: Option<egui::Rect>,
}

impl Default for MonitorPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl MonitorPanel {
    pub fn new() -> Self {
        Self {
            monitor: None,
            auto_refresh: true,
            refresh_interval_secs: 5.0,
            alert_cpu_pct: 80.0,
            alert_mem_pct: 90.0,
            alert_disk_pct: 85.0,
            last_ui_refresh: 0.0_f64,
            last_error: None,
            pending_raw: None,
            last_panel_slot_rect: None,
        }
    }

    /// 初始化监控器(使用现有 SSH 连接与对应的 `SshManager` 克隆以供 exec)
    pub fn init(
        &mut self,
        ssh_handle: crate::ssh::SshSessionHandle,
        ssh_manager: crate::ssh::SshManager,
    ) {
        self.pending_raw = None;
        self.monitor = Some(Monitor::new(ssh_handle, ssh_manager));
        self.last_error = None;
        self.begin_async_collect();
    }

    /// 清空采集状态(切换至无 SSH 的标签或未连接时调用)。
    pub fn clear(&mut self) {
        self.pending_raw = None;
        self.monitor = None;
        self.last_error = None;
    }

    /// 若当前无进行中的采集,则向 shell 泵排队一次 `exec`(不得另开线程,以免与 PTY 争用 `Session`)。
    fn begin_async_collect(&mut self) {
        if self.monitor.is_none() {
            return;
        }
        if self.pending_raw.is_some() {
            return;
        }
        let m = self.monitor.as_ref().unwrap();
        match m
            .ssh_session_handle()
            .enqueue_remote_exec(Monitor::COLLECT_CMD)
        {
            Ok(rx) => {
                self.pending_raw = Some(rx);
            }
            Err(e) => {
                self.last_error = Some(e);
            }
        }
    }

    fn poll_bg_collect(&mut self, ctx: &egui::Context) {
        let msg = self
            .pending_raw
            .as_ref()
            .map(|rx| rx.try_recv());
        match msg {
            None => {}
            Some(Ok(Ok(raw))) => {
                self.pending_raw = None;
                self.last_ui_refresh = ctx.input(|i| i.time);
                if let Some(monitor) = &mut self.monitor {
                    match monitor.ingest_remote_output(&raw) {
                        Ok(_) => self.last_error = None,
                        Err(e) => self.last_error = Some(e),
                    }
                }
                ctx.request_repaint();
            }
            Some(Ok(Err(e))) => {
                self.pending_raw = None;
                self.last_ui_refresh = ctx.input(|i| i.time);
                self.last_error = Some(format!(
                    "{}{}",
                    i18n::tr(ctx, "Monitor collection failed: ", "监控采集失败："),
                    e
                ));
                ctx.request_repaint();
            }
            Some(Err(TryRecvError::Empty)) => {
                ctx.request_repaint_after(Duration::from_millis(120));
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.pending_raw = None;
                self.last_ui_refresh = ctx.input(|i| i.time);
                self.last_error = Some(
                    i18n::tr(ctx, "Collection channel disconnected", "采集结果通道已断开")
                        .to_string(),
                );
                ctx.request_repaint();
            }
        }
    }

    /// 手动触发一次后台采集(不阻塞 UI)
    pub fn refresh(&mut self) {
        self.begin_async_collect();
    }

    /// 是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.monitor.is_some()
    }

    /// 底栏摘要：CPU / 内存（无有效采集数据时返回 None）
    pub fn status_bar_metrics_line(&self, egui_ctx: &egui::Context) -> Option<String> {
        let monitor = self.monitor.as_ref()?;
        let stats = monitor.last_stats();
        if stats.memory_total == 0 && stats.disk_total == 0 && stats.uptime_secs == 0 {
            return None;
        }
        let cpu_lbl = i18n::tr(egui_ctx, "CPU", "CPU");
        Some(format!(
            "{} {:.0}% · {}",
            cpu_lbl, stats.cpu_percent, stats.format_memory()
        ))
    }

    /// 判断当前快照是否已具备有效指标（避免全零占位触发误告警）。
    fn stats_look_valid(stats: &ServerStats) -> bool {
        stats.memory_total > 0 || stats.disk_total > 0 || stats.uptime_secs > 0
    }

    /// 当前采样下超过阈值的告警文案（本地规则，Week 10 告警设置的最小可用版）。
    fn collect_alerts_with(
        loc: Locale,
        cpu_th: f32,
        mem_th: f32,
        disk_th: f32,
        stats: &ServerStats,
    ) -> Vec<String> {
        if !Self::stats_look_valid(stats) {
            return Vec::new();
        }
        let th = loc.tr("threshold", "阈值");
        let mut v = Vec::new();
        if stats.cpu_percent >= cpu_th {
            v.push(format!(
                "{} {:.1}% ≥ {} {:.0}%",
                loc.tr("CPU", "CPU"),
                stats.cpu_percent,
                th,
                cpu_th
            ));
        }
        let mem = stats.memory_percent();
        if mem >= mem_th {
            v.push(format!(
                "{} {:.1}% ≥ {} {:.0}%",
                loc.tr("Memory", "内存"),
                mem,
                th,
                mem_th
            ));
        }
        let disk = stats.disk_percent();
        if disk >= disk_th {
            v.push(format!(
                "{} {:.1}% ≥ {} {:.0}%",
                loc.tr("Disk", "磁盘"),
                disk,
                th,
                disk_th
            ));
        }
        v
    }

    fn collect_alerts(&self, loc: Locale, stats: &ServerStats) -> Vec<String> {
        Self::collect_alerts_with(
            loc,
            self.alert_cpu_pct,
            self.alert_mem_pct,
            self.alert_disk_pct,
            stats,
        )
    }

    /// 每帧更新:拉取 shell 泵返回的采集结果,并在开启自动刷新时排队下一次采集。
    pub fn update(&mut self, ctx: &egui::Context, panel_open: bool) {
        if !panel_open {
            self.pending_raw = None;
            return;
        }

        self.poll_bg_collect(ctx);

        if !self.auto_refresh {
            return;
        }

        let now = ctx.input(|i| i.time);
        if self.pending_raw.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
            return;
        }
        if now - self.last_ui_refresh >= f64::from(self.refresh_interval_secs) {
            self.last_ui_refresh = now;
            self.begin_async_collect();
            ctx.request_repaint_after(Duration::from_secs_f32(self.refresh_interval_secs));
        }
    }

    /// 注册监控栏槽位（须在 Central 之前）。正文见 [`show_foreground_panel`]。
    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        if !*open {
            self.last_panel_slot_rect = None;
            return;
        }

        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right(layout_util::MONITOR_PANEL_ID)
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                crate::ui::chrome::paint_right_dock_left_gap(ui, theme);
                self.last_panel_slot_rect = Some(ui.max_rect());
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
        let _ = theme;
    }

    /// Central 之后绘制监控侧栏（避免被 CentralPanel 盖住）。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
    ) {
        if !*open {
            return;
        }
        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            layout_util::MONITOR_PANEL_ID,
            layout_util::SidePanelProfile::Monitor,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_monitor_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_monitor_fg",
            ctx,
            &geom,
            crate::ui::layout_util::SidePanelProfile::Monitor,
            |ui, _body_w| {
                let loc_fg = i18n::locale(ctx);
                let alert_count = self.monitor.as_ref().and_then(|mon| {
                        let alerts = Self::collect_alerts_with(
                            loc_fg,
                            self.alert_cpu_pct,
                            self.alert_mem_pct,
                            self.alert_disk_pct,
                            mon.last_stats(),
                        );
                        if alerts.is_empty() {
                            None
                        } else {
                            Some(alerts.len())
                        }
                    });
                    let prev_gap_y = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    theme.frame_right_dock_header_band().show(ui, |ui| {
                            layout_util::set_width_to_available(ui);
                            crate::ui::chrome::dock_header_horizontal(ui, theme, |ui| {
                                ui.horizontal(|ui| {
                                    crate::ui::chrome::panel_header_title_leading(
                                        ui,
                                        theme,
                                        crate::ui::icons::IconId::Monitor,
                                        crate::i18n::tr(
                                            ui.ctx(),
                                            "System Monitor",
                                            "系统监控",
                                        ),
                                    );
                                    if let Some(n) = alert_count {
                                        crate::ui::icons::icon_label_row(
                                            ui,
                                            crate::ui::icons::IconId::Warning,
                                            &format!(
                                                "{}{}",
                                                n,
                                                i18n::tr(ui.ctx(), " alerts", " 项告警"),
                                            ),
                                            theme.font_size_medium(),
                                            5.0,
                                            |t| {
                                                t.size(theme.font_size_medium())
                                                    .color(theme.red_color())
                                            },
                                        );
                                    }
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::ui::chrome::dock_close_icon_button(
                                            ui,
                                            theme,
                                            crate::i18n::tr(
                                                ui.ctx(),
                                                "Hide side panel · switch with footer Monitor",
                                                "隐藏侧栏 · 也可用底部「监控」切换",
                                            ),
                                        )
                                        .clicked()
                                        {
                                            *open = false;
                                        }
                                    },
                                );
                            });
                        });
                    crate::ui::chrome::right_dock_header_divider(ui, theme);
                    ui.spacing_mut().item_spacing.y = prev_gap_y;
                    ui.add_space(theme.spacing_xs());

                    let scroll_h = ui.available_height().max(120.0);
                    let prev_extreme = ui.visuals().extreme_bg_color;
                    ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
                    egui::ScrollArea::vertical()
                        .id_source("mistterm_monitor_scroll")
                        .auto_shrink([false; 2])
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            layout_util::set_width_to_available(ui);
                            self.show_content(ui, theme);
                        });
                    ui.visuals_mut().extreme_bg_color = prev_extreme;
        },
        );
    }

    fn show_content(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        layout_util::set_width_to_available(ui);
        let loc = i18n::locale(ui.ctx());
        ui.vertical(|ui| {
            layout_util::set_width_to_available(ui);
            ui.horizontal(|ui| {
                crate::ui::chrome::form_checkbox(
                    ui,
                    theme,
                    &mut self.auto_refresh,
                    crate::i18n::tr(ui.ctx(), "Auto refresh", "自动刷新"),
                );
                if crate::ui::chrome::panel_toolbar_icon_button_or_busy(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Refresh,
                    crate::i18n::tr(ui.ctx(), "Refresh", "刷新"),
                    self.pending_raw.is_some(),
                )
                .clicked()
                {
                    self.refresh();
                }
            });
            if self.auto_refresh {
                crate::ui::chrome::labeled_slider_f32(
                    ui,
                    theme,
                    &mut self.refresh_interval_secs,
                    1.0..=30.0,
                    crate::i18n::tr(ui.ctx(), "Interval (s)", "间隔 (秒)"),
                    "",
                );
            }
        });
        ui.add_space(theme.spacing_sm());

        egui::CollapsingHeader::new(
            egui::RichText::new(i18n::tr(ui.ctx(), "Alert thresholds", "告警阈值"))
                .size(theme.font_size_medium())
                .color(theme.text_secondary()),
        )
        .default_open(false)
        .show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.label(
                egui::RichText::new(i18n::tr(
                    ui.ctx(),
                    "When exceeded, show alerts in header and below (this session only).",
                    "超出阈值时在标题与下方显示告警（仅当前会话）。",
                ))
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
            );
            ui.add_space(4.0);
            for (pct, label) in [
                (
                    &mut self.alert_cpu_pct,
                    i18n::tr(ui.ctx(), "CPU alert %", "CPU 告警 %"),
                ),
                (
                    &mut self.alert_mem_pct,
                    i18n::tr(ui.ctx(), "Memory alert %", "内存告警 %"),
                ),
                (
                    &mut self.alert_disk_pct,
                    i18n::tr(ui.ctx(), "Disk alert %", "磁盘告警 %"),
                ),
            ] {
                crate::ui::chrome::labeled_slider_f32(
                    ui,
                    theme,
                    pct,
                    50.0..=100.0,
                    label,
                    "%",
                );
            }
        });
        ui.add_space(theme.spacing_md());

        if let Some(ref monitor) = self.monitor {
            let stats = monitor.last_stats();
            let history = monitor.get_history();
            let (rx_rate, tx_rate) = monitor.network_rate();
            let alerts = self.collect_alerts(loc, stats);
            if !alerts.is_empty() {
                theme.frame_monitor_alert()
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(i18n::tr(ui.ctx(), "Current alerts", "当前告警"))
                                .size(theme.font_size_medium())
                                .color(theme.red_color()),
                        );
                        ui.add_space(4.0);
                        for line in &alerts {
                            ui.label(
                                egui::RichText::new(line)
                                    .size(theme.font_size_small())
                                    .color(theme.text_primary()),
                            );
                        }
                    });
                ui.add_space(theme.spacing_md());
            }

            crate::ui::chrome::dock_label_value_row(
                ui,
                theme,
                crate::ui::icons::IconId::Timer,
                loc.tr("Uptime", "运行时间"),
                stats.format_uptime(),
            );
            ui.add_space(theme.spacing_sm());

            // CPU 使用率
            self.show_metric_bar(
                ui,
                theme,
                crate::ui::icons::IconId::Cpu,
                loc.tr("CPU", "CPU"),
                stats.cpu_percent,
                format!("{:.1}%", stats.cpu_percent),
                cpu_color(stats.cpu_percent, theme),
            );

            self.show_metric_bar(
                ui,
                theme,
                crate::ui::icons::IconId::Memory,
                loc.tr("Memory", "内存"),
                stats.memory_percent(),
                stats.format_memory(),
                mem_color(stats.memory_percent(), theme),
            );

            self.show_metric_bar(
                ui,
                theme,
                crate::ui::icons::IconId::Disk,
                loc.tr("Disk", "磁盘"),
                stats.disk_percent(),
                stats.format_disk(),
                disk_color(stats.disk_percent(), theme),
            );

            ui.add_space(theme.spacing_md());
            ui.separator();
            ui.add_space(theme.spacing_sm());

            // 系统负载
            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Chart,
                loc.tr("Load average", "系统负载"),
                theme.font_size_medium(),
                6.0,
                |t| t.size(theme.font_size_medium()).color(theme.text_secondary()),
            );
            ui.horizontal(|ui| {
                let (l1, l5, l15) = stats.load_avg;
                self.load_chip(ui, theme, "1m", l1);
                self.load_chip(ui, theme, "5m", l5);
                self.load_chip(ui, theme, "15m", l15);
            });

            ui.add_space(theme.spacing_md());

            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Network,
                loc.tr("Network throughput", "网络速率"),
                theme.font_size_monitor_section(),
                6.0,
                |t| {
                    t.size(theme.font_size_monitor_section())
                        .color(theme.text_secondary())
                },
            );
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("↓ {}", format_bytes_per_sec(rx_rate)))
                        .monospace()
                        .size(theme.font_size_normal())
                        .color(theme.green_color()),
                );
                ui.add_space(theme.spacing_lg());
                ui.label(
                    egui::RichText::new(format!("↑ {}", format_bytes_per_sec(tx_rate)))
                        .monospace()
                        .size(theme.font_size_normal())
                        .color(theme.accent_color()),
                );
            });

            ui.add_space(theme.spacing_lg() - theme.spacing_sm());
            ui.separator();
            ui.add_space(theme.spacing_sm());

            // 历史图表(egui_plot,至多 60 个采样点)
            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Chart,
                loc.tr("Trend", "历史趋势"),
                theme.font_size_medium(),
                6.0,
                |t| t.size(theme.font_size_medium()).color(theme.text_secondary()),
            );
            ui.add_space(theme.spacing_sm());

            self.show_history_plots(ui, theme, history);

        } else {
            // 未初始化提示
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new(i18n::tr(
                        ui.ctx(),
                        "No SSH session on this tab",
                        "当前标签无可用 SSH 会话",
                    ))
                        .size(theme.font_size_large())
                        .color(theme.text_tertiary()),
                );
                ui.label(
                    egui::RichText::new(i18n::tr(
                        ui.ctx(),
                        "Connect to a server, or switch to a connected tab.",
                        "请先连接服务器，或切换到已连接的标签",
                    ))
                        .size(theme.font_size_normal())
                        .color(theme.text_secondary()),
                );
            });
        }

        // 显示错误信息
        if let Some(ref err) = self.last_error {
            ui.add_space(8.0);
            crate::ui::icons::icon_label_row(
                ui,
                crate::ui::icons::IconId::Warning,
                err,
                theme.font_size_small(),
                6.0,
                |t| t.size(theme.font_size_small()).color(theme.red_color()),
            );
        }
    }

    /// 显示指标进度条
    #[allow(clippy::too_many_arguments)]
    fn show_metric_bar(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        icon: crate::ui::icons::IconId,
        label: &str,
        percent: f32,
        value_text: String,
        bar_color: egui::Color32,
    ) {
        crate::ui::chrome::dock_label_value_row(ui, theme, icon, label, value_text);

        let bar_height = theme.progress_bar_height();
        let available_width = layout_util::set_width_to_available(ui);
        let bg_color = theme.border_color();

        ui.allocate_ui_with_layout(
            egui::vec2(available_width, bar_height + 2.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let (rect, _resp) = ui.allocate_exact_size(
                    egui::vec2(available_width, bar_height),
                    egui::Sense::hover(),
                );

                ui.painter().rect_filled(rect, 4.0, bg_color);

                let fill_width = (percent.clamp(0.0, 100.0) / 100.0 * rect.width()).max(0.0);
                if fill_width > 0.0 {
                    let fill_rect = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(fill_width, rect.height()),
                    );
                    ui.painter().rect_filled(fill_rect, 4.0, bar_color);
                }
            },
        );

        ui.add_space(4.0);
    }

    /// 显示负载标签
    fn load_chip(&self, ui: &mut egui::Ui, theme: &Theme, label: &str, value: f32) {
        let color = if value < 1.0 {
            theme.green_color()
        } else if value < 4.0 {
            theme.amber_color()
        } else {
            theme.red_color()
        };

        egui::Frame::none()
            .fill(theme.border_color())
            .rounding(4.0)
            .inner_margin(theme.margin_monitor_metric_row())
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(theme.font_size_small())
                            .color(theme.text_tertiary()),
                    );
                    ui.label(
                        egui::RichText::new(format!("{:.2}", value))
                            .monospace()
                            .size(theme.font_size_medium())
                            .color(color),
                    );
                });
            });
    }

    /// 历史趋势:`egui_plot` 展示 CPU/内存/磁盘(%)、负载与网络速率(B/s),横轴为相对时间并联动。
    fn show_history_plots(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        history: &[ServerStats],
    ) {
        let loc = i18n::locale(ui.ctx());
        const CHART_HEIGHT: f32 = 110.0;
        let width = layout_util::set_width_to_available(ui);
        let plot_margin = egui::vec2(0.14, 0.14);
        let legend = |corner: Corner| {
            Legend::default()
                .position(corner)
                .background_alpha(0.55)
        };
        let link_x_id = ui.id().with("monitor_hist_time_axis");
        let tip_id = ui.id().with("monitor_history_tooltip");

        if history.len() < 2 {
            ui.label(
                egui::RichText::new(i18n::tr(
                    ui.ctx(),
                    "Waiting for samples… (curve needs at least two refreshes)",
                    "等待数据采集…（至少两次刷新后显示曲线）",
                ))
                    .size(theme.font_size_menu_item())
                    .color(theme.text_tertiary()),
            );
            return;
        }

        let pct_y = loc.tr("Usage %", "使用率 %").to_string();
        let time_x = loc.tr("Time (s)", "时间 (s)").to_string();
        let load_y = loc.tr("Load", "负载").to_string();
        let n = history.len();
        let t0 = history[0].collected_at;
        let t_end = (history[n - 1].collected_at - t0).as_secs_f64().max(0.0);

        let name_cpu = loc.tr("CPU", "CPU");
        let name_mem = loc.tr("Memory", "内存");
        let name_disk = loc.tr("Disk", "磁盘");

        let cpu_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.cpu_percent.clamp(0.0, 100.0));
                [x, y]
            })
            .collect();

        let mem_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.memory_percent().clamp(0.0, 100.0));
                [x, y]
            })
            .collect();

        let disk_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.disk_percent().clamp(0.0, 100.0));
                [x, y]
            })
            .collect();

        let cpu_line = Line::new(cpu_points)
            .name(name_cpu)
            .color(theme.green_color())
            .width(1.6);
        let mem_line = Line::new(mem_points)
            .name(name_mem)
            .color(theme.accent_color())
            .width(1.6);
        let disk_line = Line::new(disk_points)
            .name(name_disk)
            .color(disk_color(72.0_f32, theme))
            .width(1.6);

        ui.label(
            egui::RichText::new(format!(
                "{} / {} / {}",
                loc.tr("CPU", "CPU"),
                loc.tr("Memory", "内存"),
                loc.tr("Disk", "磁盘")
            ))
                .size(theme.font_size_normal())
                .color(theme.text_tertiary()),
        );
        ui.add_space(2.0);

        let mut hover_idx: Option<usize> = None;

        let pct_resp = Plot::new(ui.id().with("mist_monitor_pct"))
            .height(CHART_HEIGHT)
            .width(width)
            .link_axis(link_x_id, true, false)
            .allow_zoom(AxisBools::new(true, true))
            .allow_drag(AxisBools::new(true, true))
            .allow_scroll(true)
            .allow_boxed_zoom(false)
            .view_aspect(2.5)
            .include_x(0.0)
            .include_x(t_end.max(1.0))
            .include_y(0.0)
            .include_y(100.0)
            .set_margin_fraction(plot_margin)
            .y_axis_label(pct_y.clone())
            .x_axis_label(time_x.clone())
            .legend(legend(Corner::LeftTop))
            .show_axes([true, true])
            .show_grid([true, true])
            .label_formatter(|name, value| {
                if name.is_empty() {
                    format!("t={:.1}s  {:.1}%", value.x, value.y)
                } else {
                    format!("{}  t={:.1}s  {:.1}%", name, value.x, value.y)
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.line(cpu_line);
                plot_ui.line(mem_line);
                plot_ui.line(disk_line);

                if plot_ui.response().hovered() {
                    if let Some(pp) = plot_ui.pointer_coordinate() {
                        let xi = pp.x.clamp(0.0, t_end.max(1e-6));
                        let idx = nearest_history_index(history, t0, xi);
                        hover_idx = Some(idx);
                        let snap_x = (history[idx].collected_at - t0).as_secs_f64();
                        plot_ui.vline(
                            VLine::new(snap_x)
                                .color(theme.subtle_line_color())
                                .width(1.0)
                                .style(LineStyle::Dotted { spacing: 4.0 }),
                        );
                    }
                }
            });

        if pct_resp.response.hovered() {
            if let Some(idx) = hover_idx {
                let s = &history[idx];
                let (l1, l5, l15) = s.load_avg;
                let tip = match loc.lang {
                    UiLanguage::En => format!(
                        "Sample {}/{}\n\
                         t = {:.1} s · CPU {:.1}%\n\
                         Memory {:.1}% ({})\n\
                         Disk {:.1}% ({})\n\
                         Load {:.2} / {:.2} / {:.2}",
                        idx + 1,
                        n,
                        (s.collected_at - t0).as_secs_f64(),
                        s.cpu_percent,
                        s.memory_percent(),
                        s.format_memory(),
                        s.disk_percent(),
                        s.format_disk(),
                        l1,
                        l5,
                        l15,
                    ),
                    UiLanguage::Zh => format!(
                        "样本 {}/{}\n\
                         t = {:.1} s · CPU {:.1}%\n\
                         内存 {:.1}%({})\n\
                         磁盘 {:.1}%({})\n\
                         负载 {:.2} / {:.2} / {:.2}",
                        idx + 1,
                        n,
                        (s.collected_at - t0).as_secs_f64(),
                        s.cpu_percent,
                        s.memory_percent(),
                        s.format_memory(),
                        s.disk_percent(),
                        s.format_disk(),
                        l1,
                        l5,
                        l15,
                    ),
                };
                egui::show_tooltip_text(ui.ctx(), tip_id, tip);
            }
        }

        ui.add_space(10.0);

        let load1_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.load_avg.0.max(0.0));
                [x, y]
            })
            .collect();
        let load5_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.load_avg.1.max(0.0));
                [x, y]
            })
            .collect();
        let load15_points: PlotPoints = history
            .iter()
            .map(|s| {
                let x = (s.collected_at - t0).as_secs_f64();
                let y = f64::from(s.load_avg.2.max(0.0));
                [x, y]
            })
            .collect();

        ui.label(
            egui::RichText::new(loc.tr(
                "Load average",
                "负载 (load average)",
            ))
                .size(theme.font_size_normal())
                .color(theme.text_tertiary()),
        );
        ui.add_space(2.0);

        Plot::new(ui.id().with("mist_monitor_load"))
            .height(CHART_HEIGHT)
            .width(width)
            .link_axis(link_x_id, true, false)
            .allow_zoom(AxisBools::new(true, true))
            .allow_drag(AxisBools::new(true, true))
            .allow_scroll(true)
            .allow_boxed_zoom(false)
            .view_aspect(2.5)
            .include_x(0.0)
            .include_x(t_end.max(1.0))
            .include_y(0.0)
            .auto_bounds_y()
            .set_margin_fraction(plot_margin)
            .y_axis_label(load_y.clone())
            .x_axis_label(time_x.clone())
            .legend(legend(Corner::LeftTop))
            .show_axes([true, true])
            .show_grid([true, true])
            .label_formatter(|name, value| {
                if name.is_empty() {
                    format!("t={:.1}s  {:.2}", value.x, value.y)
                } else {
                    format!("{}  t={:.1}s  {:.2}", name, value.x, value.y)
                }
            })
            .show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(load1_points)
                        .name(loc.tr("1 min", "1 分钟"))
                        .color(theme.green_color())
                        .width(1.6),
                );
                plot_ui.line(
                    Line::new(load5_points)
                        .name(loc.tr("5 min", "5 分钟"))
                        .color(theme.amber_color())
                        .width(1.6),
                );
                plot_ui.line(
                    Line::new(load15_points)
                        .name(loc.tr("15 min", "15 分钟"))
                        .color(theme.text_primary())
                        .width(1.6),
                );
            });

        ui.add_space(10.0);

        let mut rx_pts: Vec<[f64; 2]> = Vec::new();
        let mut tx_pts: Vec<[f64; 2]> = Vec::new();
        for i in 1..history.len() {
            let prev = &history[i - 1];
            let curr = &history[i];
            let dt = (curr.collected_at - prev.collected_at).as_secs_f64();
            if dt <= f64::EPSILON {
                continue;
            }
            let x = (curr.collected_at - t0).as_secs_f64();
            let rx = (curr.network_rx_bytes.saturating_sub(prev.network_rx_bytes) as f64) / dt;
            let tx = (curr.network_tx_bytes.saturating_sub(prev.network_tx_bytes) as f64) / dt;
            rx_pts.push([x, rx.max(0.0)]);
            tx_pts.push([x, tx.max(0.0)]);
        }

        ui.label(
            egui::RichText::new(loc.tr(
                "Network throughput",
                "网络速率",
            ))
                .size(theme.font_size_normal())
                .color(theme.text_tertiary()),
        );
        ui.add_space(2.0);

        if rx_pts.is_empty() {
            ui.label(
                egui::RichText::new(i18n::tr(
                    ui.ctx(),
                    "No valid sampling interval yet…",
                    "暂无有效采样间隔...",
                ))
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
            );
        } else {
            let rx_line: PlotPoints = rx_pts.into();
            let tx_line: PlotPoints = tx_pts.into();

            Plot::new(ui.id().with("mist_monitor_net"))
                .height(CHART_HEIGHT)
                .width(width)
                .link_axis(link_x_id, true, false)
                .allow_zoom(AxisBools::new(true, true))
                .allow_drag(AxisBools::new(true, true))
                .allow_scroll(true)
                .allow_boxed_zoom(false)
                .view_aspect(2.5)
                .include_x(0.0)
                .include_x(t_end.max(1.0))
                .include_y(0.0)
                .auto_bounds_y()
                .set_margin_fraction(plot_margin)
                .y_axis_label("B/s")
                .x_axis_label(time_x.clone())
                .y_axis_formatter(|v, _max_chars, _range| format_bytes_per_sec(v))
                .legend(legend(Corner::LeftTop))
                .show_axes([true, true])
                .show_grid([true, true])
                .label_formatter(|name, value| {
                    if name.is_empty() {
                        format!("t={:.1}s  {}", value.x, format_bytes_per_sec(value.y))
                    } else {
                        format!("{}  t={:.1}s  {}", name, value.x, format_bytes_per_sec(value.y))
                    }
                })
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        Line::new(rx_line)
                            .name(loc.tr("Download", "下行"))
                            .color(theme.green_color())
                            .width(1.6),
                    );
                    plot_ui.line(
                        Line::new(tx_line)
                            .name(loc.tr("Upload", "上行"))
                            .color(theme.accent_color())
                            .width(1.6),
                    );
                });
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(i18n::tr(
                ui.ctx(),
                "Tip: up to 60 samples; double‑click resets view; plots share horizontal pan/zoom.",
                "提示：至多保留 60 个采样；双击复位视图；各图横向联动。",
            ))
                .size(theme.font_size_small())
                .color(theme.text_tertiary()),
        );
    }
}

/// CPU 使用率颜色
fn cpu_color(pct: f32, theme: &Theme) -> egui::Color32 {
    if pct < 50.0 {
        theme.green_color()
    } else if pct < 80.0 {
        theme.amber_color()
    } else {
        theme.red_color()
    }
}

/// 内存使用率颜色
fn mem_color(pct: f32, theme: &Theme) -> egui::Color32 {
    if pct < 70.0 {
        theme.accent_color()
    } else if pct < 90.0 {
        theme.amber_color()
    } else {
        theme.red_color()
    }
}

/// 磁盘使用率颜色
fn disk_color(pct: f32, theme: &Theme) -> egui::Color32 {
    if pct < 70.0 {
        theme.accent_color()
    } else if pct < 90.0 {
        theme.amber_color()
    } else {
        theme.red_color()
    }
}

/// 与横轴采样时间最接近的历史点索引(用于悬浮提示)。
fn nearest_history_index(history: &[ServerStats], t0: std::time::Instant, plot_x: f64) -> usize {
    if history.is_empty() {
        return 0;
    }
    history
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da = ((a.collected_at - t0).as_secs_f64() - plot_x).abs();
            let db = ((b.collected_at - t0).as_secs_f64() - plot_x).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// 格式化每秒字节数
fn format_bytes_per_sec(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}
