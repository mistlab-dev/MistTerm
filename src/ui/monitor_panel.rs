//! 监控面板 UI
//!
//! 实时显示服务器资源使用状态

use eframe::egui;
use egui_plot::{AxisBools, Corner, Legend, Line, LineStyle, Plot, PlotPoints, VLine};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

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
    /// 手动刷新按钮标签
    refresh_label: String,
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
            refresh_label: "📊 监控".to_string(),
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
        self.refresh_label = "📊 监控 ...".to_string();
        self.begin_async_collect();
    }

    /// 清空采集状态(切换至无 SSH 的标签或未连接时调用)。
    pub fn clear(&mut self) {
        self.pending_raw = None;
        self.monitor = None;
        self.last_error = None;
        self.refresh_label = "📊 监控".to_string();
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
                self.refresh_label = "📊 监控 ✗".to_string();
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
                        Ok(_) => {
                            self.last_error = None;
                            self.refresh_label = "📊 监控 ✓".to_string();
                        }
                        Err(e) => {
                            self.last_error = Some(e);
                            self.refresh_label = "📊 监控 ✗".to_string();
                        }
                    }
                }
                ctx.request_repaint();
            }
            Some(Ok(Err(e))) => {
                self.pending_raw = None;
                self.last_ui_refresh = ctx.input(|i| i.time);
                self.last_error = Some(format!("监控采集失败: {}", e));
                self.refresh_label = "📊 监控 ✗".to_string();
                ctx.request_repaint();
            }
            Some(Err(TryRecvError::Empty)) => {
                ctx.request_repaint();
            }
            Some(Err(TryRecvError::Disconnected)) => {
                self.pending_raw = None;
                self.last_ui_refresh = ctx.input(|i| i.time);
                self.last_error = Some("采集结果通道已断开".to_string());
                self.refresh_label = "📊 监控 ✗".to_string();
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
    pub fn status_bar_metrics_line(&self) -> Option<String> {
        let monitor = self.monitor.as_ref()?;
        let stats = monitor.last_stats();
        if stats.memory_total == 0 && stats.disk_total == 0 && stats.uptime_secs == 0 {
            return None;
        }
        Some(format!(
            "CPU {:.0}% · {}",
            stats.cpu_percent,
            stats.format_memory()
        ))
    }

    /// 判断当前快照是否已具备有效指标（避免全零占位触发误告警）。
    fn stats_look_valid(stats: &ServerStats) -> bool {
        stats.memory_total > 0 || stats.disk_total > 0 || stats.uptime_secs > 0
    }

    /// 当前采样下超过阈值的告警文案（本地规则，Week 10 告警设置的最小可用版）。
    fn collect_alerts_with(
        cpu_th: f32,
        mem_th: f32,
        disk_th: f32,
        stats: &ServerStats,
    ) -> Vec<String> {
        if !Self::stats_look_valid(stats) {
            return Vec::new();
        }
        let mut v = Vec::new();
        if stats.cpu_percent >= cpu_th {
            v.push(format!("CPU {:.1}% ≥ 阈值 {:.0}%", stats.cpu_percent, cpu_th));
        }
        let mem = stats.memory_percent();
        if mem >= mem_th {
            v.push(format!("内存 {:.1}% ≥ 阈值 {:.0}%", mem, mem_th));
        }
        let disk = stats.disk_percent();
        if disk >= disk_th {
            v.push(format!("磁盘 {:.1}% ≥ 阈值 {:.0}%", disk, disk_th));
        }
        v
    }

    fn collect_alerts(&self, stats: &ServerStats) -> Vec<String> {
        Self::collect_alerts_with(
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
    ) {
        if !*open {
            self.last_panel_slot_rect = None;
            return;
        }

        let (m_def, m_min, m_max) =
            crate::ui::layout_util::side_panel_widths(ctx, crate::ui::layout_util::SidePanelProfile::Monitor);
        let panel = egui::SidePanel::right(layout_util::MONITOR_PANEL_ID)
            .default_width(m_def)
            .min_width(m_min)
            .max_width(m_max)
            .resizable(true)
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                self.last_panel_slot_rect = Some(ui.max_rect());
                let w = layout_util::dock_panel_content_width(ui, m_min, m_max);
                let h = ui.available_height().max(1.0);
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
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            layout_util::MONITOR_PANEL_ID,
            layout_util::SidePanelProfile::Monitor,
            None,
        ) else {
            return;
        };
        let paint = layout_util::inset_slot_for_foreground_paint(slot, screen);
        let (_, m_min, m_max) =
            layout_util::side_panel_widths(ctx, layout_util::SidePanelProfile::Monitor);
        let inner = crate::ui::chrome::right_dock_slot_content_rect(paint, theme);
        let panel_w = layout_util::clamp_f32(inner.width(), m_min, m_max);
        let border = theme.border_color();
        crate::ui::chrome::right_dock_foreground_area("mistterm_monitor_fg")
            .constrain_to(paint)
            .fixed_pos(paint.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(paint);
                ui.set_min_size(paint.size());
                ui.set_max_size(paint.size());
                crate::ui::chrome::paint_right_dock_slot_shell(ui, paint, theme);
                ui.allocate_ui_at_rect(inner, |ui| {
                    ui.set_clip_rect(inner);
                    ui.set_min_width(panel_w);
                    ui.set_max_width(panel_w);
                    let alert_label = self.monitor.as_ref().and_then(|mon| {
                        let alerts = Self::collect_alerts_with(
                            self.alert_cpu_pct,
                            self.alert_mem_pct,
                            self.alert_disk_pct,
                            mon.last_stats(),
                        );
                        if alerts.is_empty() {
                            None
                        } else {
                            Some(format!("⚠ {} 项告警", alerts.len()))
                        }
                    });
                    let trailing_w =
                        crate::ui::chrome::panel_header_trailing_width(ui, theme, &[]);
                    if crate::ui::chrome::dock_panel_title_row(
                        ui,
                        theme,
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.label(crate::ui::chrome::rich_dock_title(theme, "📊 系统监控"));
                                if let Some(ref text) = alert_label {
                                    ui.label(
                                        egui::RichText::new(text)
                                            .size(theme.font_size_medium())
                                            .color(theme.red_color()),
                                    );
                                }
                            });
                        },
                        "隐藏侧栏 · 也可用底部「📊 监控」切换",
                        trailing_w,
                        |ui, theme| {
                            crate::ui::chrome::close_icon_button(ui, theme)
                                .on_hover_text("隐藏侧栏 · 也可用底部「📊 监控」切换")
                                .clicked()
                        },
                    ) {
                        *open = false;
                    }
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(panel_w);
                            ui.set_max_width(panel_w);
                            self.show_content(ui, theme, panel_w);
                        });
                });
                ui.painter().vline(
                    paint.max.x - 0.5,
                    paint.y_range(),
                    egui::Stroke::new(1.0, border),
                );
            });
    }

    fn show_content(&mut self, ui: &mut egui::Ui, theme: &Theme, panel_w: f32) {
        ui.set_max_width(panel_w);
        // 控制栏
        ui.horizontal(|ui| {
            ui.set_max_width(panel_w);
            ui.checkbox(&mut self.auto_refresh, "自动刷新");
            if self.auto_refresh {
                ui.add(
                    egui::Slider::new(&mut self.refresh_interval_secs, 1.0..=30.0)
                        .text("间隔")
                        .suffix("s"),
                );
            }
            ui.add_enabled_ui(self.pending_raw.is_none(), |ui| {
                if ui.button("🔄 刷新").clicked() {
                    self.refresh();
                }
            });
        });
        ui.add_space(theme.spacing_sm());

        egui::CollapsingHeader::new(
            egui::RichText::new("告警阈值")
                .size(theme.font_size_medium())
                .color(theme.fg_medium_color()),
        )
        .default_open(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("超出阈值时在标题与下方显示告警（仅当前会话）。")
                    .size(theme.font_size_small())
                    .color(theme.fg_low_color()),
            );
            ui.add_space(4.0);
            ui.add(
                egui::Slider::new(&mut self.alert_cpu_pct, 50.0..=100.0)
                    .text("CPU 告警 %")
                    .suffix("%"),
            );
            ui.add(
                egui::Slider::new(&mut self.alert_mem_pct, 50.0..=100.0)
                    .text("内存告警 %")
                    .suffix("%"),
            );
            ui.add(
                egui::Slider::new(&mut self.alert_disk_pct, 50.0..=100.0)
                    .text("磁盘告警 %")
                    .suffix("%"),
            );
        });
        ui.add_space(theme.spacing_md());

        if self.pending_raw.is_some() {
            ui.label(
                egui::RichText::new("远程采集中...")
                    .size(theme.font_size_normal())
                    .color(theme.fg_medium_color()),
            );
            ui.add_space(theme.spacing_md() - theme.spacing_sm());
        }

        if let Some(ref monitor) = self.monitor {
            let stats = monitor.last_stats();
            let history = monitor.get_history();
            let (rx_rate, tx_rate) = monitor.network_rate();
            let alerts = self.collect_alerts(stats);
            if !alerts.is_empty() {
                theme.frame_monitor_alert()
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("当前告警")
                                .size(theme.font_size_medium())
                                .color(theme.red_color()),
                        );
                        ui.add_space(4.0);
                        for line in &alerts {
                            ui.label(
                                egui::RichText::new(line)
                                    .size(theme.font_size_small())
                                    .color(theme.fg_high_color()),
                            );
                        }
                    });
                ui.add_space(theme.spacing_md());
            }

            // 服务器运行时间
            Self::label_value_row(
                ui,
                theme,
                panel_w,
                egui::RichText::new("⏱ 运行时间")
                    .size(theme.font_size_medium())
                    .color(theme.fg_medium_color()),
                egui::RichText::new(stats.format_uptime())
                    .monospace()
                    .size(theme.font_size_medium())
                    .color(theme.fg_high_color()),
            );
            ui.add_space(theme.spacing_sm());

            // CPU 使用率
            self.show_metric_bar(
                ui,
                theme,
                "🖥 CPU",
                stats.cpu_percent,
                format!("{:.1}%", stats.cpu_percent),
                cpu_color(stats.cpu_percent, theme),
            );

            // 内存使用
            self.show_metric_bar(
                ui,
                theme,
                "💾 内存",
                stats.memory_percent(),
                stats.format_memory(),
                mem_color(stats.memory_percent(), theme),
            );

            // 磁盘使用
            self.show_metric_bar(
                ui,
                theme,
                "💿 磁盘",
                stats.disk_percent(),
                stats.format_disk(),
                disk_color(stats.disk_percent(), theme),
            );

            ui.add_space(theme.spacing_md());
            ui.separator();
            ui.add_space(theme.spacing_sm());

            // 系统负载
            ui.label(egui::RichText::new("📊 系统负载").size(theme.font_size_medium()).color(theme.fg_medium_color()));
            ui.horizontal(|ui| {
                let (l1, l5, l15) = stats.load_avg;
                self.load_chip(ui, theme, "1m", l1);
                self.load_chip(ui, theme, "5m", l5);
                self.load_chip(ui, theme, "15m", l15);
            });

            ui.add_space(theme.spacing_md());

            // 网络流量
            ui.label(
                egui::RichText::new("🌐 网络速率")
                    .size(theme.font_size_monitor_section())
                    .color(theme.fg_medium_color()),
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
            ui.label(egui::RichText::new("📈 历史趋势").size(theme.font_size_medium()).color(theme.fg_medium_color()));
            ui.add_space(theme.spacing_sm());

            self.show_history_plots(ui, theme, history, panel_w);

        } else {
            // 未初始化提示
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new("当前标签无可用 SSH 会话")
                        .size(theme.font_size_large())
                        .color(theme.fg_low_color()),
                );
                ui.label(
                    egui::RichText::new("请先连接服务器,或切换到已连接的标签")
                        .size(theme.font_size_normal())
                        .color(theme.fg_medium_color()),
                );
            });
        }

        // 显示错误信息
        if let Some(ref err) = self.last_error {
            ui.add_space(8.0);
            ui.colored_label(
                theme.red_color(),
                egui::RichText::new(format!("⚠ {}", err)).size(theme.font_size_small()),
            );
        }
    }

    /// 行内左标签 + 右对齐值（限制在 panel_w 内，避免 RTL 按窗宽排版）。
    fn label_value_row(
        ui: &mut egui::Ui,
        theme: &Theme,
        panel_w: f32,
        label: egui::RichText,
        value: egui::RichText,
    ) {
        let _ = theme;
        ui.horizontal(|ui| {
            ui.set_max_width(panel_w);
            ui.label(label);
            let val_w = ui.available_width().max(0.0);
            ui.allocate_ui_with_layout(
                egui::vec2(val_w, 18.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.label(value);
                },
            );
        });
    }

    /// 显示指标进度条
    fn show_metric_bar(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        label: &str,
        percent: f32,
        value_text: String,
        bar_color: egui::Color32,
    ) {
        let row_w = ui.available_width().max(120.0);
        Self::label_value_row(
            ui,
            theme,
            row_w,
            egui::RichText::new(label)
                .size(theme.font_size_medium())
                .color(theme.fg_medium_color()),
            egui::RichText::new(&value_text)
                .monospace()
                .size(theme.font_size_normal())
                .color(theme.fg_high_color()),
        );

        let bar_height = theme.progress_bar_height();
        let available_width = row_w;
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
                            .color(theme.fg_low_color()),
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
        panel_w: f32,
    ) {
        const CHART_HEIGHT: f32 = 110.0;
        let width = ui.available_width().max(160.0).min(panel_w);
        let plot_margin = egui::vec2(0.10, 0.12);
        let legend = |corner: Corner| {
            Legend::default()
                .position(corner)
                .background_alpha(0.55)
        };
        let link_x_id = ui.id().with("monitor_hist_time_axis");
        let tip_id = ui.id().with("monitor_history_tooltip");

        if history.len() < 2 {
            ui.label(
                egui::RichText::new("等待数据采集...(至少两次刷新后显示曲线)")
                    .size(theme.font_size_menu_item())
                    .color(theme.fg_low_color()),
            );
            return;
        }

        let n = history.len();
        let t0 = history[0].collected_at;
        let t_end = (history[n - 1].collected_at - t0).as_secs_f64().max(0.0);

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
            .name("CPU")
            .color(theme.green_color())
            .width(1.6);
        let mem_line = Line::new(mem_points)
            .name("内存")
            .color(theme.accent_color())
            .width(1.6);
        let disk_line = Line::new(disk_points)
            .name("磁盘")
            .color(disk_color(72.0_f32, theme))
            .width(1.6);

        ui.label(
            egui::RichText::new("CPU / 内存 / 磁盘")
                .size(theme.font_size_normal())
                .color(theme.fg_low_color()),
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
            .y_axis_label("使用率 %")
            .x_axis_label("时间 (s)")
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
                let tip = format!(
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
                );
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
            egui::RichText::new("负载 (load average)")
                .size(theme.font_size_normal())
                .color(theme.fg_low_color()),
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
            .y_axis_label("负载")
            .x_axis_label("时间 (s)")
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
                        .name("1 分钟")
                        .color(theme.green_color())
                        .width(1.6),
                );
                plot_ui.line(
                    Line::new(load5_points)
                        .name("5 分钟")
                        .color(theme.amber_color())
                        .width(1.6),
                );
                plot_ui.line(
                    Line::new(load15_points)
                        .name("15 分钟")
                        .color(theme.fg_high_color())
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
            egui::RichText::new("网络速率")
                .size(theme.font_size_normal())
                .color(theme.fg_low_color()),
        );
        ui.add_space(2.0);

        if rx_pts.is_empty() {
            ui.label(
                egui::RichText::new("暂无有效采样间隔...")
                    .size(theme.font_size_small())
                    .color(theme.fg_low_color()),
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
                .x_axis_label("时间 (s)")
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
                            .name("下行")
                            .color(theme.green_color())
                            .width(1.6),
                    );
                    plot_ui.line(
                        Line::new(tx_line)
                            .name("上行")
                            .color(theme.accent_color())
                            .width(1.6),
                    );
                });
        }

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("提示:至多保留 60 个采样;双击复位视图;各图横向联动。")
                .size(theme.font_size_small())
                .color(theme.fg_low_color()),
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
