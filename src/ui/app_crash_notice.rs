//! #189：下次启动展示上次崩溃摘要，并提供打开目录 / 复制全文 / 关闭。

use super::*;

impl MistTermApp {
    pub(crate) fn render_startup_crash_notice(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if self.startup_crash_notice.is_none() {
            return;
        }
        let mut open = true;
        let mut should_close = false;
        let mut open_folder = false;
        let mut copy_report = false;
        let title = crate::i18n::tr(ctx, "MistTerm quit unexpectedly", "MistTerm 异常退出");
        let base = layout_util::modal_quick_fragment_size(ctx);
        let modal_sz = egui::vec2(base.x.max(480.0), base.y.max(280.0));
        let modal_resp = crate::ui::chrome::modal_window("startup_crash_notice", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    Self::modal_header(ui, theme, title, &mut should_close);
                    let Some(notice) = self.startup_crash_notice.as_ref() else {
                        return;
                    };
                    let intro = if notice.report_is_new {
                        crate::i18n::tr(
                            ctx,
                            "A crash report was saved. Summary:",
                            "已保存崩溃报告，摘要如下。",
                        )
                    } else if notice.report.is_some() {
                        crate::i18n::tr(
                            ctx,
                            "The last session did not exit cleanly. The latest report on disk is shown below.",
                            "上次没有正常退出。下面是磁盘上最近一份崩溃报告。",
                        )
                    } else {
                        crate::i18n::tr(
                            ctx,
                            "The last session did not exit cleanly, and no crash report was found.",
                            "上次没有正常退出，且没有找到崩溃报告。",
                        )
                    };
                    ui.scope(|ui| {
                        ui.style_mut().wrap = Some(true);
                        ui.label(
                            egui::RichText::new(intro)
                                .size(theme.font_size_panel_title())
                                .color(theme.color_body_text_muted()),
                        );
                    });
                    ui.add_space(theme.spacing_md());
                    let version = notice
                        .report
                        .as_ref()
                        .map(|r| r.version.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| crate::platform::APP_VERSION.to_string());
                    let time = notice
                        .report
                        .as_ref()
                        .map(|r| r.time.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "—".to_string());
                    let panic_line = notice
                        .report
                        .as_ref()
                        .map(|r| r.panic_first_line.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "—".to_string());
                    let meta = format!(
                        "{}\n{}\n{}",
                        format!(
                            "{} {version}",
                            crate::i18n::tr(ctx, "Version:", "版本：")
                        ),
                        format!("{} {time}", crate::i18n::tr(ctx, "Time:", "时间：")),
                        format!(
                            "{} {panic_line}",
                            crate::i18n::tr(ctx, "Panic:", "原因：")
                        ),
                    );
                    ui.scope(|ui| {
                        ui.style_mut().wrap = Some(true);
                        ui.label(
                            egui::RichText::new(meta)
                                .font(egui::FontId::monospace(theme.font_size_small()))
                                .color(theme.color_caption_text()),
                        );
                    });
                    ui.add_space(theme.spacing_md());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_primary_button(
                            ui,
                            th,
                            crate::i18n::tr(ctx, "Open reports folder", "打开报告文件夹"),
                        )
                        .clicked()
                        {
                            open_folder = true;
                        }
                        if crate::ui::chrome::modal_secondary_button(
                            ui,
                            th,
                            crate::i18n::tr(ctx, "Copy full report", "复制全文"),
                        )
                        .clicked()
                        {
                            copy_report = true;
                        }
                        if crate::ui::chrome::modal_secondary_button(
                            ui,
                            th,
                            crate::i18n::tr(ctx, "Dismiss", "关闭"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                });
            });
        if let Some(inner) = &modal_resp {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        if open_folder {
            self.open_crash_report_folder(ctx);
        }
        if copy_report {
            self.copy_startup_crash_report(ctx);
        }
        if should_close || !open {
            if let Some(notice) = self.startup_crash_notice.take() {
                crate::core::crash_reporter::acknowledge(&notice);
            }
        }
    }

    pub(crate) fn open_crash_report_folder(&mut self, ctx: &egui::Context) {
        let Ok(dir) = crate::core::crash_reporter::ensure_crash_report_dir() else {
            self.notify_auto(
                crate::i18n::tr(
                    ctx,
                    "Failed to prepare crash report folder",
                    "无法准备崩溃报告目录",
                )
                .to_string(),
            );
            return;
        };
        if crate::platform::reveal_directory(&dir) {
            self.notify_auto(match crate::i18n::language(ctx) {
                crate::i18n::UiLanguage::En => {
                    format!("Opened crash reports folder: {}", dir.display())
                }
                crate::i18n::UiLanguage::Zh => {
                    format!("已打开崩溃报告目录：{}", dir.display())
                }
            });
        } else {
            self.notify_auto(
                crate::i18n::tr(
                    ctx,
                    "Failed to open crash report folder",
                    "无法打开崩溃报告目录",
                )
                .to_string(),
            );
        }
    }

    fn copy_startup_crash_report(&mut self, ctx: &egui::Context) {
        let text = self
            .startup_crash_notice
            .as_ref()
            .and_then(|n| n.report.as_ref())
            .map(|r| r.full_text.clone())
            .filter(|t| !t.is_empty());
        let Some(text) = text else {
            self.notify_auto(
                crate::i18n::tr(ctx, "No crash report to copy", "没有可复制的崩溃报告").to_string(),
            );
            return;
        };
        ctx.copy_text(text);
        self.notify_auto(
            crate::i18n::tr(
                ctx,
                "Copied crash report to clipboard",
                "已复制崩溃报告到剪贴板",
            )
            .to_string(),
        );
    }
}
