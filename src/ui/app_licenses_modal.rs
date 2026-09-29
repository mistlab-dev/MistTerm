use super::*;

/// 由 `scripts/generate-third-party-licenses.py` 生成；CI 校验与 `Cargo.lock` 同步。
const THIRD_PARTY_LICENSES: &str = include_str!("../../resources/THIRD_PARTY_LICENSES.txt");

fn third_party_license_lines() -> &'static [&'static str] {
    static LINES: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    LINES.get_or_init(|| THIRD_PARTY_LICENSES.lines().collect())
}

impl MistTermApp {
    pub(crate) fn render_licenses_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if !self.show_licenses_dialog {
            return;
        }
        let mut open = true;
        let mut should_close = false;
        let title = crate::i18n::tr(ctx, "Open-source licenses", "开源许可");
        let modal_sz = layout_util::modal_pref_size(ctx);
        crate::ui::chrome::modal_window("licenses_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .default_size(modal_sz)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    Self::modal_header_title_only(ui, theme, title, &mut should_close);
                    let font = egui::FontId::monospace(theme.font_size_small());
                    let row_height = ui.fonts(|f| f.row_height(&font));
                    let lines = third_party_license_lines();
                    egui::ScrollArea::both()
                        .auto_shrink([false; 2])
                        .show_rows(ui, row_height, lines.len(), |ui, range| {
                            for line in &lines[range] {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(*line)
                                            .font(font.clone())
                                            .color(theme.color_body_text_muted()),
                                    )
                                    .wrap(false),
                                );
                            }
                        });
                });
            });
        self.show_licenses_dialog = open && !should_close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_notices_cover_core_dependencies() {
        for name in ["egui", "ssh2", "libssh2-sys", "alacritty_terminal", "zmodem2"] {
            assert!(
                third_party_license_lines()
                    .iter()
                    .any(|l| l.trim_start().starts_with(&format!("{name} "))),
                "THIRD_PARTY_LICENSES.txt is missing {name}"
            );
        }
    }
}
