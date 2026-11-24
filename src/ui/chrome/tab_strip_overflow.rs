//! #454 标签栏溢出布局：纯计算，不依赖 egui，便于独立单测。
//!
//! 标签芯片超出可用宽度时，预留左右滚动按钮和固定的「+」按钮，
//! 视口只显示一段内容，并把滚动偏移夹在合法范围内。

/// 容差：差 0.5px 仍视为能放下，避免字体取整把本来刚好的行打进溢出态。
const FIT_EPSILON: f32 = 0.5;

/// 标签栏溢出布局结果。
#[derive(Debug, Clone, PartialEq)]
pub struct TabStripOverflowPlan {
    /// 芯片总宽（含芯片之间的间距，不含「+」和滚动按钮）。
    pub content_width: f32,
    /// 芯片视口宽度。未溢出时等于 `content_width`。
    pub viewport_width: f32,
    /// 夹紧后的水平滚动偏移（内容坐标）。
    pub scroll_x: f32,
    /// 内容是否超出「芯片 + 加号」的可用宽度。
    pub overflow: bool,
    /// 还可以向左滚（更早的标签）。
    pub can_scroll_left: bool,
    /// 还可以向右滚（更晚的标签）。
    pub can_scroll_right: bool,
    /// 最大滚动偏移，`content_width - viewport_width`（未溢出时为 0）。
    pub max_scroll: f32,
}

/// 芯片行内容宽度：各芯片宽之和，加上相邻间距。
pub fn tab_strip_content_width(chip_widths: &[f32], item_spacing: f32) -> f32 {
    match chip_widths.len() {
        0 => 0.0,
        n => chip_widths.iter().copied().sum::<f32>() + item_spacing * (n as f32 - 1.0),
    }
}

/// 「芯片 + 加号」在当前可用宽度里要占多少（含芯片与加号之间的间距）。
fn needed_width_with_plus(
    content_width: f32,
    chip_count: usize,
    item_spacing: f32,
    plus_width: f32,
) -> f32 {
    if chip_count == 0 {
        plus_width
    } else {
        content_width + item_spacing + plus_width
    }
}

/// 溢出态预留的左右按钮 + 加号 + 三段间距。
fn overflow_chrome_width(item_spacing: f32, plus_width: f32, scroll_btn_width: f32) -> f32 {
    scroll_btn_width * 2.0 + plus_width + item_spacing * 3.0
}

/// 根据芯片宽度和可用宽度计算标签栏是否溢出，以及夹紧后的滚动位置。
pub fn plan_tab_strip_overflow(
    chip_widths: &[f32],
    item_spacing: f32,
    available_width: f32,
    plus_width: f32,
    scroll_btn_width: f32,
    requested_scroll_x: f32,
) -> TabStripOverflowPlan {
    let content_width = tab_strip_content_width(chip_widths, item_spacing);
    let needed = needed_width_with_plus(content_width, chip_widths.len(), item_spacing, plus_width);
    if needed <= available_width + FIT_EPSILON {
        return TabStripOverflowPlan {
            content_width,
            viewport_width: content_width,
            scroll_x: 0.0,
            overflow: false,
            can_scroll_left: false,
            can_scroll_right: false,
            max_scroll: 0.0,
        };
    }

    let chrome = overflow_chrome_width(item_spacing, plus_width, scroll_btn_width);
    let viewport_width = (available_width - chrome).max(0.0);
    let max_scroll = (content_width - viewport_width).max(0.0);
    let scroll_x = requested_scroll_x.clamp(0.0, max_scroll);
    TabStripOverflowPlan {
        content_width,
        viewport_width,
        scroll_x,
        overflow: true,
        can_scroll_left: scroll_x > 0.0,
        can_scroll_right: scroll_x + 0.5 < max_scroll,
        max_scroll,
    }
}

/// 第 `index` 个芯片在内容坐标中的 [left, right]。
pub fn tab_content_range(
    chip_widths: &[f32],
    item_spacing: f32,
    index: usize,
) -> Option<(f32, f32)> {
    if index >= chip_widths.len() {
        return None;
    }
    let mut left = 0.0;
    for (i, w) in chip_widths.iter().copied().enumerate() {
        if i == index {
            return Some((left, left + w));
        }
        left += w + item_spacing;
    }
    None
}

/// 把指定芯片完整滚进视口；若单芯片比视口更宽，则对齐其左缘。
pub fn scroll_tab_into_view(
    scroll_x: f32,
    tab_left: f32,
    tab_right: f32,
    viewport_width: f32,
    max_scroll: f32,
) -> f32 {
    let max_scroll = max_scroll.max(0.0);
    if viewport_width <= 0.0 {
        return scroll_x.clamp(0.0, max_scroll);
    }
    let tab_w = (tab_right - tab_left).max(0.0);
    let next = if tab_w >= viewport_width {
        tab_left
    } else if tab_left < scroll_x {
        tab_left
    } else if tab_right > scroll_x + viewport_width {
        tab_right - viewport_width
    } else {
        scroll_x
    };
    next.clamp(0.0, max_scroll)
}

/// 左右按钮的步长：约一个视口宽，至少能挪开一枚最小芯片。
pub fn tab_strip_scroll_step(viewport_width: f32) -> f32 {
    viewport_width.max(72.0) * 0.8
}

/// 把滚动偏移加上 `delta` 后再夹紧。
pub fn apply_tab_strip_scroll(scroll_x: f32, delta: f32, max_scroll: f32) -> f32 {
    (scroll_x + delta).clamp(0.0, max_scroll.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: f32 = 8.0;
    const PLUS: f32 = 22.0;
    const BTN: f32 = 22.0;

    fn widths(n: usize, w: f32) -> Vec<f32> {
        vec![w; n]
    }

    #[test]
    fn few_tabs_fit_without_overflow_or_scroll_buttons() {
        let chips = widths(3, 80.0);
        let content = tab_strip_content_width(&chips, GAP);
        assert_eq!(content, 80.0 * 3.0 + GAP * 2.0);
        let available = content + GAP + PLUS + 100.0;
        let plan = plan_tab_strip_overflow(&chips, GAP, available, PLUS, BTN, 40.0);
        assert!(!plan.overflow);
        assert_eq!(plan.viewport_width, content);
        assert_eq!(plan.scroll_x, 0.0);
        assert!(!plan.can_scroll_left);
        assert!(!plan.can_scroll_right);
        assert_eq!(plan.max_scroll, 0.0);
    }

    #[test]
    fn exact_fit_including_plus_is_not_overflow() {
        let chips = widths(2, 100.0);
        let needed = tab_strip_content_width(&chips, GAP) + GAP + PLUS;
        let plan = plan_tab_strip_overflow(&chips, GAP, needed, PLUS, BTN, 0.0);
        assert!(!plan.overflow);
        assert_eq!(plan.scroll_x, 0.0);
    }

    #[test]
    fn half_pixel_over_fit_epsilon_still_counts_as_fit() {
        let chips = widths(2, 100.0);
        let needed = tab_strip_content_width(&chips, GAP) + GAP + PLUS;
        let plan = plan_tab_strip_overflow(&chips, GAP, needed - 0.4, PLUS, BTN, 0.0);
        assert!(!plan.overflow);
    }

    #[test]
    fn overflowing_tabs_reserve_scroll_buttons_and_keep_plus() {
        let chips = widths(8, 90.0);
        let content = tab_strip_content_width(&chips, GAP);
        let available = 360.0;
        let plan = plan_tab_strip_overflow(&chips, GAP, available, PLUS, BTN, 0.0);
        assert!(plan.overflow);
        assert_eq!(plan.content_width, content);
        let chrome = BTN * 2.0 + PLUS + GAP * 3.0;
        assert!((plan.viewport_width - (available - chrome)).abs() < 0.01);
        assert!(plan.viewport_width < content);
        assert_eq!(plan.scroll_x, 0.0);
        assert!(!plan.can_scroll_left);
        assert!(plan.can_scroll_right);
        assert_eq!(plan.max_scroll, content - plan.viewport_width);
        // 加号不在视口里，预留在滚动按钮右侧。
        assert!(chrome + plan.viewport_width <= available + 0.01);
    }

    #[test]
    fn almost_fits_but_plus_would_be_clipped_is_overflow() {
        let chips = widths(3, 100.0);
        let content = tab_strip_content_width(&chips, GAP);
        // 只够芯片，不够加号。
        let plan = plan_tab_strip_overflow(&chips, GAP, content, PLUS, BTN, 0.0);
        assert!(plan.overflow);
        assert!(plan.can_scroll_right);
    }

    #[test]
    fn requested_scroll_is_clamped_to_max() {
        let chips = widths(10, 80.0);
        let plan = plan_tab_strip_overflow(&chips, GAP, 300.0, PLUS, BTN, 10_000.0);
        assert!(plan.overflow);
        assert_eq!(plan.scroll_x, plan.max_scroll);
        assert!(plan.can_scroll_left);
        assert!(!plan.can_scroll_right);
    }

    #[test]
    fn mid_scroll_enables_both_directions() {
        let chips = widths(10, 80.0);
        let plan = plan_tab_strip_overflow(&chips, GAP, 300.0, PLUS, BTN, 0.0);
        let mid = plan.max_scroll * 0.5;
        let plan = plan_tab_strip_overflow(&chips, GAP, 300.0, PLUS, BTN, mid);
        assert!(plan.can_scroll_left);
        assert!(plan.can_scroll_right);
        assert!((plan.scroll_x - mid).abs() < 0.01);
    }

    #[test]
    fn tab_content_range_matches_widths_and_gaps() {
        let chips = [70.0, 80.0, 90.0];
        assert_eq!(tab_content_range(&chips, GAP, 0), Some((0.0, 70.0)));
        assert_eq!(
            tab_content_range(&chips, GAP, 1),
            Some((70.0 + GAP, 70.0 + GAP + 80.0))
        );
        assert_eq!(
            tab_content_range(&chips, GAP, 2),
            Some((70.0 + GAP + 80.0 + GAP, 70.0 + GAP + 80.0 + GAP + 90.0))
        );
        assert_eq!(tab_content_range(&chips, GAP, 3), None);
        assert_eq!(tab_content_range(&chips, GAP, 99), None);
    }

    #[test]
    fn scroll_into_view_brings_right_edge_tab_on_screen() {
        let chips = widths(6, 100.0);
        let plan = plan_tab_strip_overflow(&chips, GAP, 280.0, PLUS, BTN, 0.0);
        let (left, right) = tab_content_range(&chips, GAP, 5).unwrap();
        // 末枚标签在初始偏移 0 时完全在视口外。
        assert!(right > plan.viewport_width);
        let scrolled = scroll_tab_into_view(0.0, left, right, plan.viewport_width, plan.max_scroll);
        assert!(scrolled > 0.0);
        assert!(left >= scrolled - 0.01);
        assert!(right <= scrolled + plan.viewport_width + 0.01);
        assert!(scrolled <= plan.max_scroll + 0.01);
    }

    #[test]
    fn scroll_into_view_leaves_already_visible_tab_alone() {
        let scrolled = scroll_tab_into_view(50.0, 60.0, 140.0, 200.0, 400.0);
        assert_eq!(scrolled, 50.0);
    }

    #[test]
    fn scroll_into_view_reveals_left_hidden_tab() {
        let scrolled = scroll_tab_into_view(200.0, 20.0, 100.0, 150.0, 400.0);
        assert_eq!(scrolled, 20.0);
    }

    #[test]
    fn scroll_into_view_wide_tab_aligns_left_edge() {
        let scrolled = scroll_tab_into_view(0.0, 80.0, 400.0, 100.0, 500.0);
        assert_eq!(scrolled, 80.0);
    }

    #[test]
    fn widening_window_clamps_previous_end_scroll() {
        let chips = widths(8, 90.0);
        let narrow = plan_tab_strip_overflow(&chips, GAP, 280.0, PLUS, BTN, 10_000.0);
        assert!(narrow.overflow);
        let wide = plan_tab_strip_overflow(&chips, GAP, 600.0, PLUS, BTN, narrow.scroll_x);
        assert!(wide.overflow);
        assert!(wide.max_scroll < narrow.max_scroll);
        assert_eq!(wide.scroll_x, wide.max_scroll);
    }

    #[test]
    fn shrinking_window_keeps_previous_scroll_if_still_valid() {
        let chips = widths(8, 90.0);
        let wide = plan_tab_strip_overflow(&chips, GAP, 600.0, PLUS, BTN, 40.0);
        let narrow = plan_tab_strip_overflow(&chips, GAP, 280.0, PLUS, BTN, wide.scroll_x);
        assert!(narrow.overflow);
        assert!(narrow.max_scroll > wide.max_scroll);
        assert_eq!(narrow.scroll_x, wide.scroll_x);
    }

    #[test]
    fn closing_tabs_until_they_fit_clears_overflow() {
        let many = widths(8, 90.0);
        let few = widths(2, 90.0);
        let available = 360.0;
        assert!(plan_tab_strip_overflow(&many, GAP, available, PLUS, BTN, 80.0).overflow);
        let after = plan_tab_strip_overflow(&few, GAP, available, PLUS, BTN, 80.0);
        assert!(!after.overflow);
        assert_eq!(after.scroll_x, 0.0);
    }

    #[test]
    fn empty_tab_list_has_no_overflow() {
        let plan = plan_tab_strip_overflow(&[], GAP, 400.0, PLUS, BTN, 12.0);
        assert!(!plan.overflow);
        assert_eq!(plan.content_width, 0.0);
        assert_eq!(plan.scroll_x, 0.0);
    }

    #[test]
    fn apply_scroll_and_step_move_then_clamp() {
        let step = tab_strip_scroll_step(100.0);
        assert!((step - 80.0).abs() < 0.01);
        assert_eq!(apply_tab_strip_scroll(0.0, step, 50.0), 50.0);
        assert_eq!(apply_tab_strip_scroll(10.0, -40.0, 50.0), 0.0);
        assert_eq!(apply_tab_strip_scroll(10.0, 15.0, 50.0), 25.0);
        // 很窄的视口仍至少按最小芯片宽度迈步。
        assert!((tab_strip_scroll_step(10.0) - 72.0 * 0.8).abs() < 0.01);
    }
}
