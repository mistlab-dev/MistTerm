//! 唯一的下次启动提示：有新的 crash-*.log 时展示 #190 的摘要；
//! 没有新报告但上次没有正常退出时，说明这一点。不在展示前写确认。

use super::*;

impl MistTermApp {
    pub(crate) fn render_crash_notice_dialog(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        let Some(notice) = self.crash_notice.clone() else {
            return;
        };
        let mut open = true;
        let mut should_close = false;
        let mut copy_report = false;
        let mut open_folder = false;
        let title = crate::i18n::tr(
            ctx,
            "MistTerm exited unexpectedly last time",
            "上次 MistTerm 异常退出",
        );
        let sz = egui::vec2(520.0, 340.0);
        crate::ui::chrome::modal_window("crash_notice_modal", theme, ctx)
            .open(&mut open)
            .default_pos(crate::ui::layout_util::modal_center_pos(ctx, sz))
            .default_size(sz)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    Self::modal_header(ui, theme, title, &mut should_close);
                    let version = if notice.version.is_empty() {
                        crate::platform::APP_VERSION
                    } else {
                        notice.version.as_str()
                    };
                    let time = if notice.time.is_empty() {
                        "—"
                    } else {
                        notice.time.as_str()
                    };
                    let line = format!(
                        "{} v{version} · {time}",
                        crate::i18n::tr(ctx, "Version", "版本"),
                    );
                    ui.label(
                        egui::RichText::new(line)
                            .size(theme.font_size_panel_title())
                            .color(theme.color_body_text_muted()),
                    );
                    ui.add_space(theme.spacing_sm());
                    if notice.has_report {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "A crash report was saved locally:",
                                "已在本机保存崩溃报告：",
                            ))
                            .size(theme.font_size_small())
                            .color(theme.color_form_hint()),
                        );
                        ui.label(
                            egui::RichText::new(&notice.panic_summary)
                                .monospace()
                                .size(theme.font_size_small())
                                .color(theme.color_danger_emphasis()),
                        );
                        ui.label(
                            egui::RichText::new(&notice.file_name)
                                .monospace()
                                .size(theme.font_size_small())
                                .color(theme.color_form_hint()),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "The last session did not exit cleanly, and no new crash report was written.",
                                "上次没有正常退出，且没有新的崩溃报告。",
                            ))
                            .size(theme.font_size_small())
                            .color(theme.color_form_hint()),
                        );
                    }
                    ui.add_space(theme.spacing_md());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, theme| {
                        if crate::ui::chrome::modal_secondary_button(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Close", "关闭"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                        if crate::ui::chrome::modal_secondary_button(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Copy full report", "复制全文"),
                        )
                        .clicked()
                        {
                            copy_report = true;
                        }
                        if crate::ui::chrome::modal_primary_button(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Open reports folder", "打开报告夹"),
                        )
                        .clicked()
                        {
                            open_folder = true;
                        }
                    });
                });
            });
        if open_folder {
            self.open_crash_report_folder(ctx);
        }
        if copy_report {
            if notice.has_report && !notice.full_text.is_empty() {
                ctx.copy_text(notice.full_text);
                self.notify_auto(
                    crate::i18n::tr(ctx, "Copied to clipboard", "已复制到剪贴板").to_string(),
                );
            } else {
                self.notify_auto(
                    crate::i18n::tr(ctx, "No crash report to copy", "没有可复制的崩溃报告")
                        .to_string(),
                );
            }
        }
        if should_close || !open {
            // 只关掉本轮窗口。不在展示前写确认；下次是否再提示由 last_run.ok 决定。
            self.crash_notice = None;
        }
    }

    pub(crate) fn open_crash_report_folder(&mut self, ctx: &egui::Context) {
        let dir = crate::core::crash_reporter::crash_report_dir();
        let _ = std::fs::create_dir_all(&dir);
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
                    "Failed to open crash reports folder",
                    "无法打开崩溃报告目录",
                )
                .to_string(),
            );
        }
    }
}
