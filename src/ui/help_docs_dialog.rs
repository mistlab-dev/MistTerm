//! 帮助弹窗：结构化排版（快速入门 / 快捷键），完整文档用系统应用打开。

use crate::platform::{docs, shortcuts};
use crate::ui::chrome;
use crate::ui::theme::Theme;
use eframe::egui::{self, FontId, RichText, Ui};
use std::path::PathBuf;

struct QuickStep {
    title: &'static str,
    detail: &'static str,
    keys: Vec<String>,
}

fn quick_steps() -> Vec<QuickStep> {
    vec![
        QuickStep {
            title: "连接与标签",
            detail: "左侧选择或新建连接；双击 / 回车打开终端标签。",
            keys: vec![shortcuts::accel("N"), shortcuts::accel("T")],
        },
        QuickStep {
            title: "底栏工具",
            detail: "SFTP 文件、命令片段、系统监控等底栏图标入口；终端搜索见菜单或快捷键 F。",
            keys: vec![],
        },
        QuickStep {
            title: "视图与面板",
            detail: "菜单「视图」可开关右侧 SFTP、片段侧栏、监控面板。",
            keys: vec![],
        },
        QuickStep {
            title: "工具与数据",
            detail: "片段库、凭证管理、云端同步、会话日志在「工具」菜单。",
            keys: vec![],
        },
        QuickStep {
            title: "终端内操作",
            detail: "搜索当前屏输出；已连接时可用命令历史。",
            keys: vec![
                shortcuts::accel("F"),
                shortcuts::terminal_history_accel().to_string(),
            ],
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpPage {
    #[default]
    QuickStart,
    Shortcuts,
}

impl HelpPage {
    fn label(self) -> &'static str {
        match self {
            Self::QuickStart => "快速入门",
            Self::Shortcuts => "键盘快捷键",
        }
    }
}

pub struct HelpDocsDialog {
    pub open: bool,
    pub page: HelpPage,
}

impl Default for HelpDocsDialog {
    fn default() -> Self {
        Self {
            open: false,
            page: HelpPage::default(),
        }
    }
}

impl HelpDocsDialog {
    pub fn open_page(&mut self, page: HelpPage) {
        self.page = page;
        self.open = true;
    }

    pub fn open_markdown_in_system(doc_rel_path: &str) -> Result<(), String> {
        let path: PathBuf = docs::docs_directory().join(doc_rel_path);
        crate::platform::open_file(&path)
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        shortcuts_text: &str,
        status_message: &mut String,
    ) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        let mut should_close = false;
        egui::Window::new("help_docs_modal")
            .open(&mut open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(true)
            .resizable(true)
            .collapsible(false)
            .default_size(egui::vec2(560.0, 480.0))
            .frame(chrome::modal_window_frame(theme))
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(
                        ui,
                        theme,
                        "帮助",
                        theme.font_size_prominent(),
                    ) {
                        should_close = true;
                    }
                    render_help_tabs(ui, theme, &mut self.page);
                    ui.add_space(theme.spacing_md());
                    egui::Frame::none()
                        .fill(theme.color_subtle_inset_fill())
                        .stroke(egui::Stroke::new(1.0, theme.border_divider_color()))
                        .rounding(theme.radius_panel())
                        .inner_margin(egui::Margin::symmetric(
                            theme.spacing_body_pad(),
                            theme.spacing_body_pad(),
                        ))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(300.0)
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    match self.page {
                                        HelpPage::QuickStart => render_quick_start(ui, theme),
                                        HelpPage::Shortcuts => {
                                            render_shortcuts(ui, theme, shortcuts_text)
                                        }
                                    }
                                });
                        });
                    ui.add_space(theme.spacing_md());
                    ui.horizontal(|ui| {
                        if chrome::modal_secondary_button(ui, theme, "打开完整说明").clicked()
                        {
                            match Self::open_markdown_in_system("product/FUNCTIONAL_SPEC.md") {
                                Ok(()) => {
                                    *status_message = "已在系统默认应用中打开说明文档".to_string();
                                }
                                Err(e) => *status_message = e,
                            }
                        }
                        if chrome::modal_secondary_button(ui, theme, "打开文档索引").clicked() {
                            match Self::open_markdown_in_system("README.md") {
                                Ok(()) => {
                                    *status_message = "已在系统默认应用中打开文档索引".to_string();
                                }
                                Err(e) => *status_message = e,
                            }
                        }
                    });
                    ui.add_space(theme.spacing_list_item_x());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if chrome::modal_secondary_button(ui, th, "关闭").clicked() {
                            should_close = true;
                        }
                    });
                });
            });
        self.open = open && !should_close;
    }
}

fn render_help_tabs(ui: &mut Ui, theme: &Theme, page: &mut HelpPage) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm();
        for tab in [HelpPage::QuickStart, HelpPage::Shortcuts] {
            let selected = *page == tab;
            let label = RichText::new(tab.label())
                .size(theme.font_size_connection_name())
                .color(if selected {
                    theme.text_primary()
                } else {
                    theme.color_form_hint()
                })
                .strong();
            if ui.selectable_label(selected, label).clicked() {
                *page = tab;
            }
        }
    });
}

fn render_quick_start(ui: &mut Ui, theme: &Theme) {
    ui.label(
        RichText::new("Mist")
            .size(theme.font_size_empty_state())
            .strong()
            .color(theme.text_primary()),
    );
    ui.label(
        RichText::new("SSH 终端 · 快速上手")
            .size(theme.font_size_panel_title())
            .color(theme.color_form_hint()),
    );
    ui.add_space(theme.spacing_lg());
    let steps = quick_steps();
    for (i, step) in steps.iter().enumerate() {
        render_step_row(ui, theme, i + 1, step);
        if i + 1 < steps.len() {
            ui.add_space(theme.spacing_md());
        }
    }
    ui.add_space(theme.spacing_lg());
    let docs_menu_hint = format!(
        "产品规格与详细设计在 docs/ 目录；也可通过菜单「{}」查看。",
        crate::platform::reveal_docs_folder_menu_hint()
    );
    render_tip_box(ui, theme, "完整说明", &docs_menu_hint);
}

fn render_step_row(ui: &mut Ui, theme: &Theme, index: usize, step: &QuickStep) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md();
        let size = egui::vec2(26.0, 26.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let center = rect.center();
        ui.painter().circle_filled(center, 13.0, theme.accent_a13());
        ui.painter().circle_stroke(center, 13.0, egui::Stroke::new(1.0, theme.accent_alpha(89)));
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("{index}"),
            FontId::proportional(theme.font_size_connection_name()),
            theme.accent_color(),
        );
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                RichText::new(step.title)
                    .size(theme.font_size_connection_name())
                    .strong()
                    .color(theme.text_primary()),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(step.detail)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
            if !step.keys.is_empty() {
                ui.add_space(theme.spacing_sm());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for key in &step.keys {
                        kbd_chip(ui, theme, key);
                    }
                });
            }
        });
    });
}

fn render_shortcuts(ui: &mut Ui, theme: &Theme, raw: &str) {
    let mut lines = raw.lines().map(str::trim).filter(|l| !l.is_empty());
    if let Some(intro) = lines.next() {
        ui.label(
            RichText::new(intro)
                .size(theme.font_size_panel_title())
                .italics()
                .color(theme.color_form_hint()),
        );
        ui.add_space(theme.spacing_md());
    }
    for line in lines {
        let Some((keys, desc)) = line.split_once('—').or_else(|| line.split_once('-')) else {
            ui.label(
                RichText::new(line)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
            continue;
        };
        let keys = keys.trim();
        let desc = desc.trim();
        ui.add_space(theme.spacing_sm());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_md();
            ui.allocate_ui_with_layout(
                egui::vec2(148.0, 0.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    for (i, part) in keys.split('/').map(str::trim).enumerate() {
                        if i > 0 {
                            ui.label(
                                RichText::new("/")
                                    .size(theme.font_size_panel_title())
                                    .color(theme.color_form_hint()),
                            );
                        }
                        kbd_chip(ui, theme, part);
                    }
                },
            );
            ui.label(
                RichText::new(desc)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_text_input_text()),
            );
        });
    }
}

fn kbd_chip(ui: &mut Ui, theme: &Theme, text: &str) {
    egui::Frame::none()
        .fill(theme.color_panel_toolbar_btn_fill())
        .stroke(egui::Stroke::new(1.0, theme.color_text_input_stroke()))
        .rounding(theme.radius_status_btn())
        .inner_margin(egui::Margin::symmetric(7.0, 3.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .font(FontId::monospace(theme.font_size_connection_name()))
                    .color(theme.text_primary()),
            );
        });
}

fn render_tip_box(ui: &mut Ui, theme: &Theme, title: &str, body: &str) {
    egui::Frame::none()
        .fill(theme.accent_a10())
        .stroke(egui::Stroke::new(1.0, theme.accent_alpha(48)))
        .rounding(theme.radius_list_item())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_search_input_x(),
            theme.spacing_search_input_y(),
        ))
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(theme.font_size_connection_name())
                    .strong()
                    .color(theme.accent_color()),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(body)
                    .size(theme.font_size_panel_title())
                    .color(theme.color_form_label()),
            );
        });
}
