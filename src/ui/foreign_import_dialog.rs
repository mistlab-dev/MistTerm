//! 「从 Xshell / FinalShell 导入」弹窗：选来源和位置 → 预览 → 导入所选。

use std::path::{Path, PathBuf};

use crate::core::foreign_import::{
    default_locations, detect_source, is_already_imported, parse_path, ForeignCandidate,
    ForeignImportOptions, ForeignSource,
};
use crate::core::session::SessionConfig;
use crate::ui::chrome;
use crate::ui::file_dialog::FileDialog;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;

const PAGE_SIZE: usize = 20;

pub struct ForeignImportDialog {
    pub open: bool,
    source: ForeignSource,
    path: Option<PathBuf>,
    candidates: Vec<ForeignCandidate>,
    selected: Vec<bool>,
    already_imported: Vec<bool>,
    warnings: Vec<String>,
    error: Option<String>,
    page: usize,
}

impl Default for ForeignImportDialog {
    fn default() -> Self {
        Self {
            open: false,
            source: ForeignSource::Xshell,
            path: None,
            candidates: Vec::new(),
            selected: Vec::new(),
            already_imported: Vec::new(),
            warnings: Vec::new(),
            error: None,
            page: 0,
        }
    }
}

impl ForeignImportDialog {
    /// 打开弹窗；本机能找到默认位置时直接读出来预览。
    pub fn open_dialog(&mut self, existing: &[SessionConfig]) {
        self.open = true;
        if self.path.is_none() {
            self.switch_source(self.source, existing);
        } else if let Some(p) = self.path.clone() {
            self.load(&p, existing);
        }
    }

    fn switch_source(&mut self, source: ForeignSource, existing: &[SessionConfig]) {
        self.source = source;
        self.clear();
        if let Some(p) = default_locations(source).into_iter().next() {
            self.load(&p, existing);
        }
    }

    fn clear(&mut self) {
        self.path = None;
        self.candidates.clear();
        self.selected.clear();
        self.already_imported.clear();
        self.warnings.clear();
        self.error = None;
        self.page = 0;
    }

    fn load(&mut self, path: &Path, existing: &[SessionConfig]) {
        // 用户选的东西明显是另一种软件的，就跟着切换
        if let Some(detected) = detect_source(path) {
            self.source = detected;
        }
        self.clear();
        self.path = Some(path.to_path_buf());
        match parse_path(self.source, path, &ForeignImportOptions::default()) {
            Ok(r) => {
                self.already_imported = r.candidates.iter().map(|c| is_already_imported(c, existing)).collect();
                self.selected = r
                    .candidates
                    .iter()
                    .zip(&self.already_imported)
                    .map(|(c, done)| c.importable() && !done)
                    .collect();
                self.candidates = r.candidates;
                self.warnings = r.warnings;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    fn selected_candidates(&self) -> Vec<ForeignCandidate> {
        self.candidates
            .iter()
            .enumerate()
            .filter(|(i, c)| {
                c.importable()
                    && !self.already_imported.get(*i).copied().unwrap_or(false)
                    && self.selected.get(*i).copied().unwrap_or(false)
            })
            .map(|(_, c)| c.clone())
            .collect()
    }

    /// 返回用户确认要导入的会话。
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, existing: &[SessionConfig]) -> Option<Vec<ForeignCandidate>> {
        if !self.open {
            return None;
        }
        let zh = crate::i18n::tr(ctx, "en", "zh") == "zh";
        let mut result = None;
        let mut should_close = false;
        let mut pick_folder = false;
        let mut pick_file = false;
        let mut new_source: Option<ForeignSource> = None;
        let chosen = self.selected_candidates().len();

        let total_pages = (self.candidates.len() + PAGE_SIZE - 1) / PAGE_SIZE;
        let page = self.page.min(total_pages.saturating_sub(1));
        self.page = page;
        let page_start = page * PAGE_SIZE;
        let page_end = (page_start + PAGE_SIZE).min(self.candidates.len());

        let modal_sz = layout_util::modal_edit_size(ctx);
        let mut open = self.open;
        chrome::modal_window("foreign_session_import", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Import from Xshell / FinalShell", "从 Xshell / FinalShell 导入"),
                        chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    let active = match self.source {
                        ForeignSource::Xshell => "xshell",
                        ForeignSource::FinalShell => "finalshell",
                    };
                    if let Some(v) = chrome::segmented_control_row(
                        ui,
                        theme,
                        &[("xshell", "Xshell"), ("finalshell", "FinalShell")],
                        active,
                        Some(260.0),
                    ) {
                        new_source = Some(if v == "finalshell" { ForeignSource::FinalShell } else { ForeignSource::Xshell });
                    }
                    ui.add_space(theme.spacing_sm());
                    let hint = match self.source {
                        ForeignSource::Xshell => crate::i18n::tr(
                            ctx,
                            "Choose Xshell's Sessions folder (Documents\\NetSarang Computer\\<version>\\Xshell\\Sessions), or a .xts file from Xshell's File → Export.",
                            "选择 Xshell 的 Sessions 文件夹（在「文档\\NetSarang Computer\\版本号\\Xshell\\Sessions」），或 Xshell「文件 → 导出」得到的 .xts 文件。",
                        ),
                        ForeignSource::FinalShell => crate::i18n::tr(
                            ctx,
                            "Choose FinalShell's data folder (the one containing a conn folder). Close FinalShell first so everything is saved.",
                            "选择 FinalShell 的数据目录（里面有 conn 文件夹）。先关掉 FinalShell，保证设置都已保存。",
                        ),
                    };
                    ui.label(egui::RichText::new(hint).size(theme.font_size_small()).color(theme.text_secondary()));
                    ui.add_space(theme.spacing_xs());
                    ui.horizontal(|ui| {
                        if chrome::panel_action_button_ex(ui, theme, crate::i18n::tr(ctx, "Choose folder…", "选择文件夹…"), true).clicked() {
                            pick_folder = true;
                        }
                        let file_label = match self.source {
                            ForeignSource::Xshell => crate::i18n::tr(ctx, "Choose .xts / .xsh file…", "选择 .xts / .xsh 文件…"),
                            ForeignSource::FinalShell => crate::i18n::tr(ctx, "Choose a connection file…", "选择单个连接文件…"),
                        };
                        if chrome::panel_action_button_ex(ui, theme, file_label, true).clicked() {
                            pick_file = true;
                        }
                    });
                    if let Some(p) = &self.path {
                        ui.label(egui::RichText::new(p.display().to_string()).size(theme.font_size_small()).color(theme.text_tertiary()));
                    }
                    if let Some(e) = &self.error {
                        ui.label(egui::RichText::new(e).size(theme.font_size_small()).color(theme.amber_color()));
                    }
                    for w in self.warnings.iter().take(4) {
                        ui.label(egui::RichText::new(w).size(theme.font_size_small()).color(theme.amber_color()));
                    }
                    ui.add_space(theme.spacing_sm());
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for i in page_start..page_end {
                            let c = &self.candidates[i];
                            let imported = self.already_imported.get(i).copied().unwrap_or(false);
                            let can = c.importable() && !imported;
                            ui.horizontal(|ui| {
                                let mut sel = self.selected.get(i).copied().unwrap_or(false);
                                if !can {
                                    ui.add_enabled(false, egui::Checkbox::without_text(&mut false));
                                } else if chrome::form_checkbox_with_id(ui, theme, ("foreign_import_sel", i), &mut sel, "").changed() {
                                    if let Some(s) = self.selected.get_mut(i) {
                                        *s = sel;
                                    }
                                }
                                ui.label(egui::RichText::new(&c.name).color(if can { theme.text_primary() } else { theme.text_tertiary() }));
                                ui.label(
                                    egui::RichText::new(format!("{} · {}", c.group, c.display_target()))
                                        .size(theme.font_size_small())
                                        .color(theme.text_tertiary()),
                                );
                                let tail = if imported {
                                    Some((if zh { "已导入".to_string() } else { "already imported".to_string() }, theme.text_tertiary()))
                                } else if let Some(r) = &c.skip_reason {
                                    Some((r.clone(), theme.amber_color()))
                                } else if c.password.is_some() {
                                    Some((if zh { "含密码".to_string() } else { "password included".to_string() }, theme.text_tertiary()))
                                } else {
                                    c.notes.first().map(|n| (n.clone(), theme.amber_color()))
                                };
                                if let Some((t, color)) = tail {
                                    ui.label(egui::RichText::new(format!("（{t}）")).size(theme.font_size_small()).color(color));
                                }
                            });
                        }
                    });
                    if total_pages > 1 {
                        ui.horizontal(|ui| {
                            if chrome::panel_action_button_ex(ui, theme, crate::i18n::tr(ctx, "Previous", "上一页"), page > 0).clicked() {
                                self.page = page.saturating_sub(1);
                            }
                            ui.label(format!("{}/{}", page + 1, total_pages));
                            if chrome::panel_action_button_ex(ui, theme, crate::i18n::tr(ctx, "Next", "下一页"), page + 1 < total_pages).clicked() {
                                self.page = page + 1;
                            }
                        });
                    }
                    ui.add_space(theme.spacing_lg());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        let label = if zh { format!("导入所选 ({chosen})") } else { format!("Import selected ({chosen})") };
                        if chrome::modal_primary_button_with_icon(ui, th, crate::ui::icons::IconId::Check, &label).clicked() && chosen > 0 {
                            result = Some(self.selected_candidates());
                            should_close = true;
                        }
                        if chrome::modal_secondary_icon_button(ui, th, crate::ui::icons::IconId::Cross, crate::i18n::tr(ctx, "Cancel", "取消")).clicked() {
                            should_close = true;
                        }
                    });
                });
            });

        if let Some(s) = new_source {
            if s != self.source {
                self.switch_source(s, existing);
            }
        }
        if pick_folder {
            let mut dlg = FileDialog::new();
            if let Some(start) = default_locations(self.source).into_iter().next() {
                dlg = dlg.set_directory(start);
            }
            if let Some(p) = dlg.pick_folder() {
                self.load(&p, existing);
            }
        }
        if pick_file {
            let dlg = match self.source {
                ForeignSource::Xshell => FileDialog::new().add_filter("Xshell", &["xts", "xsh"]),
                ForeignSource::FinalShell => FileDialog::new().add_filter("FinalShell", &["json"]),
            };
            if let Some(p) = dlg.pick_file() {
                self.load(&p, existing);
            }
        }
        if should_close || !open {
            self.open = false;
        }
        result
    }
}
