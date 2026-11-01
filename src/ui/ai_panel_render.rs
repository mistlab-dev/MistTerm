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
    let bar = ui.spacing().scroll.bar_width;
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
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    // 短句按内容宽分配，避免「通栏蓝条」像按钮
    let content_w = galley.size().x.min(width).max(12.0);
    let size = egui::vec2(content_w, galley.size().y.max(font_size * 1.2));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().galley(rect.min, galley, egui::Color32::WHITE);
}

/// 按文案估算用户气泡外宽(含内边距)，上限约可用宽的 85%。
pub(super) fn estimate_user_bubble_outer_width(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    max_row_w: f32,
) -> f32 {
    let pad_x = 10.0;
    let font = egui::FontId::proportional(theme.font_size_body());
    let cap = (max_row_w * 0.85).max(64.0).min(max_row_w);
    let inner_cap = (cap - pad_x * 2.0).max(24.0);
    let soft = user_text_with_soft_breaks(text, inner_cap, theme.font_size_body());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = inner_cap;
    job.append(
        &soft,
        0.0,
        egui::TextFormat {
            font_id: font,
            color: theme.text_primary(),
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let content = galley.size().x.max(12.0);
    (content + pad_x * 2.0 + 2.0).clamp(48.0, cap)
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
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let height = galley
        .size()
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().with_clip_rect(rect).galley(rect.min, galley, egui::Color32::WHITE);
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
    // Models often title-case English headings ("Suggested Commands").
    let lower = s.to_ascii_lowercase();
    let s = if s.is_ascii() { lower.as_str() } else { s };
    matches!(
        s,
        "结论"
            | "关键点"
            | "风险"
            | "下一步"
            | "建议命令"
            | "可执行命令"
            | "conclusion"
            | "key points"
            | "risks"
            | "next steps"
            | "suggested commands"
            | "runnable commands"
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

/// 子 Ui 占满**当前**可用行宽(勿把外层宽度传入 Frame/ScrollArea 内层，否则会左裁切)。
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
    egui::Frame::NONE
        .fill(theme.color_markdown_code_block_fill())
        .stroke(egui::Stroke::new(
            theme.hairline_width(ui.ctx()),
            theme.accent_alpha(90),
        ))
        .corner_radius(theme.radius_list_item())
        .inner_margin(egui::vec2(10.0, 8.0))
        .show(ui, |ui| {
            let row_w = ui.available_width().max(48.0);
            ui.set_max_width(row_w);
            ui.set_width(row_w);
            // 整宽代码块 + 横向滚动，避免长命令被截断或强制折行难读（#249）。
            egui::ScrollArea::horizontal()
                .id_salt(("ai_cmd_card_scroll", egui::Id::new(&display)))
                .max_height(
                    (theme.font_size_small() * 1.35 * display.lines().count().max(1) as f32)
                        .clamp(theme.font_size_small() * 1.4, theme.font_size_small() * 8.0),
                )
                .show(ui, |ui| {
                    // 简单 shell 语法高亮：命令名蓝、参数灰、字符串绿、管道符黄。
                    // 用 horizontal（非 wrapped）保留横向滚动；parts 保留原文空格。
                    let parts = highlight_shell_command(&display, theme);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (text, color) in parts {
                            ui.label(
                                egui::RichText::new(text)
                                    .monospace()
                                    .size(theme.font_size_small())
                                    .color(color),
                            );
                        }
                    });
                });
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = crate::ui::chrome::panel_solid_primary_button_with_icon_ex(
                        ui,
                        theme,
                        IconId::TerminalPrompt,
                        i18n::tr(ctx, "Apply to terminal", "用到终端"),
                        true,
                    )
                    .on_hover_text(i18n::tr(
                        ctx,
                        "Put this command on the active terminal's input line without running it; check it, then press Enter",
                        "把这条命令放进当前终端的输入行，不会自动执行；检查后自己按回车",
                    ))
                    .clicked();
                });
            });
        });
    clicked
}

/// 简单 shell 命令语法高亮：命令名蓝、参数灰、字符串绿、管道符黄。
/// 按 shell 词法切分（保留引号内空格与原文分隔空白），避免 `echo "a b"` 显示失真。
fn highlight_shell_command(cmd: &str, theme: &Theme) -> Vec<(String, egui::Color32)> {
    let tokens = tokenize_shell_command(cmd);
    let mut parts = Vec::new();
    let mut prev_was_op = false;
    let mut saw_command = false;

    for (token, is_ws) in tokens {
        if is_ws {
            parts.push((token, theme.text_primary()));
            continue;
        }
        if token == "|" || token == "&&" || token == "||" || token == ";" {
            parts.push((token, egui::Color32::from_rgb(220, 180, 80)));
            prev_was_op = true;
            saw_command = false;
            continue;
        }
        if token.starts_with('\'') || token.starts_with('"') || token.starts_with('$') {
            parts.push((token, egui::Color32::from_rgb(80, 200, 120)));
            prev_was_op = false;
            continue;
        }
        if !saw_command || prev_was_op {
            parts.push((token, egui::Color32::from_rgb(80, 160, 240)));
            prev_was_op = false;
            saw_command = true;
            continue;
        }
        if token.starts_with('-') {
            parts.push((token, theme.text_tertiary()));
            continue;
        }
        parts.push((token, theme.text_primary()));
    }
    parts
}

/// `(text, is_whitespace)`；引号串作为整段，空白单独保留以便还原原文。
fn tokenize_shell_command(cmd: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            out.push((chars[start..i].iter().collect(), true));
            continue;
        }
        // 双字符运算符优先
        if i + 1 < chars.len() {
            let two: String = chars[i..i + 2].iter().collect();
            if two == "&&" || two == "||" {
                out.push((two, false));
                i += 2;
                continue;
            }
        }
        if chars[i] == '|' || chars[i] == ';' {
            out.push((chars[i].to_string(), false));
            i += 1;
            continue;
        }
        if chars[i] == '\'' || chars[i] == '"' {
            let quote = chars[i];
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                // 简单转义：\" 在双引号内跳过结束判定
                if quote == '"' && chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2;
                    continue;
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // 吞掉收尾引号
            }
            out.push((chars[start..i].iter().collect(), false));
            continue;
        }
        let start = i;
        while i < chars.len() {
            if chars[i].is_whitespace() || chars[i] == '|' || chars[i] == ';' {
                break;
            }
            if i + 1 < chars.len() && chars[i] == '&' && chars[i + 1] == '&' {
                break;
            }
            i += 1;
        }
        out.push((chars[start..i].iter().collect(), false));
    }
    out
}

#[cfg(test)]
mod highlight_tests {
    use super::tokenize_shell_command;

    #[test]
    fn tokenize_preserves_quoted_spaces_and_separators() {
        let parts = tokenize_shell_command(r#"echo "hello world" | wc -l"#);
        let joined: String = parts.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(joined, r#"echo "hello world" | wc -l"#);
        assert!(parts.iter().any(|(t, ws)| !ws && t == r#""hello world""#));
        assert!(parts.iter().any(|(t, ws)| !ws && t == "|"));
    }

    #[test]
    fn tokenize_keeps_and_or_ops() {
        let parts = tokenize_shell_command("true && echo ok || echo fail");
        let tokens: Vec<&str> = parts
            .iter()
            .filter(|(_, ws)| !ws)
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(tokens, vec!["true", "&&", "echo", "ok", "||", "echo", "fail"]);
    }
}
