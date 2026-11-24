//! Display-only typography for host labels in tab chips and the left
//! connection list (#455).
//!
//! IPv4 periods in the bundled proportional UI font are tiny at tab / caption
//! size, so `127.0.0.1` is easily read as `127.0.01`. This module keeps the
//! *logical* string unchanged (still ASCII `.` / U+002E, never a middot,
//! hyphen, fullwidth period, or any other stand-in) and only paints those
//! octet separators a little larger, with a little extra advance, so the four
//! octets stay visually distinct.
//!
//! Connection identity, `SessionConfig.host`, and `session_tab_label*` output
//! are not rewritten here. Callers pass the real host text in and paint it.

use egui::{Align, Color32, FontId, Galley, Pos2, Rect, Sense, Ui, Vec2};
use std::sync::Arc;

/// Radius of the painted IPv4 separator, relative to the label's font size.
/// Sized to read as a period, not a tab status bullet (those use ~2.5–3.0).
pub fn ipv4_dot_radius(font_size: f32) -> f32 {
    (font_size * 0.13).clamp(1.25, 1.75)
}

/// Extra advance added on *each* side of an IPv4 separator slot.
pub fn ipv4_dot_side_pad(font_size: f32) -> f32 {
    (font_size * 0.09).clamp(0.6, 1.2)
}

/// Horizontal slot reserved for one painted IPv4 separator.
pub fn ipv4_dot_slot_width(font_size: f32, period_advance: f32) -> f32 {
    let radius = ipv4_dot_radius(font_size);
    let pad = ipv4_dot_side_pad(font_size);
    (2.0 * radius + 2.0 * pad).max(period_advance + 2.0 * pad)
}

/// `'.'` with an ASCII digit on both sides: an IPv4 dotted-decimal separator.
/// Hostname dots (`example.com`) and the `·` name/host divider are left alone.
pub fn is_ipv4_octet_dot_at(chars: &[char], index: usize) -> bool {
    chars.get(index) == Some(&'.')
        && chars
            .get(index.wrapping_sub(1))
            .is_some_and(|c| c.is_ascii_digit())
        && chars
            .get(index + 1)
            .is_some_and(|c| c.is_ascii_digit())
}

/// Count of IPv4 octet separators in `text`. Does not rewrite `text`.
pub fn ipv4_octet_dot_count(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len())
        .filter(|&i| is_ipv4_octet_dot_at(&chars, i))
        .count()
}

#[derive(Clone)]
enum PreparedRun {
    Text {
        galley: Arc<Galley>,
        x: f32,
    },
    Ipv4Dot {
        center_x: f32,
        cy: f32,
        radius: f32,
        color: Color32,
    },
}

/// Laid-out host / tab label. `source` is the original string, unchanged.
pub struct ReadableHostLayout {
    pub source: String,
    pub size: Vec2,
    /// Optical center of the text ink, in layout-local coordinates, matching
    /// the previous `galley.rows[0].visuals.mesh_bounds` centering used by tabs.
    pub ink_center_y: f32,
    pub has_enhanced_dots: bool,
    runs: Vec<PreparedRun>,
}

impl ReadableHostLayout {
    /// The text this layout represents. Always identical to the input.
    pub fn source_text(&self) -> &str {
        &self.source
    }
}

/// Layout `text` with enhanced IPv4 separators when present. Non-IP labels
/// collapse to a single `layout_no_wrap` galley (same metrics as before).
pub fn layout_readable_host(
    ctx: &egui::Context,
    text: &str,
    font_id: &FontId,
    color: Color32,
) -> ReadableHostLayout {
    let chars: Vec<char> = text.chars().collect();
    let has_enhanced_dots = (0..chars.len()).any(|i| is_ipv4_octet_dot_at(&chars, i));
    if !has_enhanced_dots {
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font_id.clone(), color));
        let ink = galley
            .rows
            .first()
            .map(|r| r.visuals.mesh_bounds)
            .filter(|b| b.is_positive())
            .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, galley.size()));
        return ReadableHostLayout {
            source: text.to_owned(),
            size: galley.size(),
            ink_center_y: ink.center().y,
            has_enhanced_dots: false,
            runs: vec![PreparedRun::Text { galley, x: 0.0 }],
        };
    }

    let font_size = font_id.size;
    let radius = ipv4_dot_radius(font_size);
    let (period_advance, period_cy) = ctx.fonts_mut(|f| {
        let g = f.layout_no_wrap(".".to_owned(), font_id.clone(), color);
        let ink = g
            .rows
            .first()
            .map(|r| r.visuals.mesh_bounds)
            .filter(|b| b.is_positive())
            .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, g.size()));
        (g.size().x, ink.center().y)
    });
    let slot = ipv4_dot_slot_width(font_size, period_advance);

    let mut runs = Vec::new();
    let mut x = 0.0;
    let mut i = 0;
    let mut ink: Option<Rect> = None;
    let mut max_h = 0.0f32;

    while i < chars.len() {
        if is_ipv4_octet_dot_at(&chars, i) {
            runs.push(PreparedRun::Ipv4Dot {
                center_x: x + slot * 0.5,
                cy: period_cy,
                radius,
                color,
            });
            x += slot;
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < chars.len() && !is_ipv4_octet_dot_at(&chars, i) {
            i += 1;
        }
        let chunk: String = chars[start..i].iter().collect();
        let galley = ctx.fonts_mut(|f| f.layout_no_wrap(chunk, font_id.clone(), color));
        if let Some(row) = galley.rows.first() {
            let mut b = row.visuals.mesh_bounds;
            if b.is_positive() {
                b = b.translate(egui::vec2(x, 0.0));
                ink = Some(match ink {
                    Some(acc) => acc.union(b),
                    None => b,
                });
            }
        }
        max_h = max_h.max(galley.size().y);
        let w = galley.size().x;
        runs.push(PreparedRun::Text { galley, x });
        x += w;
    }

    let height = max_h.max(2.0 * radius + 2.0);
    let ink_rect = ink.unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::new(x, height)));
    ReadableHostLayout {
        source: text.to_owned(),
        size: Vec2::new(x, height),
        ink_center_y: ink_rect.center().y,
        has_enhanced_dots: true,
        runs,
    }
}

pub fn paint_readable_host(painter: &egui::Painter, origin: Pos2, layout: &ReadableHostLayout) {
    for run in &layout.runs {
        match run {
            PreparedRun::Text { galley, x } => {
                painter.galley(
                    Pos2::new(origin.x + *x, origin.y),
                    galley.clone(),
                    Color32::WHITE,
                );
            }
            PreparedRun::Ipv4Dot {
                center_x,
                cy,
                radius,
                color,
                ..
            } => {
                painter.circle_filled(
                    Pos2::new(origin.x + *center_x, origin.y + *cy),
                    *radius,
                    *color,
                );
            }
        }
    }
}

/// Single-line host / endpoint label for the left connection list.
/// Uses the available width and clips on overflow; does not wrap mid-address.
pub fn readable_host_line(
    ui: &mut Ui,
    text: &str,
    font_size: f32,
    color: Color32,
) -> egui::Response {
    let font_id = FontId::proportional(font_size);
    let layout = layout_readable_host(ui.ctx(), text, &font_id, color);
    let avail_w = ui.available_width();
    let size = Vec2::new(avail_w.max(0.0), layout.size.y);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let clip = ui.clip_rect().intersect(rect);
        let mut painter = ui.painter().clone();
        painter.set_clip_rect(clip);
        // Honor RTL rows (team-server endpoint column is right-aligned).
        let x = if ui.layout().horizontal_align() == Align::Max {
            (rect.max.x - layout.size.x).max(rect.min.x)
        } else {
            rect.min.x
        };
        paint_readable_host(&painter, Pos2::new(x, rect.min.y), &layout);
    }
    response
}

/// Width of a host / endpoint string after IPv4-dot enhancement (if any).
pub fn measure_readable_host_width(
    ctx: &egui::Context,
    text: &str,
    font_size: f32,
    color: Color32,
) -> f32 {
    let font_id = FontId::proportional(font_size);
    layout_readable_host(ctx, text, &font_id, color).size.x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    const REPRESENTATIVE_IPV4: &[&str] = &[
        "127.0.0.1",
        "127.0.0.1:22",
        "10.0.0.2",
        "192.168.1.1",
        "0.0.0.0",
        "255.255.255.255",
        "8.8.8.8",
        "user@127.0.0.1",
        "生产机 · 127.0.0.1",
        "root@10.0.0.5:2222",
    ];

    const NON_IPV4: &[&str] = &[
        "localhost",
        "example.com",
        "my-host",
        "2001:db8::1",
        "生产机",
        "ssh.example.org:22",
    ];

    #[test]
    fn representative_ipv4_strings_keep_ascii_dots_and_count_separators() {
        let cases: &[(&str, usize)] = &[
            ("127.0.0.1", 3),
            ("127.0.0.1:22", 3),
            ("10.0.0.2", 3),
            ("192.168.1.1", 3),
            ("0.0.0.0", 3),
            ("255.255.255.255", 3),
            ("8.8.8.8", 3),
            ("user@127.0.0.1", 3),
            ("生产机 · 127.0.0.1", 3),
            ("root@10.0.0.5:2222", 3),
        ];
        for (text, dots) in cases {
            assert_eq!(ipv4_octet_dot_count(text), *dots, "{text}");
            assert!(
                text.contains('.'),
                "logical host text must keep ASCII periods: {text}"
            );
            assert!(
                !text.contains('·') || text.contains(" · "),
                "must not invent a middot as an address separator: {text}"
            );
            assert!(
                !text.contains('．') && !text.contains('•') && !text.contains('\u{2024}'),
                "must not substitute a look-alike separator: {text}"
            );
            // The misleading reading from the bug report must not be produced.
            assert_ne!(*text, "127.0.01");
        }
    }

    #[test]
    fn non_ipv4_hosts_are_not_treated_as_dotted_decimal() {
        for text in NON_IPV4 {
            assert_eq!(ipv4_octet_dot_count(text), 0, "{text}");
        }
    }

    fn assert_layout_for_sizes(theme: &Theme) {
        let sizes = [
            theme.font_size_tab_label(),
            theme.font_size_caption(),
            theme.font_size_connection_meta(),
        ];
        egui::__run_test_ui(|ui| {
            for size in sizes {
                let font_id = FontId::proportional(size);
                let color = theme.color_caption_text();
                for text in REPRESENTATIVE_IPV4 {
                    let layout = layout_readable_host(ui.ctx(), text, &font_id, color);
                    assert_eq!(layout.source_text(), *text, "must not rewrite {text}");
                    assert!(layout.has_enhanced_dots, "{text}");
                    assert!(ipv4_dot_radius(size) >= 1.25);
                    let period = ui.ctx().fonts_mut(|f| {
                        f.layout_no_wrap(".".to_owned(), font_id.clone(), color)
                            .size()
                            .x
                    });
                    let slot = ipv4_dot_slot_width(size, period);
                    assert!(
                        slot > period,
                        "IPv4 slot must be wider than a raw period at size={size}: {slot} vs {period}"
                    );
                    let plain = ui.ctx().fonts_mut(|f| {
                        f.layout_no_wrap((*text).to_owned(), font_id.clone(), color)
                            .size()
                            .x
                    });
                    assert!(
                        layout.size.x + 0.01 >= plain,
                        "enhanced {text} must not shrink: {} vs plain {plain}",
                        layout.size.x
                    );
                    // Three separators should add measurable extra advance so
                    // adjacent octets (the `0.0` in 127.0.0.1) do not collapse.
                    assert!(
                        layout.size.x >= plain + 3.0 * ipv4_dot_side_pad(size) * 0.9,
                        "enhanced {text} should gain breathing room: {} vs {plain}",
                        layout.size.x
                    );
                }
                for text in NON_IPV4 {
                    let layout = layout_readable_host(ui.ctx(), text, &font_id, color);
                    assert_eq!(layout.source_text(), *text);
                    assert!(!layout.has_enhanced_dots, "{text}");
                    let plain = ui.ctx().fonts_mut(|f| {
                        f.layout_no_wrap((*text).to_owned(), font_id.clone(), color)
                            .size()
                            .x
                    });
                    assert!(
                        (layout.size.x - plain).abs() < 0.05,
                        "non-IP {text} width must match the stock galley: {} vs {plain}",
                        layout.size.x
                    );
                }
            }
        });
    }

    #[test]
    fn layout_and_metrics_smoke_on_all_themes() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            assert_layout_for_sizes(&theme);
        }
    }

    #[test]
    fn tab_chip_grows_for_ipv4_label_but_keeps_the_same_source_string() {
        let theme = Theme::dark();
        egui::__run_test_ui(|ui| {
            let label = "127.0.0.1";
            let font_id = FontId::proportional(theme.font_size_tab_label());
            let color = theme.text_primary();
            let layout = layout_readable_host(ui.ctx(), label, &font_id, color);
            let plain = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(label.to_owned(), font_id.clone(), color)
                    .size()
                    .x
            });
            assert_eq!(layout.source_text(), label);
            assert!(layout.has_enhanced_dots);
            assert!(layout.size.x > plain);
            // Layout used by session_tab_chip must remain a single line.
            assert!(layout.size.y > 0.0);
            assert!(layout.size.y < theme.size_tab_min_h() * 2.0);
        });
    }

    #[test]
    fn session_tab_label_text_is_still_the_real_host() {
        use crate::core::session::{session_tab_label_with, SessionConfig};
        let mut s = SessionConfig::default();
        s.name.clear();
        s.host = "127.0.0.1".into();
        s.port = 22;
        assert_eq!(session_tab_label_with(&s, true), "127.0.0.1");
        s.name = "生产机".into();
        assert_eq!(session_tab_label_with(&s, true), "生产机 · 127.0.0.1");
        assert_eq!(session_tab_label_with(&s, false), "生产机");
    }
}
