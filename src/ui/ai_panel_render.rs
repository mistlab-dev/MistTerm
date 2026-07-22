use eframe::egui;

use crate::core::is_runnable_shell_command;
use crate::i18n;
use crate::ui::icons::IconId;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

pub(super) fn ai_message_side_padding(theme: &Theme) -> f32 {
    theme.spacing_md().max(12.0)
}

pub(super) fn ai_content_width(ui: &egui::Ui) -> f32 {
    let w = ui.available_width().max(120.0);
    let bar = ui.spacing().scroll_bar_width;
    (w - bar - 4.0).max(96.0)
}

pub(super) fn show_wrapped_user_text(ui: &mut egui::Ui, theme: &Theme, text: &str, width: f32) {
    let width = width.max(24.0);
    let font_size = theme.font_size_body();
    let text = user_text_with_soft_breaks(text, width, font_size);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        &text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(font_size),
            color: theme.text_primary(),
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts(|f| f.layout_job(job));
    let size = egui::vec2(width, galley.size().y.max(font_size * 1.45));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().galley(rect.min, galley);
}

pub(super) fn show_assistant_text(ui: &mut egui::Ui, theme: &Theme, text: &str, width: f32) {
    let width = width.max(24.0);
    let font_size = theme.font_size_body();
    let gap = theme.spacing_xs();
    let paragraph_gap = theme.spacing_sm().max(6.0);
    ui.set_max_width(width);

    let mut in_code = false;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(paragraph_gap);
            continue;
        }
        if in_code {
            continue;
        }

        let (line, strong, mono) = assistant_display_line(trimmed, in_code);
        if is_assistant_section_heading(&line) {
            ui.add_space(paragraph_gap);
            continue;
        }
        if is_command_like_line(&line) {
            // 命令已在下方“可执行命令”区域统一展示，正文里跳过，避免重复卡片。
            ui.add_space(paragraph_gap);
            continue;
        }
        let font = if mono {
            egui::FontId::monospace(theme.font_size_small())
        } else {
            egui::FontId::proportional(font_size)
        };
        let color = theme.text_primary();
        let wrap_font_size = if mono {
            theme.font_size_small()
        } else {
            font_size
        };
        for wrapped in wrap_text_for_units(&line, width, wrap_font_size) {
            let mut line_font = font.clone();
            if strong {
                line_font.size += 0.5;
            }
            paint_ai_text_line(ui, &wrapped, line_font, color, width);
            ui.add_space(gap);
        }
    }
}

fn break_shell_command_for_wrap(text: &str, width: f32, font_size: f32) -> String {
    let cols = (width / (font_size * 0.62)).floor() as usize;
    let cols = cols.clamp(6, 64);
    let mut out = String::with_capacity(text.len() + text.len() / cols.max(1));
    for (li, line) in text.lines().enumerate() {
        if li > 0 {
            out.push('\n');
        }
        let mut run = 0usize;
        for ch in line.chars() {
            out.push(ch);
            if ch.is_whitespace() || ch == '|' || ch == ';' || ch == '&' {
                run = 0;
                continue;
            }
            run += 1;
            if run >= cols {
                out.push('\u{200b}');
                run = 0;
            }
        }
    }
    out
}

fn paint_ai_text_line(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) {
    let width = width.max(24.0);
    let display = if font.family == egui::FontFamily::Monospace {
        break_shell_command_for_wrap(text, width, font.size)
    } else {
        text.to_string()
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        &display,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts(|f| f.layout_job(job));
    let height = galley
        .size()
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().with_clip_rect(rect).galley(rect.min, galley);
}

fn assistant_display_line(line: &str, in_code: bool) -> (String, bool, bool) {
    if in_code {
        return (line.to_string(), false, true);
    }
    let mut s = line.trim().to_string();
    let mut strong = false;
    if let Some(rest) = s.strip_prefix("* ") {
        s = format!("• {}", rest.trim());
    } else if let Some(rest) = s.strip_prefix("- ") {
        s = format!("• {}", rest.trim());
    }
    for prefix in ["### ", "## ", "# "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            strong = true;
            break;
        }
    }
    s = s.replace("**", "").replace("__", "").replace('`', "");
    (s, strong, false)
}

fn wrap_text_for_units(line: &str, width: f32, font_size: f32) -> Vec<String> {
    let max_units = (width / (font_size * 0.58)).floor().max(8.0);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut units = 0.0f32;

    for token in split_wrap_tokens(line) {
        let token_units = text_units(&token);
        if units + token_units > max_units && !cur.trim().is_empty() {
            out.push(cur.trim_end().to_string());
            cur.clear();
            units = 0.0;
        }
        if token_units > max_units && !is_protected_token(token.trim()) {
            for ch in token.chars() {
                let u = char_units(ch);
                if units + u > max_units && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    units = 0.0;
                }
                cur.push(ch);
                units += u;
            }
        } else {
            cur.push_str(&token);
            units += token_units;
        }
    }
    let cur = cur.trim_end();
    if !cur.is_empty() {
        out.push(cur.to_string());
    }
    out
}

fn split_wrap_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        cur.push(ch);
        if ch.is_whitespace()
            || matches!(
                ch,
                ',' | '，' | '。' | ':' | '：' | ';' | '；' | '/' | '|' | ')'
            )
        {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn text_units(text: &str) -> f32 {
    text.chars().map(char_units).sum()
}

fn char_units(ch: char) -> f32 {
    if ch.is_ascii() {
        0.56
    } else {
        1.0
    }
}

fn is_protected_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let dot_count = token.chars().filter(|&c| c == '.').count();
    if has_digit && dot_count >= 2 {
        return true;
    }
    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn is_assistant_section_heading(line: &str) -> bool {
    let s = line.trim().trim_end_matches([':', '：']).trim();
    matches!(
        s,
        "结论"
            | "关键点"
            | "风险"
            | "下一步"
            | "建议命令"
            | "可执行命令"
            | "Conclusion"
            | "Key points"
            | "Risks"
            | "Next steps"
            | "Suggested commands"
            | "Runnable commands"
    )
}

fn is_command_like_line(line: &str) -> bool {
    let s = line.trim();
    if s.starts_with('•') {
        return false;
    }
    let has_pipe = s.contains('|');
    let has_shell_tool = [
        " awk ", " sort ", " head ", " cut ", " ls ", " find ", " du ",
    ]
    .iter()
    .any(|needle| format!(" {s} ").contains(needle));
    has_pipe || has_shell_tool
}

fn user_text_with_soft_breaks(text: &str, width: f32, font_size: f32) -> String {
    let cols = (width / (font_size * 0.95)).floor() as usize;
    let cols = cols.clamp(8, 48);
    let mut out = String::with_capacity(text.len() + text.len() / cols.max(1));
    let mut run = 0usize;
    for ch in text.chars() {
        out.push(ch);
        if ch.is_whitespace() || ch == '-' || ch == '_' || ch == '/' || ch == '.' {
            run = 0;
            continue;
        }
        run += 1;
        if run >= cols {
            out.push('\u{200b}');
            run = 0;
        }
    }
    out
}

/// 子 Ui 占满**当前**可用行宽（勿把外层宽度传入 Frame/ScrollArea 内层，否则会左裁切）。
pub(super) fn bind_row_width(ui: &mut egui::Ui) -> f32 {
    let w = layout_util::set_width_to_available(ui);
    ui.set_width(w);
    w
}

pub(super) fn show_command_card(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    cmd: &str,
) -> bool {
    let mut clicked = false;
    if !is_runnable_shell_command(cmd) {
        return false;
    }
    let display = cmd
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if display.is_empty() {
        return false;
    }
    egui::Frame::none()
        .fill(theme.color_markdown_code_block_fill())
        .stroke(egui::Stroke::new(
            theme.hairline_width(ui.ctx()),
            theme.accent_alpha(90),
        ))
        .rounding(theme.radius_list_item())
        .inner_margin(egui::vec2(10.0, 8.0))
        .show(ui, |ui| {
            let row_w = ui.available_width().max(48.0);
            ui.set_max_width(row_w);
            ui.set_width(row_w);
            let wrapped = break_shell_command_for_wrap(&display, row_w, theme.font_size_small());
            paint_ai_text_line(
                ui,
                &wrapped,
                egui::FontId::monospace(theme.font_size_small()),
                theme.text_primary(),
                row_w,
            );
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                        ui,
                        theme,
                        IconId::TerminalPrompt,
                        i18n::tr(ctx, "Send", "发送"),
                        true,
                    )
                    .on_hover_text(i18n::tr(
                        ctx,
                        "Send this command to the terminal",
                        "发送该命令到终端",
                    ))
                    .clicked();
                });
            });
        });
    clicked
}
