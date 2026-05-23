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

        let modal_sz = layout_util::modal_edit_size(ctx);
        chrome::modal_window("ssh_config_import", theme, ctx)
            .open(&mut self.open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    let mut close_hdr = false;
                    if chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Import SSH Config", "SSH Config 导入"),
                        chrome::modal_title_font_size(theme),
                    ) {
                        close_hdr = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}{}",
                            crate::i18n::tr(ctx, "Found ", "找到以下 SSH 配置，共 "),
                            self.candidates.len(),
                            crate::i18n::tr(ctx, " SSH host entries:", " 个："),
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
                                egui::RichText::new(                                format!(
                                    "{} {} {}",
                                    crate::i18n::tr(ctx, "… ", "… 另有"),
                                    self.parse_warnings.len() - 5,
                                    crate::i18n::tr(ctx, " more parse hints", " 条解析提示"),
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
                                    } else if chrome::form_checkbox(ui, theme, &mut sel, "").changed() {
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
                                            egui::RichText::new(crate::i18n::tr(
                                                ctx,
                                                "(already imported)",
                                                "(已导入)",
                                            ))
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
                            if chrome::panel_action_icon_button_ex(
                                ui,
                                theme,
                                crate::ui::icons::IconId::ChevronLeft,
                                crate::i18n::tr(ctx, "Previous", "上一页"),
                                page > 0,
                            )
                            .clicked()
                            {
                                self.page = page.saturating_sub(1);
                            }
                            ui.label(format!(
                                "{}{}/{}{}",
                                crate::i18n::tr(ctx, "Page ", "第 "),
                                page + 1,
                                total_pages,
                                crate::i18n::tr(ctx, "", " 页"),
                            ));
                            if chrome::panel_action_icon_button_ex(
                                ui,
                                theme,
                                crate::ui::icons::IconId::ChevronRight,
                                crate::i18n::tr(ctx, "Next", "下一页"),
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
                            format!(
                                "{}{})",
                                crate::i18n::tr(ctx, "Import selected (", "导入所选 ("),
                                importable_count,
                            )
                        } else {
                            crate::i18n::tr(ctx, "Import selected", "导入所选").to_string()
                        };
                        if chrome::modal_primary_icon_button(ui, th, crate::ui::icons::IconId::Check, &label)
                            .clicked()
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
                        if chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                            .clicked() {
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
