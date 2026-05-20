//! SSH Config 导入确认弹窗

use crate::core::ssh_config_importer::SshConfigCandidate;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;

const PAGE_SIZE: usize = 20;

pub struct SshConfigImportDialog {
    pub open: bool,
    pub candidates: Vec<SshConfigCandidate>,
    pub selected: Vec<bool>,
    pub already_imported: Vec<bool>,
    pub parse_warnings: Vec<String>,
    pub page: usize,
}

impl Default for SshConfigImportDialog {
    fn default() -> Self {
        Self {
            open: false,
            candidates: Vec::new(),
            selected: Vec::new(),
            already_imported: Vec::new(),
            parse_warnings: Vec::new(),
            page: 0,
        }
    }
}

impl SshConfigImportDialog {
    pub fn set_candidates(
        &mut self,
        candidates: Vec<SshConfigCandidate>,
        already_imported: Vec<bool>,
        parse_warnings: Vec<String>,
    ) {
        self.selected = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| c.importable() && !already_imported.get(i).copied().unwrap_or(false))
            .collect();
        self.candidates = candidates;
        self.already_imported = already_imported;
        self.parse_warnings = parse_warnings;
        self.page = 0;
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
            .filter(|(i, c)| {
                c.importable()
                    && !self.already_imported.get(*i).copied().unwrap_or(false)
                    && self.selected.get(*i).copied().unwrap_or(false)
            })
            .count();

        let total_pages = (self.candidates.len() + PAGE_SIZE - 1) / PAGE_SIZE.max(1);
        let page = self.page.min(total_pages.saturating_sub(1));
        self.page = page;
        let page_start = page * PAGE_SIZE;
        let page_end = (page_start + PAGE_SIZE).min(self.candidates.len());

        egui::Window::new("ssh_config_import")
            .open(&mut self.open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(true)
            .resizable(false)
            .collapsible(false)
            .fixed_size(layout_util::modal_edit_size(ctx))
            .frame(chrome::modal_window_frame(theme))
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    let mut close_hdr = false;
                    if chrome::modal_header(ui, theme, "SSH Config 导入", chrome::modal_title_font_size(theme)) {
                        close_hdr = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "找到以下 SSH 配置，共 {} 个：",
                            self.candidates.len()
                        ))
                        .size(theme.font_size_normal())
                        .color(theme.text_secondary()),
                    );
                    if !self.parse_warnings.is_empty() {
                        ui.add_space(theme.spacing_sm());
                        for w in self.parse_warnings.iter().take(5) {
                            ui.label(
                                egui::RichText::new(w)
                                    .size(theme.font_size_small())
                                    .color(theme.amber_color()),
                            );
                        }
                        if self.parse_warnings.len() > 5 {
                            ui.label(
                                egui::RichText::new(format!(
                                    "… 另有 {} 条解析提示",
                                    self.parse_warnings.len() - 5
                                ))
                                .size(theme.font_size_small())
                                .color(theme.text_tertiary()),
                            );
                        }
                    }
                    ui.add_space(theme.spacing_panel_gap());
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            for i in page_start..page_end {
                                let c = &self.candidates[i];
                                let imported = self.already_imported.get(i).copied().unwrap_or(false);
                                let can = c.importable() && !imported;
                                ui.horizontal(|ui| {
                                    let mut sel = self.selected.get(i).copied().unwrap_or(false);
                                    if !can {
                                        ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                                    } else if ui.checkbox(&mut sel, "").changed() {
                                        if let Some(s) = self.selected.get_mut(i) {
                                            *s = sel;
                                        }
                                    }
                                    let name = egui::RichText::new(&c.host_alias).color(if can {
                                        theme.text_primary()
                                    } else {
                                        theme.text_tertiary()
                                    });
                                    ui.label(name);
                                    ui.label(
                                        egui::RichText::new(format!("→ {}", c.display_target()))
                                            .size(theme.font_size_small())
                                            .color(theme.text_tertiary()),
                                    );
                                    if imported {
                                        ui.label(
                                            egui::RichText::new("(已导入)")
                                                .size(theme.font_size_small())
                                                .color(theme.text_tertiary()),
                                        );
                                    } else if let Some(reason) = &c.skip_reason {
                                        ui.label(
                                            egui::RichText::new(format!("({})", reason))
                                                .size(theme.font_size_small())
                                                .color(theme.amber_color()),
                                        );
                                    }
                                });
                            }
                        });
                    if total_pages > 1 {
                        ui.horizontal(|ui| {
                            if chrome::panel_action_button_ex(ui, theme, "上一页", page > 0)
                                .clicked()
                            {
                                self.page = page.saturating_sub(1);
                            }
                            ui.label(format!("第 {}/{} 页", page + 1, total_pages));
                            if chrome::panel_action_button_ex(
                                ui,
                                theme,
                                "下一页",
                                page + 1 < total_pages,
                            )
                            .clicked()
                            {
                                self.page = page + 1;
                            }
                        });
                    }
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
                                            && !self.already_imported.get(*i).copied().unwrap_or(false)
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
