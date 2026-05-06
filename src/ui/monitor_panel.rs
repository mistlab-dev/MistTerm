//! 监控面板 UI
//!
//! 实时显示服务器资源使用状态

use eframe::egui;
use std::time::Duration;
use crate::monitor::{Monitor, ServerStats, format_bytes};
use crate::ui::theme::Theme;

/// 监控面板组件
pub struct MonitorPanel {
    /// 监控器（None 表示未初始化）
    monitor: Option<Monitor>,
    /// 是否自动刷新
    auto_refresh: bool,
    /// 刷新间隔（秒）
    refresh_interval_secs: f32,
    /// 上次 UI 刷新时间
    last_ui_refresh: f32,
    /// 最后一次错误
    last_error: Option<String>,
    /// 是否可见
    visible: bool,
    /// 手动刷新按钮标签
    refresh_label: String,
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
            auto_refresh: false,
            refresh_interval_secs: 5.0,
            last_ui_refresh: 0.0,
            last_error: None,
            visible: false,
            refresh_label: "📊 监控".to_string(),
        }
    }

    /// 初始化监控器（使用现有 SSH 连接）
    pub fn init(&mut self, ssh_handle: crate::ssh::SshSessionHandle) {
        self.monitor = Some(Monitor::new(ssh_handle));
        self.last_error = None;
        self.refresh();
    }

    /// 手动刷新数据
    pub fn refresh(&mut self) {
        if let Some(ref mut monitor) = self.monitor {
            match monitor.refresh() {
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
    }

    /// 切换面板可见性
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// 是否可见
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.monitor.is_some()
    }

    /// 设置可见性
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// 每帧更新（处理自动刷新）
    pub fn update(&mut self, ctx: &egui::Context) {
        if !self.visible || !self.auto_refresh {
            return;
        }

        let now = ctx.input(|i| i.time);
        if now - self.last_ui_refresh >= self.refresh_interval_secs {
            self.last_ui_refresh = now;
            self.refresh();
            // 请求下次刷新
            ctx.request_repaint_after(Duration::from_secs_f32(self.refresh_interval_secs));
        }
    }

    /// 在右侧面板中绘制监控内容
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.visible {
            return;
        }

        egui::SidePanel::right("monitor_panel")
            .default_width(320.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(theme.bg_window_color())
                    .stroke(egui::Stroke::new(
                        1.0,
                        theme.border_color(),
                    ))
                    .inner_margin(egui::Margin::same(12.0)),
            )
            .show(ctx, |ui| {
                self.show_content(ui, theme);
            });
    }

    fn show_content(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        // 标题栏
        ui.horizontal(|ui| {
            ui.heading(
                egui::RichText::new("📊 系统监控")
                    .size(16.0)
                    .color(theme.fg_high_color()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("×").clicked() {
                    self.visible = false;
                }
            });
        });
        ui.separator();

        // 控制栏
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.auto_refresh, "自动刷新");
            if self.auto_refresh {
                ui.add(
                    egui::Slider::new(&mut self.refresh_interval_secs, 1.0..=30.0)
                        .text("间隔")
                        .suffix("s"),
                );
            }
            if ui.button("🔄 刷新").clicked() {
                self.refresh();
            }
        });
        ui.add_space(8.0);

        if let Some(ref monitor) = self.monitor {
            let stats = monitor.last_stats();
            let history = monitor.get_history();
            let (rx_rate, tx_rate) = monitor.network_rate();

            // 服务器运行时间
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("⏱ 运行时间").size(13.0).color(theme.fg_medium_color()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(stats.format_uptime())
                            .monospace()
                            .size(13.0)
                            .color(theme.fg_high_color()),
                    );
                });
            });
            ui.add_space(4.0);

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

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // 系统负载
            ui.label(egui::RichText::new("📊 系统负载").size(13.0).color(theme.fg_medium_color()));
            ui.horizontal(|ui| {
                let (l1, l5, l15) = stats.load_avg;
                self.load_chip(ui, theme, "1m", l1);
                self.load_chip(ui, theme, "5m", l5);
                self.load_chip(ui, theme, "15m", l15);
            });

            ui.add_space(8.0);

            // 网络流量
            ui.label(egui::RichText::new("🌐 网络速率").size(13.0).color(theme.fg_medium_color()));
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("↓ {}", format_bytes_per_sec(rx_rate)))
                        .monospace()
                        .size(12.0)
                        .color(theme.green_color()),
                );
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(format!("↑ {}", format_bytes_per_sec(tx_rate)))
                        .monospace()
                        .size(12.0)
                        .color(theme.accent_color()),
                );
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);

            // 历史图表标题
            ui.label(egui::RichText::new("📈 历史趋势 (60s)").size(13.0).color(theme.fg_medium_color()));
            ui.add_space(4.0);

            // CPU / 内存折线图
            self.show_history_chart(ui, theme, history);

        } else {
            // 未初始化提示
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new("请先连接服务器")
                        .size(14.0)
                        .color(theme.fg_low_color()),
                );
                ui.label(
                    egui::RichText::new("连接后点击监控按钮启用")
                        .size(12.0)
                        .color(theme.fg_medium_color()),
                );
            });
        }

        // 显示错误信息
        if let Some(ref err) = self.last_error {
            ui.add_space(8.0);
            ui.colored_label(
                theme.red_color(),
                egui::RichText::new(format!("⚠ {}", err)).size(11.0),
            );
        }
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
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(13.0)
                    .color(theme.fg_medium_color()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(&value_text)
                        .monospace()
                        .size(12.0)
                        .color(theme.fg_high_color()),
                );
            });
        });

        let bar_height = 8.0;
        let available_width = ui.available_width();
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
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(10.0)
                            .color(theme.fg_low_color()),
                    );
                    ui.label(
                        egui::RichText::new(format!("{:.2}", value))
                            .monospace()
                            .size(13.0)
                            .color(color),
                    );
                });
            });
    }

    /// 显示历史趋势图（CPU + 内存折线）
    fn show_history_chart(&self, ui: &mut egui::Ui, theme: &Theme, history: &[ServerStats]) {
        let chart_height = 120.0;
        let width = ui.available_width();

        if history.len() < 2 {
            ui.label(
                egui::RichText::new("等待数据采集…")
                    .size(11.0)
                    .color(theme.fg_low_color()),
            );
            return;
        }

        let (rect, _resp) = ui.allocate_exact_size(
            egui::vec2(width, chart_height),
            egui::Sense::hover(),
        );

        let painter = ui.painter();

        // 背景网格
        painter.rect_filled(rect, 4.0, theme.bg_terminal_color());

        // 水平参考线 (25%, 50%, 75%)
        for pct in [25.0, 50.0, 75.0] {
            let y = rect.max.y - (pct / 100.0) * rect.height();
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(0.5, theme.subtle_line_color()),
            );
        }

        // Y 轴刻度
        painter.text(
            egui::pos2(rect.min.x + 4.0, rect.min.y + 2.0),
            egui::Align2::LEFT_TOP,
            "100%",
            egui::FontId::monospace(9.0),
            theme.subtle_label_color(),
        );
        painter.text(
            egui::pos2(rect.min.x + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "50%",
            egui::FontId::monospace(9.0),
            theme.subtle_label_color(),
        );
        painter.text(
            egui::pos2(rect.min.x + 4.0, rect.max.y - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "0%",
            egui::FontId::monospace(9.0),
            theme.subtle_label_color(),
        );

        let n = history.len();
        let x_step = rect.width() / (n - 1).max(1) as f32;

        // CPU 折线（成功/监控主色 — 绿）
        let cpu_line = theme.green_color();
        let mut cpu_points = Vec::with_capacity(n);
        for (i, stats) in history.iter().enumerate() {
            let x = rect.min.x + i as f32 * x_step;
            let y = rect.max.y - (stats.cpu_percent.clamp(0.0, 100.0) / 100.0) * rect.height();
            cpu_points.push(egui::pos2(x, y));
        }
        painter.line_series(cpu_points, egui::Stroke::new(1.5, cpu_line));

        // 内存折线（主强调色）
        let mem_line = theme.accent_color();
        let mut mem_points = Vec::with_capacity(n);
        for (i, stats) in history.iter().enumerate() {
            let x = rect.min.x + i as f32 * x_step;
            let y = rect.max.y - (stats.memory_percent().clamp(0.0, 100.0) / 100.0) * rect.height();
            mem_points.push(egui::pos2(x, y));
        }
        painter.line_series(mem_points, egui::Stroke::new(1.5, mem_line));

        // 图例
        let legend_x = rect.max.x - 80.0;
        let legend_y = rect.min.y + 6.0;

        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(legend_x, legend_y),
                egui::vec2(6.0, 6.0),
            ),
            2.0,
            cpu_line,
        );
        painter.text(
            egui::pos2(legend_x + 10.0, legend_y + 3.0),
            egui::Align2::LEFT_CENTER,
            "CPU",
            egui::FontId::monospace(10.0),
            theme.fg_medium_color(),
        );

        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(legend_x + 40.0, legend_y),
                egui::vec2(6.0, 6.0),
            ),
            2.0,
            mem_line,
        );
        painter.text(
            egui::pos2(legend_x + 50.0, legend_y + 3.0),
            egui::Align2::LEFT_CENTER,
            "MEM",
            egui::FontId::monospace(10.0),
            theme.fg_medium_color(),
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

/// 格式化每秒字节数
fn format_bytes_per_sec(bytes_per_sec: f64) -> String {
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}
