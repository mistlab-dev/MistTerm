//! 将 Markdown 渲染为 egui 控件（AI 面板对话区等）。

use arboard::Clipboard;
use eframe::egui::{self, FontId, TextFormat, text::LayoutJob};
use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

/// 渲染完整 Markdown 正文（不过滤围栏、标题、列表等）。
/// `command_for_terminal`：点击 shell 代码块「执行」时写入待发送内容。
/// `bind_full_width`：用户气泡等窄容器内应传 `false`，避免 `set_width` 撑破导致左裁切。
pub fn show_markdown(
    ui: &mut egui::Ui,
    theme: &Theme,
    markdown: &str,
    command_for_terminal: &mut Option<String>,
    bind_full_width: bool,
) {
    if markdown.trim().is_empty() {
        return;
    }
    if bind_full_width {
        layout_util::set_width_to_available(ui);
    } else {
        let w = ui.available_width();
        if w.is_finite() && w > 1.0 {
            ui.set_max_width(w);
        }
    }
    let mut r = MarkdownRenderer::new(theme, command_for_terminal);
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(markdown, opts);
    for event in parser {
        r.on_event(ui, event);
    }
    r.finish(ui);
}

struct MarkdownRenderer<'a> {
    theme: &'a Theme,
    command_for_terminal: &'a mut Option<String>,
    code_block_serial: u32,
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
    fn new(theme: &'a Theme, command_for_terminal: &'a mut Option<String>) -> Self {
        let body_px = theme.font_size_body();
        let code_px = theme.font_size_small();
        Self {
            theme,
            command_for_terminal,
            code_block_serial: 0,
            inline: LayoutJob::default(),
            body_format: TextFormat {
                font_id: FontId::proportional(body_px),
                color: theme.text_primary(),
                line_height: Some(body_px * 1.3),
                ..Default::default()
            },
            code_format: TextFormat {
                font_id: FontId::monospace(code_px),
                color: theme.text_primary(),
                background: theme.color_subtle_inset_fill(),
                line_height: Some(code_px * 1.4),
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
                ui.add_space(1.0);
                ui.separator();
                ui.add_space(1.0);
            }
            Event::Html(html) => self.push_text(html.as_ref()),
            _ => {}
        }
    }

    fn finish(&mut self, ui: &mut egui::Ui) {
        self.flush_paragraph(ui);
        if let Some(code) = self.code_block.take() {
            let lang = self.code_block_lang.take();
            paint_code_block(
                ui,
                self.theme,
                &code,
                lang.as_deref(),
                self.command_for_terminal,
                &mut self.code_block_serial,
            );
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
                    ui.add_space(1.0);
                }
                self.heading_level = None;
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code_block.take() {
                    let lang = self.code_block_lang.take();
                    paint_code_block(
                        ui,
                        self.theme,
                        &code,
                        lang.as_deref(),
                        self.command_for_terminal,
                        &mut self.code_block_serial,
                    );
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
                ui.add_space(0.0);
            }
            TagEnd::Item => {
                self.flush_paragraph(ui);
                if self.list_ordered && self.list_depth >= 1 {
                    self.list_index += 1;
                }
            }
            TagEnd::BlockQuote => {
                self.in_blockquote = false;
                ui.add_space(1.0);
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
                ui.add_space(1.0);
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
        let mut job = std::mem::take(&mut self.inline);
        self.inline = LayoutJob::default();
        let row_w = layout_util::set_width_to_available(ui);
        ui.set_max_width(row_w);
        job.wrap.max_width = row_w;
        if self.in_blockquote {
            egui::Frame::none()
                .fill(self.theme.color_subtle_inset_fill())
                .rounding(self.theme.radius_list_item())
                .inner_margin(egui::vec2(8.0, 6.0))
                .show(ui, |ui| {
                    let _ = layout_util::set_width_to_available(ui);
                    ui.add(egui::Label::new(job.clone()).wrap(true));
                });
        } else {
            ui.add(egui::Label::new(job).wrap(true));
        }
        ui.add_space(1.0);
    }
}

fn paint_code_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    code: &str,
    lang: Option<&str>,
    command_for_terminal: &mut Option<String>,
    code_block_serial: &mut u32,
) {
    let code = code.trim_end();
    let code_lang = detect_code_lang(lang, code);
    let is_shell = matches!(code_lang, CodeLang::Shell);
    let block_id = egui::Id::new(("mistterm_md_codeblock", *code_block_serial));
    *code_block_serial = code_block_serial.saturating_add(1);

    egui::Frame::none()
        .fill(theme.color_text_input_fill())
        .stroke(theme.stroke_input())
        .rounding(theme.radius_list_item())
        .inner_margin(egui::vec2(8.0, 6.0))
        .show(ui, |ui| {
            let _ = layout_util::set_width_to_available(ui);
            if is_shell && !code.is_empty() {
                ui.push_id(block_id, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(l) = lang.map(str::trim).filter(|s| !s.is_empty()) {
                            ui.label(
                                egui::RichText::new(l)
                                    .monospace()
                                    .size(theme.font_size_small())
                                    .color(theme.color_form_hint()),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let rest = ui.available_width();
                            if rest.is_finite() && rest > 1.0 {
                                ui.set_min_width(rest);
                                ui.set_max_width(rest);
                            }
                            if chrome::chrome_small_accent_button(ui, theme, "执行")
                                .on_hover_text("发送到活动终端执行")
                                .clicked()
                            {
                                *command_for_terminal = Some(code.to_string());
                            }
                            if chrome::chrome_small_button(ui, theme, "复制")
                                .on_hover_text("复制代码到剪贴板")
                                .clicked()
                            {
                                if let Ok(mut clip) = Clipboard::new() {
                                    let _ = clip.set_text(code);
                                }
                            }
                        });
                    });
                    ui.add_space(theme.spacing_xs());
                });
            }
            let code_w = ui.available_width().max(1.0);
            let job = match code_lang {
                CodeLang::Shell => build_shell_code_layout_job(theme, code),
                CodeLang::JsonLike => build_json_like_layout_job(theme, code),
                CodeLang::Yaml => build_yaml_layout_job(theme, code),
                CodeLang::Sql => build_keyword_code_layout_job(
                    theme,
                    code,
                    &[
                        "select", "from", "where", "group", "by", "order", "limit", "offset",
                        "insert", "into", "values", "update", "set", "delete", "join", "left",
                        "right", "inner", "outer", "on", "as", "and", "or", "not", "null",
                        "is", "in", "exists", "having", "distinct", "union", "all", "create",
                        "table", "index", "alter", "drop", "case", "when", "then", "else", "end",
                    ],
                    &["--"],
                ),
                CodeLang::Python => build_keyword_code_layout_job(
                    theme,
                    code,
                    &[
                        "def", "class", "import", "from", "as", "if", "elif", "else", "for",
                        "while", "try", "except", "finally", "with", "return", "yield", "lambda",
                        "pass", "break", "continue", "in", "is", "and", "or", "not", "None",
                        "True", "False", "async", "await", "global", "nonlocal",
                    ],
                    &["#"],
                ),
                CodeLang::JsTs => build_keyword_code_layout_job(
                    theme,
                    code,
                    &[
                        "const", "let", "var", "function", "class", "import", "from", "export",
                        "default", "if", "else", "for", "while", "do", "switch", "case", "break",
                        "continue", "return", "try", "catch", "finally", "throw", "new", "this",
                        "async", "await", "null", "undefined", "true", "false", "typeof",
                        "instanceof", "interface", "type", "extends", "implements", "enum",
                    ],
                    &["//"],
                ),
                CodeLang::Plain => build_plain_code_layout_job(theme, code),
            };
            show_selectable_layout_job(ui, job, code_w);
        });
    ui.add_space(1.0);
}

fn show_selectable_layout_job(ui: &mut egui::Ui, job: LayoutJob, width: f32) {
    ui.set_max_width(width.max(1.0));
    // 用 Label 展示高亮 galley，避免 TextEdit+code_editor 在窄宽下左缘裁切。
    ui.add(egui::Label::new(job).wrap(false));
}

#[derive(Copy, Clone)]
enum CodeLang {
    Shell,
    JsonLike,
    Yaml,
    Sql,
    Python,
    JsTs,
    Plain,
}

fn detect_code_lang(lang: Option<&str>, code: &str) -> CodeLang {
    if let Some(l) = lang.map(|s| s.trim().to_ascii_lowercase()) {
        if !l.is_empty() {
            return match l.as_str() {
                "bash" | "sh" | "zsh" | "shell" | "console" => CodeLang::Shell,
                "json" | "jsonc" | "json5" => CodeLang::JsonLike,
                "yaml" | "yml" => CodeLang::Yaml,
                "sql" | "mysql" | "postgresql" | "postgres" | "sqlite" => CodeLang::Sql,
                "py" | "python" => CodeLang::Python,
                "js" | "javascript" | "ts" | "tsx" | "typescript" => CodeLang::JsTs,
                _ => CodeLang::Plain,
            };
        }
    }
    if code_has_shell_shebang(code) {
        CodeLang::Shell
    } else {
        CodeLang::Plain
    }
}

fn code_has_shell_shebang(code: &str) -> bool {
    code.lines().take(3).any(|line| {
        let t = line.trim();
        t.starts_with("#!/bin/bash")
            || t.starts_with("#!/bin/sh")
            || t.starts_with("#!/usr/bin/env bash")
            || t.starts_with("#!/usr/bin/env sh")
            || t.starts_with("#!/usr/bin/env zsh")
    })
}

fn build_json_like_layout_job(theme: &Theme, code: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let default_fmt = base_code_format(theme);
    let mut key_fmt = default_fmt.clone();
    key_fmt.color = theme.accent_color();
    let mut string_fmt = default_fmt.clone();
    string_fmt.color = theme.green_color();
    let mut number_fmt = default_fmt.clone();
    number_fmt.color = theme.amber_color();
    let mut punct_fmt = default_fmt.clone();
    punct_fmt.color = theme.text_secondary();

    let lines: Vec<&str> = code.lines().collect();
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];
            if ch == '"' {
                let mut j = i + 1;
                while j < chars.len() {
                    if chars[j] == '"' && chars[j - 1] != '\\' {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                let text: String = chars[i..j.min(chars.len())].iter().collect();
                let mut k = j;
                while k < chars.len() && chars[k].is_whitespace() {
                    k += 1;
                }
                let fmt = if k < chars.len() && chars[k] == ':' {
                    key_fmt.clone()
                } else {
                    string_fmt.clone()
                };
                job.append(&text, 0.0, fmt);
                i = j.min(chars.len());
                continue;
            }

            if ch.is_ascii_digit() || (ch == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_ascii_digit() || matches!(chars[j], '.' | 'e' | 'E' | '+' | '-'))
                {
                    j += 1;
                }
                let text: String = chars[i..j].iter().collect();
                job.append(&text, 0.0, number_fmt.clone());
                i = j;
                continue;
            }

            if ch.is_ascii_alphabetic() {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let fmt = if matches!(word.as_str(), "true" | "false" | "null") {
                    key_fmt.clone()
                } else {
                    default_fmt.clone()
                };
                job.append(&word, 0.0, fmt);
                i = j;
                continue;
            }

            let fmt = if matches!(ch, '{' | '}' | '[' | ']' | ':' | ',') {
                punct_fmt.clone()
            } else {
                default_fmt.clone()
            };
            job.append(&ch.to_string(), 0.0, fmt);
            i += 1;
        }
        if li + 1 < lines.len() {
            job.append("\n", 0.0, default_fmt.clone());
        }
    }
    job
}

fn build_yaml_layout_job(theme: &Theme, code: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let default_fmt = base_code_format(theme);
    let mut key_fmt = default_fmt.clone();
    key_fmt.color = theme.accent_color();
    let mut string_fmt = default_fmt.clone();
    string_fmt.color = theme.green_color();
    let mut number_fmt = default_fmt.clone();
    number_fmt.color = theme.amber_color();
    let mut comment_fmt = default_fmt.clone();
    comment_fmt.color = theme.color_form_hint();

    let lines: Vec<&str> = code.lines().collect();
    for (li, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(idx) = trimmed.find('#') {
            let (head, comment) = trimmed.split_at(idx);
            paint_yaml_head(&mut job, &default_fmt, &key_fmt, &string_fmt, &number_fmt, head);
            job.append(comment, 0.0, comment_fmt.clone());
        } else {
            paint_yaml_head(&mut job, &default_fmt, &key_fmt, &string_fmt, &number_fmt, trimmed);
        }
        if li + 1 < lines.len() {
            job.append("\n", 0.0, default_fmt.clone());
        }
    }
    job
}

fn paint_yaml_head(
    job: &mut LayoutJob,
    default_fmt: &TextFormat,
    key_fmt: &TextFormat,
    string_fmt: &TextFormat,
    number_fmt: &TextFormat,
    line: &str,
) {
    if line.is_empty() {
        return;
    }
    if let Some(idx) = line.find(':') {
        let (k, rest) = line.split_at(idx);
        if !k.trim().is_empty() {
            job.append(k, 0.0, key_fmt.clone());
            job.append(":", 0.0, default_fmt.clone());
            paint_simple_tail(job, default_fmt, string_fmt, number_fmt, &rest[1..]);
            return;
        }
    }
    paint_simple_tail(job, default_fmt, string_fmt, number_fmt, line);
}

fn paint_simple_tail(
    job: &mut LayoutJob,
    default_fmt: &TextFormat,
    string_fmt: &TextFormat,
    number_fmt: &TextFormat,
    text: &str,
) {
    for token in text.split_inclusive(char::is_whitespace) {
        let bare = token.trim();
        if bare.is_empty() {
            job.append(token, 0.0, default_fmt.clone());
        } else if (bare.starts_with('"') && bare.ends_with('"'))
            || (bare.starts_with('\'') && bare.ends_with('\''))
        {
            job.append(token, 0.0, string_fmt.clone());
        } else if bare.parse::<f64>().is_ok() || matches!(bare, "true" | "false" | "null" | "~") {
            job.append(token, 0.0, number_fmt.clone());
        } else {
            job.append(token, 0.0, default_fmt.clone());
        }
    }
}

fn build_keyword_code_layout_job(
    theme: &Theme,
    code: &str,
    keywords: &[&str],
    line_comments: &[&str],
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let default_fmt = base_code_format(theme);
    let mut keyword_fmt = default_fmt.clone();
    keyword_fmt.color = theme.accent_color();
    let mut string_fmt = default_fmt.clone();
    string_fmt.color = theme.green_color();
    let mut number_fmt = default_fmt.clone();
    number_fmt.color = theme.amber_color();
    let mut comment_fmt = default_fmt.clone();
    comment_fmt.color = theme.color_form_hint();
    let mut punct_fmt = default_fmt.clone();
    punct_fmt.color = theme.text_secondary();

    let lines: Vec<&str> = code.lines().collect();
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let rest: String = chars[i..].iter().collect();
            if line_comments.iter().any(|m| rest.starts_with(m)) {
                job.append(&rest, 0.0, comment_fmt.clone());
                break;
            }

            let ch = chars[i];
            if matches!(ch, '"' | '\'' | '`') {
                let quote = ch;
                let mut j = i + 1;
                while j < chars.len() {
                    if chars[j] == quote && chars[j - 1] != '\\' {
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                let text: String = chars[i..j.min(chars.len())].iter().collect();
                job.append(&text, 0.0, string_fmt.clone());
                i = j.min(chars.len());
                continue;
            }

            if ch.is_ascii_digit() || (ch == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_ascii_digit() || matches!(chars[j], '.' | 'e' | 'E' | '+' | '-'))
                {
                    j += 1;
                }
                let text: String = chars[i..j].iter().collect();
                job.append(&text, 0.0, number_fmt.clone());
                i = j;
                continue;
            }

            if ch.is_ascii_alphabetic() || ch == '_' {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let word: String = chars[i..j].iter().collect();
                let lower = word.to_ascii_lowercase();
                if keywords.contains(&lower.as_str()) {
                    job.append(&word, 0.0, keyword_fmt.clone());
                } else {
                    job.append(&word, 0.0, default_fmt.clone());
                }
                i = j;
                continue;
            }

            let fmt = if matches!(ch, '{' | '}' | '[' | ']' | '(' | ')' | ':' | ',' | ';') {
                punct_fmt.clone()
            } else {
                default_fmt.clone()
            };
            job.append(&ch.to_string(), 0.0, fmt);
            i += 1;
        }
        if li + 1 < lines.len() {
            job.append("\n", 0.0, default_fmt.clone());
        }
    }
    job
}

fn base_code_format(theme: &Theme) -> TextFormat {
    TextFormat {
        font_id: FontId::monospace(theme.font_size_small()),
        color: theme.color_text_input_text(),
        line_height: Some(theme.font_size_small() * 1.4),
        ..Default::default()
    }
}

fn build_plain_code_layout_job(theme: &Theme, code: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let fmt = base_code_format(theme);
    for (idx, line) in code.lines().enumerate() {
        job.append(line, 0.0, fmt.clone());
        if idx + 1 < code.lines().count() {
            job.append("\n", 0.0, fmt.clone());
        }
    }
    job
}

fn build_shell_code_layout_job(theme: &Theme, code: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let default_fmt = base_code_format(theme);
    let mut keyword_fmt = default_fmt.clone();
    keyword_fmt.color = theme.accent_color();
    let mut string_fmt = default_fmt.clone();
    string_fmt.color = theme.green_color();
    let mut comment_fmt = default_fmt.clone();
    comment_fmt.color = theme.color_form_hint();
    let mut var_fmt = default_fmt.clone();
    var_fmt.color = theme.amber_color();
    let mut flag_fmt = default_fmt.clone();
    flag_fmt.color = theme.text_secondary();

    let keywords = [
        "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until", "case",
        "esac", "function", "select", "time", "coproc", "return", "break", "continue", "local",
        "export", "readonly", "unset", "alias", "source", "eval", "exec",
    ];

    let lines: Vec<&str> = code.lines().collect();
    for (line_i, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let ch = chars[i];

            // Comment
            if ch == '#' {
                let text: String = chars[i..].iter().collect();
                job.append(&text, 0.0, comment_fmt.clone());
                i = chars.len();
                continue;
            }

            // String
            if ch == '"' || ch == '\'' {
                let quote = ch;
                let mut j = i + 1;
                while j < chars.len() {
                    if chars[j] == quote {
                        j += 1;
                        break;
                    }
                    if quote == '"' && chars[j] == '\\' && j + 1 < chars.len() {
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                let text: String = chars[i..j.min(chars.len())].iter().collect();
                job.append(&text, 0.0, string_fmt.clone());
                i = j.min(chars.len());
                continue;
            }

            // Variable
            if ch == '$' {
                let mut j = i + 1;
                if j < chars.len() && chars[j] == '{' {
                    j += 1;
                    while j < chars.len() && chars[j] != '}' {
                        j += 1;
                    }
                    if j < chars.len() {
                        j += 1;
                    }
                } else {
                    while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                        j += 1;
                    }
                }
                let text: String = chars[i..j.min(chars.len())].iter().collect();
                job.append(&text, 0.0, var_fmt.clone());
                i = j.min(chars.len());
                continue;
            }

            // Word/flag
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' || ch == '/' {
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_ascii_alphanumeric()
                        || matches!(chars[j], '_' | '-' | '.' | '/' | ':'))
                {
                    j += 1;
                }
                let token: String = chars[i..j].iter().collect();
                let fmt = if keywords.contains(&token.as_str()) {
                    keyword_fmt.clone()
                } else if token.starts_with('-') {
                    flag_fmt.clone()
                } else {
                    default_fmt.clone()
                };
                job.append(&token, 0.0, fmt);
                i = j;
                continue;
            }

            // Operators and whitespace fallback
            job.append(&ch.to_string(), 0.0, default_fmt.clone());
            i += 1;
        }
        if line_i + 1 < lines.len() {
            job.append("\n", 0.0, default_fmt.clone());
        }
    }
    job
}
