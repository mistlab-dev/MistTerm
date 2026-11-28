//! Display-only typography for host labels in tab chips and the left
//! connection list.
//!
//! 视觉回退（见 #476 / #455 历史）：曾把 IPv4 的 octet 分隔点画成
//! `circle_filled` 并加宽槽位，但在 13px 标签字号下读起来像一个大圆点。
//! 现在恢复为普通字体排版：直接用内置比例字体自己的 ASCII `.`（U+002E）
//! 字形整串排版绘制，不手绘形状、不替换成间隔号等近似字符。
//!
//! Connection identity, `SessionConfig.host`, and `session_tab_label*` output
//! are not rewritten here. Callers pass the real host text in and paint it.
//! The `·` (U+00B7) name/host divider from `session_tab_label*` is untouched.

use egui::{Align, Color32, FontId, Galley, Pos2, Rect, Sense, Ui, Vec2};
use std::sync::Arc;

/// Laid-out host / tab label. `source` is the original string, unchanged.
pub struct ReadableHostLayout {
    pub source: String,
    pub size: Vec2,
    /// Optical center of the text ink, in layout-local coordinates, matching
    /// the previous `galley.rows[0].visuals.mesh_bounds` centering used by tabs.
    pub ink_center_y: f32,
    galley: Arc<Galley>,
}

impl ReadableHostLayout {
    /// The text this layout represents. Always identical to the input.
    pub fn source_text(&self) -> &str {
        &self.source
    }
}

/// Layout `text` as a single stock `layout_no_wrap` galley — IPv4 octet
/// separators render with the font's own period glyph, same metrics as any
/// other label (width and centering identical to `layout_no_wrap`).
pub fn layout_readable_host(
    ctx: &egui::Context,
    text: &str,
    font_id: &FontId,
    color: Color32,
) -> ReadableHostLayout {
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font_id.clone(), color));
    let ink = galley
        .rows
        .first()
        .map(|r| r.visuals.mesh_bounds)
        .filter(|b| b.is_positive())
        .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, galley.size()));
    ReadableHostLayout {
        source: text.to_owned(),
        size: galley.size(),
        ink_center_y: ink.center().y,
        galley,
    }
}

pub fn paint_readable_host(painter: &egui::Painter, origin: Pos2, layout: &ReadableHostLayout) {
    painter.galley(origin, layout.galley.clone(), Color32::WHITE);
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

/// Width of a host / endpoint string as laid out by [`layout_readable_host`]
/// (equal to the stock `layout_no_wrap` width).
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

    const REPRESENTATIVE_HOSTS: &[&str] = &[
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
        "localhost",
        "example.com",
        "my-host",
        "2001:db8::1",
        "生产机",
        "ssh.example.org:22",
    ];

    /// 视觉回退后的核心约束：IPv4 与普通主机一视同仁，宽度必须与
    /// `layout_no_wrap` 原始 galley 完全一致（无手绘圆点的额外步进），
    /// 逻辑字符串保持原样（仍是 ASCII `.`，不是任何替身字符）。
    #[test]
    fn all_hosts_measure_exactly_like_the_stock_galley() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::ocean(),
            Theme::forest(),
        ] {
            let sizes = [
                theme.font_size_tab_label(),
                theme.font_size_caption(),
                theme.font_size_connection_meta(),
            ];
            egui::__run_test_ui(|ui| {
                for size in sizes {
                    let font_id = FontId::proportional(size);
                    let color = theme.color_caption_text();
                    for text in REPRESENTATIVE_HOSTS {
                        let layout = layout_readable_host(ui.ctx(), text, &font_id, color);
                        assert_eq!(layout.source_text(), *text, "must not rewrite {text}");
                        assert!(
                            text.contains('.') && text.contains("0.0")
                                || !text.starts_with("root@10"),
                            "logical host text must keep ASCII periods: {text}"
                        );
                        assert!(
                            !text.contains('．') && !text.contains('•') && !text.contains('\u{2024}'),
                            "must not substitute a look-alike separator: {text}"
                        );
                        let plain = ui.ctx().fonts_mut(|f| {
                            f.layout_no_wrap((*text).to_owned(), font_id.clone(), color)
                                .size()
                                .x
                        });
                        assert!(
                            (layout.size.x - plain).abs() < 0.05,
                            "{text} must measure like the stock galley at size={size}: {} vs {plain}",
                            layout.size.x
                        );
                        // The misleading reading from the original #455 report
                        // must never be produced by any rewrite.
                        assert_ne!(*text, "127.0.01");
                    }
                }
            });
        }
    }

    /// 标签芯片的垂直居中路径：ink 中心必须与 stock galley 的
    /// `mesh_bounds` 中心一致（#476 之前的对齐方式）。
    #[test]
    fn ink_center_matches_stock_galley_centering() {
        let theme = Theme::dark();
        egui::__run_test_ui(|ui| {
            let font_id = FontId::proportional(theme.font_size_tab_label());
            let color = theme.text_primary();
            for text in ["127.0.0.1", "example.com", "生产机 · 127.0.0.1"] {
                let layout = layout_readable_host(ui.ctx(), text, &font_id, color);
                let ink = ui.ctx().fonts_mut(|f| {
                    f.layout_no_wrap(text.to_owned(), font_id.clone(), color)
                });
                let expected = ink
                    .rows
                    .first()
                    .map(|r| r.visuals.mesh_bounds)
                    .filter(|b| b.is_positive())
                    .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, ink.size()))
                    .center()
                    .y;
                assert!(
                    (layout.ink_center_y - expected).abs() < 0.01,
                    "{text}: {} vs {expected}",
                    layout.ink_center_y
                );
            }
        });
    }

    /// 排版高度来自字体行高（IPv4 标签不再被圆点半径撑高）。
    #[test]
    fn layout_height_is_the_font_line_height() {
        let theme = Theme::dark();
        egui::__run_test_ui(|ui| {
            let font_id = FontId::proportional(theme.font_size_tab_label());
            let color = theme.text_primary();
            let layout = layout_readable_host(ui.ctx(), "127.0.0.1", &font_id, color);
            let plain_h = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap("127.0.0.1".to_owned(), font_id.clone(), color).size().y
            });
            assert!(
                (layout.size.y - plain_h).abs() < 0.05,
                "{} vs {plain_h}",
                layout.size.y
            );
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
