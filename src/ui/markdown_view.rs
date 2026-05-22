//! 将 Markdown 渲染为 egui 控件（AI 面板对话区等）。

use eframe::egui::{self, FontId, TextFormat, text::LayoutJob};
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::ui::layout_util;
use crate::ui::theme::Theme;

/// 渲染完整 Markdown 正文（不过滤围栏、标题、列表等）。
pub fn show_markdown(ui: &mut egui::Ui, theme: &Theme, markdown: &str) {
    if markdown.trim().is_empty() {
        return;
    }
    layout_util::set_width_to_available(ui);
    let mut r = MarkdownRenderer::new(theme);
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, opts);
    for event in parser {
        r.on_event(ui, event);
    }
    r.finish(ui);
}

struct MarkdownRenderer<'a> {
    theme: &'a Theme,
    inline: LayoutJob,
    body_format: TextFormat,
    code_format: TextFormat,
    strong: u32,
    emphasis: u32,
    code_block: Option<String>,
    code_block_lang: Option<String>,
    list_depth: u32,
    list_ordered: bool,
    list_index: u64,
    in_blockquote: bool,
    heading_buf: String,
    heading_level: Option<HeadingLevel>,
    link_url: Option<String>,
}

impl<'a> MarkdownRenderer<'a> {
    fn new(theme: &'a Theme) -> Self {
        let body_px = theme.font_size_body();
        let code_px = theme.font_size_small();
        Self {
            theme,
            inline: LayoutJob::default(),
            body_format: TextFormat {
                font_id: FontId::proportional(body_px),
                color: theme.text_primary(),
                ..Default::default()
            },
            code_format: TextFormat {
                font_id: FontId::monospace(code_px),
                color: theme.text_primary(),
                background: theme.color_subtle_inset_fill(),
                ..Default::default()
            },
            strong: 0,
            emphasis: 0,
            code_block: None,
            code_block_lang: None,
            list_depth: 0,
            list_ordered: false,
            list_index: 1,
            in_blockquote: false,
            heading_buf: String::new(),
            heading_level: None,
            link_url: None,
        }
    }

    fn on_event<'b>(&mut self, ui: &mut egui::Ui, event: Event<'b>) {
        match event {
            Event::Start(tag) => self.start_tag(ui, tag),
            Event::End(end) => self.end_tag(ui, end),
            Event::Text(text) => self.push_text(text.as_ref()),
            Event::Code(code) => self.push_code(code.as_ref()),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.flush_paragraph(ui),
            Event::Rule => {
                self.flush_paragraph(ui);
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            Event::Html(html) => self.push_text(html.as_ref()),
            _ => {}
        }
    }

    fn finish(&mut self, ui: &mut egui::Ui) {
        self.flush_paragraph(ui);
        if let Some(code) = self.code_block.take() {
            let lang = self.code_block_lang.take();
            paint_code_block(ui, self.theme, &code, lang.as_deref());
        }
    }

    fn start_tag(&mut self, ui: &mut egui::Ui, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_paragraph(ui);
                self.heading_level = Some(level);
                self.heading_buf.clear();
            }
            Tag::CodeBlock(kind) => {
                self.flush_paragraph(ui);
                self.code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let l = lang.as_ref().trim();
                        if l.is_empty() {
                            None
                        } else {
                            Some(l.to_string())
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code_block = Some(String::new());
            }
            Tag::List(start) => {
                self.flush_paragraph(ui);
                self.list_depth += 1;
                if self.list_depth == 1 {
                    self.list_ordered = start.is_some();
                    self.list_index = start.unwrap_or(1);
                }
            }
            Tag::Item => {
                self.flush_paragraph(ui);
                self.inject_list_prefix();
            }
            Tag::BlockQuote(_) => {
                self.flush_paragraph(ui);
                self.in_blockquote = true;
            }
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
            }
            Tag::Image {
                dest_url,
                title,
                ..
            } => {
                self.flush_paragraph(ui);
                let alt = title.as_ref().trim();
                let label = if alt.is_empty() {
                    format!("[图片]({dest_url})")
                } else {
                    format!("[{alt}]({dest_url})")
                };
                self.push_styled(&label, false, false, false);
                self.flush_paragraph(ui);
            }
            Tag::Table(_) => {
                self.flush_paragraph(ui);
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, ui: &mut egui::Ui, end: TagEnd) {
        match end {
            TagEnd::Paragraph => self.flush_paragraph(ui),
            TagEnd::Heading(level) => {
                let text = std::mem::take(&mut self.heading_buf);
                let size = match level {
                    HeadingLevel::H1 => self.theme.font_size_modal_title(),
                    HeadingLevel::H2 => self.theme.font_size_large(),
                    HeadingLevel::H3 => self.theme.font_size_medium(),
                    _ => self.theme.font_size_body(),
                };
                if !text.trim().is_empty() {
                    ui.label(
                        egui::RichText::new(text.trim())
                            .strong()
                            .size(size)
                            .color(self.theme.text_primary()),
                    );
                    ui.add_space(4.0);
                }
                self.heading_level = None;
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code_block.take() {
                    let lang = self.code_block_lang.take();
                    paint_code_block(ui, self.theme, &code, lang.as_deref());
                }
            }
            TagEnd::List(_) => {
                if self.list_depth > 0 {
                    self.list_depth -= 1;
                }
                if self.list_depth == 0 {
                    self.list_ordered = false;
                    self.list_index = 1;
                }
                ui.add_space(2.0);
            }
            TagEnd::Item => {
                self.flush_paragraph(ui);
                if self.list_ordered && self.list_depth >= 1 {
                    self.list_index += 1;
                }
            }
            TagEnd::BlockQuote => {
                self.in_blockquote = false;
                ui.add_space(4.0);
            }
            TagEnd::Emphasis => {
                self.emphasis = self.emphasis.saturating_sub(1);
            }
            TagEnd::Strong => {
                self.strong = self.strong.saturating_sub(1);
            }
            TagEnd::Link => {
                self.link_url = None;
            }
            TagEnd::Table => {
                self.flush_paragraph(ui);
                ui.add_space(4.0);
            }
            TagEnd::TableRow | TagEnd::TableHead => {
                self.push_text("  ");
            }
            TagEnd::TableCell => {
                self.push_text(" | ");
            }
            _ => {}
        }
    }

    fn inject_list_prefix(&mut self) {
        if self.list_depth == 0 {
            return;
        }
        let indent = "  ".repeat(self.list_depth.saturating_sub(1) as usize);
        let prefix = if self.list_ordered {
            format!("{indent}{}. ", self.list_index)
        } else {
            format!("{indent}• ")
        };
        self.inline
            .append(&prefix, 0.0, self.body_format.clone());
    }

    fn push_text(&mut self, text: &str) {
        if self.code_block.is_some() {
            if let Some(buf) = self.code_block.as_mut() {
                buf.push_str(text);
            }
            return;
        }
        if self.heading_level.is_some() {
            self.heading_buf.push_str(text);
            return;
        }
        self.push_styled(text, false, self.strong > 0, self.emphasis > 0);
    }

    fn push_code(&mut self, code: &str) {
        self.push_styled(code, true, false, false);
    }

    fn push_styled(&mut self, text: &str, inline_code: bool, strong: bool, emphasis: bool) {
        if text.is_empty() {
            return;
        }
        let mut fmt = if inline_code {
            self.code_format.clone()
        } else {
            self.body_format.clone()
        };
        if strong {
            fmt.font_id.size += 0.5;
        }
        if emphasis {
            fmt.italics = true;
        }
        if self.link_url.is_some() && !inline_code {
            fmt.color = self.theme.accent_color();
        }
        self.inline.append(text, 0.0, fmt);
    }

    fn flush_paragraph(&mut self, ui: &mut egui::Ui) {
        if self.inline.is_empty() {
            return;
        }
        let job = std::mem::take(&mut self.inline);
        self.inline = LayoutJob::default();
        let row_w = layout_util::set_width_to_available(ui);
        ui.set_max_width(row_w);
        if self.in_blockquote {
            egui::Frame::none()
                .fill(self.theme.color_subtle_inset_fill())
                .rounding(self.theme.radius_list_item())
                .inner_margin(egui::vec2(8.0, 6.0))
                .show(ui, |ui| {
                    let w = layout_util::set_width_to_available(ui);
                    ui.set_max_width(w);
                    ui.add(egui::Label::new(job).wrap(true));
                });
        } else {
            ui.add(egui::Label::new(job).wrap(true));
        }
        ui.add_space(4.0);
    }
}

fn paint_code_block(ui: &mut egui::Ui, theme: &Theme, code: &str, lang: Option<&str>) {
    egui::Frame::none()
        .fill(theme.bg_terminal_color())
        .rounding(theme.radius_list_item())
        .inner_margin(egui::vec2(8.0, 6.0))
        .show(ui, |ui| {
            let w = layout_util::set_width_to_available(ui);
            ui.set_max_width(w);
            if let Some(lang) = lang {
                ui.label(
                    egui::RichText::new(lang)
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                );
                ui.add_space(2.0);
            }
            ui.label(
                egui::RichText::new(code.trim_end())
                    .monospace()
                    .size(theme.font_size_small())
                    .color(theme.text_primary()),
            );
        });
    ui.add_space(4.0);
}
