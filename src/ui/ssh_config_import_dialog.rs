//! SSH Config 导入确认弹窗

use crate::core::ssh_config_importer::SshConfigCandidate;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;

pub struct SshConfigImportDialog {
    pub open: bool,
    pub candidates: Vec<SshConfigCandidate>,
    pub selected: Vec<bool>,
}

impl Default for SshConfigImportDialog {
    fn default() -> Self {
        Self {
            open: false,
            candidates: Vec::new(),
            selected: Vec::new(),
        }
    }
}

impl SshConfigImportDialog {
    pub fn set_candidates(&mut self, candidates: Vec<SshConfigCandidate>) {
        self.selected = candidates
            .iter()
            .map(|c| c.importable())
            .collect();
        self.candidates = candidates;
        self.open = true;
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) -> Option<Vec<usize>> {
        if !self.open {
            return None;
        }
        let mut import_indices: Option<Vec<usize>> = None;
        let mut should_close = false;
        let importable_count = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(i, c)| c.importable() && self.selected.get(*i).copied().unwrap_or(false))
            .count();

        egui::Window::new("ssh_config_import")
            .open(&mut self.open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(false)
            .resizable(false)
            .collapsible(false)
            .fixed_size(layout_util::modal_edit_size(ctx))
            .frame(chrome::modal_window_frame(theme))
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    let mut close_hdr = false;
                    if chrome::modal_header(ui, theme, "SSH Config 导入", theme.font_size_fragment_dialog_body()) {
                        close_hdr = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "找到以下 SSH 配置，共 {} 个：",
                            self.candidates.len()
                        ))
                        .size(theme.font_size_normal())
                        .color(theme.fg_medium_color()),
                    );
                    ui.add_space(theme.spacing_panel_gap());
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for (i, c) in self.candidates.iter().enumerate() {
                                let can = c.importable();
                                ui.horizontal(|ui| {
                                    let mut sel = self.selected.get(i).copied().unwrap_or(false);
                                    if !can {
                                        ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                                    } else if ui.checkbox(&mut sel, "").changed() {
                                        if let Some(s) = self.selected.get_mut(i) {
                                            *s = sel;
                                        }
                                    }
                                    let name = egui::RichText::new(&c.host_alias)
                                        .color(if can {
                                            theme.fg_high_color()
                                        } else {
                                            theme.fg_low_color()
                                        });
                                    ui.label(name);
                                    ui.label(
                                        egui::RichText::new(format!("→ {}", c.display_target()))
                                            .size(theme.font_size_small())
                                            .color(theme.fg_low_color()),
                                    );
                                    if let Some(reason) = &c.skip_reason {
                                        ui.label(
                                            egui::RichText::new(format!("({})", reason))
                                                .size(theme.font_size_small())
                                                .color(theme.amber_color()),
                                        );
                                    }
                                });
                            }
                        });
                    ui.add_space(theme.spacing_lg());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        let label = if importable_count > 0 {
                            format!("导入所选 ({})", importable_count)
                        } else {
                            "导入所选".to_string()
                        };
                        if chrome::modal_primary_button(ui, th, &label).clicked()
                            && importable_count > 0
                        {
                            import_indices = Some(
                                self.candidates
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, c)| {
                                        c.importable()
                                            && self.selected.get(*i).copied().unwrap_or(false)
                                    })
                                    .map(|(i, _)| i)
                                    .collect(),
                            );
                            should_close = true;
                        }
                        if chrome::modal_secondary_button(ui, th, "取消").clicked() {
                            should_close = true;
                        }
                    });
                    if close_hdr {
                        should_close = true;
                    }
                });
            });
        if should_close {
            self.open = false;
        }
        import_indices
    }
}
