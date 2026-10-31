//! Tab 内终端分屏：树形嵌套(最多 8 窗格)，可关窗格、拖放换位。

use eframe::egui;

use crate::i18n;
use crate::ui::terminal::TerminalView;

pub const MAX_PANES_PER_TAB: usize = 8;
pub const NARROW_SPLIT_COLLAPSE_W: f32 = 280.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabLayout {
    #[default]
    Single,
    SplitHorizontal,
    SplitVertical,
}

#[derive(Debug, Clone)]
pub enum SplitNode {
    Pane(usize),
    Branch {
        horizontal: bool,
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    pub fn pane_leaf(idx: usize) -> Self {
        Self::Pane(idx)
    }

    fn replace_leaf(self, target: usize, horizontal: bool, new_idx: usize) -> Self {
        match self {
            Self::Pane(i) if i == target => Self::Branch {
                horizontal,
                ratio: 0.5,
                first: Box::new(Self::Pane(target)),
                second: Box::new(Self::Pane(new_idx)),
            },
            Self::Pane(i) => Self::Pane(i),
            // 字段名勿与参数 `horizontal` 同名，否则嵌套分屏会沿用父分支方向。
            Self::Branch {
                horizontal: branch_h,
                ratio,
                first,
                second,
            } => Self::Branch {
                horizontal: branch_h,
                ratio,
                first: Box::new(first.replace_leaf(target, horizontal, new_idx)),
                second: Box::new(second.replace_leaf(target, horizontal, new_idx)),
            },
        }
    }

    fn remove_leaf(self, target: usize) -> Self {
        match self {
            Self::Pane(i) => {
                debug_assert_ne!(i, target);
                Self::Pane(i)
            }
            Self::Branch {
                horizontal,
                ratio,
                first,
                second,
            } => {
                if let Self::Pane(i) = first.as_ref() {
                    if *i == target {
                        return *second;
                    }
                }
                if let Self::Pane(i) = second.as_ref() {
                    if *i == target {
                        return *first;
                    }
                }
                Self::Branch {
                    horizontal,
                    ratio,
                    first: Box::new(first.remove_leaf(target)),
                    second: Box::new(second.remove_leaf(target)),
                }
            }
        }
    }

    fn decrement_indices(&mut self, removed: usize) {
        match self {
            Self::Pane(i) => {
                if *i > removed {
                    *i -= 1;
                }
            }
            Self::Branch { first, second, .. } => {
                first.decrement_indices(removed);
                second.decrement_indices(removed);
            }
        }
    }

}

pub struct TerminalPane {
    pub session_id: String,
    pub title: String,
    pub terminal: TerminalView,
    pub ssh_auto_reconnect_next: Option<std::time::Instant>,
    pub ssh_auto_reconnect_attempts: u8,
    pub ssh_temp_key: Option<crate::core::TempKeyFile>,
    pub log_writer: Option<crate::core::SessionLogWriter>,
    pub last_term_rect: egui::Rect,
}

impl TerminalPane {
    pub fn new(session_id: String, title: String, terminal: TerminalView) -> Self {
        Self {
            session_id,
            title,
            terminal,
            ssh_auto_reconnect_next: None,
            ssh_auto_reconnect_attempts: 0,
            ssh_temp_key: None,
            log_writer: None,
            last_term_rect: egui::Rect::NOTHING,
        }
    }
}

pub struct TerminalTab {
    pub split_root: SplitNode,
    pub active_pane: usize,
    pub panes: Vec<TerminalPane>,
    /// 窗格标题拖放换位源索引
    pub drag_source_pane: Option<usize>,
    /// 开着时，当前窗格的键入会同时发到本标签的其它窗格。
    pub broadcast_input: bool,
    /// #93：未聚焦时 BEL / SessionEnded 聚合提示；聚焦该标签时清除。
    pub needs_attention: bool,
}

impl TerminalTab {
    pub fn single(pane: TerminalPane) -> Self {
        Self {
            split_root: SplitNode::Pane(0),
            active_pane: 0,
            panes: vec![pane],
            drag_source_pane: None,
            broadcast_input: false,
            needs_attention: false,
        }
    }

    pub fn clear_attention(&mut self) {
        self.needs_attention = false;
    }

    pub fn display_title(&self) -> String {
        if self.panes.is_empty() {
            return "Terminal".into();
        }
        if self.panes.len() == 1 {
            return self.panes[0].title.clone();
        }
        let head: Vec<_> = self.panes.iter().take(2).map(|p| p.title.as_str()).collect();
        let base = head.join(" | ");
        if self.panes.len() > 2 {
            format!("{base} +{}", self.panes.len() - 2)
        } else {
            base
        }
    }

    pub fn primary_session_id(&self) -> String {
        self.panes
            .first()
            .map(|p| p.session_id.clone())
            .unwrap_or_default()
    }

    pub fn can_split(&self) -> bool {
        self.panes.len() < MAX_PANES_PER_TAB
    }

    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }

    pub fn active_pane(&self) -> Option<&TerminalPane> {
        self.panes.get(self.active_pane)
    }

    pub fn active_pane_mut(&mut self) -> Option<&mut TerminalPane> {
        let i = self.active_pane.min(self.panes.len().saturating_sub(1));
        self.panes.get_mut(i)
    }

    pub fn active_terminal_mut(&mut self) -> Option<&mut TerminalView> {
        self.active_pane_mut().map(|p| &mut p.terminal)
    }

    pub fn active_terminal(&self) -> Option<&TerminalView> {
        self.active_pane().map(|p| &p.terminal)
    }

    pub fn any_connected(&self) -> bool {
        self.panes
            .iter()
            .any(|p| p.terminal.is_connected() || p.terminal.is_connecting())
    }

    pub fn any_connected_or_connecting(&self) -> bool {
        self.any_connected()
    }

    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut TerminalPane> {
        self.panes.iter_mut()
    }

    pub fn panes(&self) -> impl Iterator<Item = &TerminalPane> {
        self.panes.iter()
    }

    pub fn cycle_active_pane(&mut self) {
        if self.panes.len() <= 1 {
            return;
        }
        self.active_pane = (self.active_pane + 1) % self.panes.len();
    }

    pub fn focus_pane(&mut self, idx: usize) {
        if idx < self.panes.len() {
            self.active_pane = idx;
        }
    }

    pub fn swap_panes(&mut self, a: usize, b: usize) {
        if a < self.panes.len() && b < self.panes.len() && a != b {
            self.panes.swap(a, b);
        }
    }

    pub fn disconnect_all_panes(&mut self) {
        for p in &mut self.panes {
            p.terminal.disconnect();
        }
    }

    pub fn stop_all_logs(&mut self) {
        for p in &mut self.panes {
            if let Some(w) = p.log_writer.as_mut() {
                w.stop_log();
            }
        }
    }

    pub fn close_pane(&mut self, pane_idx: usize) -> bool {
        if self.panes.len() <= 1 || pane_idx >= self.panes.len() {
            return false;
        }
        let mut removed = self.panes.remove(pane_idx);
        removed.terminal.disconnect();
        let _ = removed.log_writer.take();
        self.split_root = self.split_root.clone().remove_leaf(pane_idx);
        self.split_root.decrement_indices(pane_idx);
        if self.active_pane >= self.panes.len() {
            self.active_pane = self.panes.len().saturating_sub(1);
        } else if pane_idx < self.active_pane {
            self.active_pane = self.active_pane.saturating_sub(1);
        }
        if self.panes.len() == 1 {
            self.split_root = SplitNode::Pane(0);
            self.active_pane = 0;
            self.broadcast_input = false;
        }
        true
    }

    pub fn unsplit_keep_active(&mut self) {
        if !self.is_split() {
            return;
        }
        let keep = self.active_pane.min(self.panes.len().saturating_sub(1));
        let kept = self.panes.remove(keep);
        for mut p in std::mem::take(&mut self.panes) {
            p.terminal.disconnect();
            let _ = p.log_writer.take();
        }
        self.panes = vec![kept];
        self.split_root = SplitNode::Pane(0);
        self.active_pane = 0;
        self.broadcast_input = false;
    }

    /// 只给当前焦点窗格挂上其它窗格的输入句柄。
    pub fn sync_broadcast_targets(&mut self) {
        if !self.broadcast_input || !self.is_split() {
            for pane in &mut self.panes {
                pane.terminal.clear_broadcast_targets();
            }
            if !self.is_split() {
                self.broadcast_input = false;
            }
            return;
        }
        let active = self.active_pane.min(self.panes.len().saturating_sub(1));
        let targets: Vec<_> = self
            .panes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != active)
            .filter_map(|(_, pane)| pane.terminal.clone_input_target())
            .collect();
        for (i, pane) in self.panes.iter_mut().enumerate() {
            if i == active {
                pane.terminal.set_broadcast_targets(targets.clone());
            } else {
                pane.terminal.clear_broadcast_targets();
            }
        }
    }

    pub fn add_pane_with_layout(&mut self, pane: TerminalPane, layout: TabLayout) {
        // Branch.horizontal=true → 左右并排；false → 上下堆叠。
        let side_by_side = layout == TabLayout::SplitHorizontal;
        let new_idx = self.panes.len();
        if new_idx == 0 {
            self.panes.push(pane);
            self.split_root = SplitNode::Pane(0);
            self.active_pane = 0;
            return;
        }
        let active = self.active_pane.min(new_idx.saturating_sub(1));
        self.panes.push(pane);
        self.split_root = self
            .split_root
            .clone()
            .replace_leaf(active, side_by_side, new_idx);
        self.active_pane = new_idx;
    }
}

#[cfg(test)]
mod split_layout_tests {
    use super::*;

    fn dummy_pane(id: &str) -> TerminalPane {
        TerminalPane::new(id.into(), id.into(), crate::ui::terminal::TerminalView::new())
    }

    #[test]
    fn left_right_split_sets_side_by_side_flag() {
        let mut tab = TerminalTab::single(dummy_pane("a"));
        tab.add_pane_with_layout(dummy_pane("b"), TabLayout::SplitHorizontal);
        match &tab.split_root {
            SplitNode::Branch { horizontal: true, .. } => {}
            other => panic!("左右分屏应得到 side-by-side 分支, got {other:?}"),
        }
    }

    #[test]
    fn top_bottom_split_sets_stacked_flag() {
        let mut tab = TerminalTab::single(dummy_pane("a"));
        tab.add_pane_with_layout(dummy_pane("b"), TabLayout::SplitVertical);
        match &tab.split_root {
            SplitNode::Branch {
                horizontal: false, ..
            } => {}
            other => panic!("上下分屏应得到 stacked 分支, got {other:?}"),
        }
    }

    #[test]
    fn nested_left_right_keeps_orientation() {
        let mut tab = TerminalTab::single(dummy_pane("a"));
        tab.add_pane_with_layout(dummy_pane("b"), TabLayout::SplitHorizontal);
        // 焦点在新窗格 b 上再左右分一次
        tab.add_pane_with_layout(dummy_pane("c"), TabLayout::SplitHorizontal);
        fn all_side_by_side(n: &SplitNode) -> bool {
            match n {
                SplitNode::Pane(_) => true,
                SplitNode::Branch {
                    horizontal,
                    first,
                    second,
                    ..
                } => *horizontal && all_side_by_side(first) && all_side_by_side(second),
            }
        }
        assert!(
            all_side_by_side(&tab.split_root),
            "连续左右分屏不应混入上下分支: {:?}",
            tab.split_root
        );
    }

    #[test]
    fn nested_vertical_after_horizontal_keeps_requested_direction() {
        let mut tab = TerminalTab::single(dummy_pane("a"));
        tab.add_pane_with_layout(dummy_pane("b"), TabLayout::SplitHorizontal);
        // 焦点在 b，再上下分 → b 应被替换为 stacked 分支，根仍左右
        tab.add_pane_with_layout(dummy_pane("c"), TabLayout::SplitVertical);
        match &tab.split_root {
            SplitNode::Branch {
                horizontal: true,
                first,
                second,
                ..
            } => {
                assert!(matches!(first.as_ref(), SplitNode::Pane(0)));
                match second.as_ref() {
                    SplitNode::Branch {
                        horizontal: false, ..
                    } => {}
                    other => panic!("嵌套上下分屏应在右枝得到 stacked, got {other:?}"),
                }
            }
            other => panic!("根应为左右分支, got {other:?}"),
        }
    }
}

pub fn render_split_body(
    ui: &mut egui::Ui,
    tab: &mut TerminalTab,
    theme: &crate::ui::theme::Theme,
    total_w: f32,
    body_h: f32,
    terminal_search_open: bool,
    mut pane_capture: impl FnMut(usize) -> bool,
    mut show_pane: impl FnMut(&mut egui::Ui, &mut TerminalView, f32, bool, bool),
    mut close_pane: impl FnMut(usize),
    mut swap_panes: impl FnMut(usize, usize),
) {
    if tab.panes.is_empty() {
        return;
    }
    let mut pane_focus: Option<usize> = None;
    let mut swap_req: Option<(usize, usize)> = None;
    if tab.panes.len() == 1 {
        render_one_pane(
            ui,
            tab,
            theme,
            0,
            total_w,
            body_h,
            terminal_search_open,
            &mut pane_capture,
            &mut show_pane,
            &mut close_pane,
            false,
            &mut swap_req,
        );
    } else {
        let splitter = theme.spacing_dock_gap().max(4.0);
        let placeholder = SplitNode::Pane(tab.active_pane.min(tab.panes.len().saturating_sub(1)));
        let mut root = std::mem::replace(&mut tab.split_root, placeholder);
        render_node(
            ui,
            tab,
            theme,
            &mut root,
            total_w,
            body_h,
            splitter,
            terminal_search_open,
            &mut pane_capture,
            &mut show_pane,
            &mut close_pane,
            &mut pane_focus,
            &mut swap_req,
        );
        tab.split_root = root;
    }
    if let Some((a, b)) = swap_req {
        swap_panes(a, b);
    }
    if let Some(i) = pane_focus {
        tab.focus_pane(i);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_node(
    ui: &mut egui::Ui,
    tab: &mut TerminalTab,
    theme: &crate::ui::theme::Theme,
    node: &mut SplitNode,
    w: f32,
    h: f32,
    splitter: f32,
    terminal_search_open: bool,
    pane_capture: &mut impl FnMut(usize) -> bool,
    show_pane: &mut impl FnMut(&mut egui::Ui, &mut TerminalView, f32, bool, bool),
    close_pane: &mut impl FnMut(usize),
    pane_focus: &mut Option<usize>,
    swap_req: &mut Option<(usize, usize)>,
) {
    match node {
        SplitNode::Pane(idx) => {
            render_one_pane(
                ui,
                tab,
                theme,
                *idx,
                w,
                h,
                terminal_search_open,
                pane_capture,
                show_pane,
                close_pane,
                true,
                swap_req,
            );
            // 勿在整窗格上 Sense::click：会盖住失败卡片「重试/编辑/关闭」等子控件。
            // 聚焦改由 render_one_pane 内对空白区的点击处理。
        }
        SplitNode::Branch {
            horizontal,
            ratio,
            first,
            second,
        } => {
            // 用绝对矩形摆窗格，避免 egui 流式布局把「左右」第二格挤到下一行。
            // horizontal=true：左右并排（竖分割条）；false：上下堆叠（横分割条）。
            let (area, _) =
                ui.allocate_exact_size(egui::vec2(w.max(1.0), h.max(1.0)), egui::Sense::hover());
            if *horizontal {
                let main = (w - splitter).max(1.0);
                let w1 = (main * *ratio).clamp(1.0, (main - 1.0).max(1.0));
                let w2 = (main - w1).max(1.0);
                let left = egui::Rect::from_min_size(area.min, egui::vec2(w1, h));
                let sep_rect = egui::Rect::from_min_size(
                    egui::pos2(area.min.x + w1, area.min.y),
                    egui::vec2(splitter, h),
                );
                let right = egui::Rect::from_min_size(
                    egui::pos2(area.min.x + w1 + splitter, area.min.y),
                    egui::vec2(w2, h),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                    ui.set_clip_rect(left.intersect(ui.clip_rect()));
                    ui.set_min_size(left.size());
                    ui.set_max_size(left.size());
                    render_node(
                        ui,
                        tab,
                        theme,
                        first,
                        w1,
                        h,
                        splitter,
                        terminal_search_open,
                        pane_capture,
                        show_pane,
                        close_pane,
                        pane_focus,
                        swap_req,
                    );
                });
                let sep = ui.interact(
                    sep_rect,
                    ui.id().with((
                        "split_sep_h",
                        area.min.x.to_bits(),
                        area.min.y.to_bits(),
                    )),
                    egui::Sense::drag(),
                );
                if sep.dragged() {
                    *ratio = (*ratio + sep.drag_delta().x / w.max(1.0)).clamp(0.12, 0.88);
                }
                ui.painter()
                    .rect_filled(sep_rect, 0.0, theme.bg_body_color());
                ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                    ui.set_clip_rect(right.intersect(ui.clip_rect()));
                    ui.set_min_size(right.size());
                    ui.set_max_size(right.size());
                    render_node(
                        ui,
                        tab,
                        theme,
                        second,
                        w2,
                        h,
                        splitter,
                        terminal_search_open,
                        pane_capture,
                        show_pane,
                        close_pane,
                        pane_focus,
                        swap_req,
                    );
                });
            } else {
                let main = (h - splitter).max(1.0);
                let h1 = (main * *ratio).clamp(1.0, (main - 1.0).max(1.0));
                let h2 = (main - h1).max(1.0);
                let top = egui::Rect::from_min_size(area.min, egui::vec2(w, h1));
                let sep_rect = egui::Rect::from_min_size(
                    egui::pos2(area.min.x, area.min.y + h1),
                    egui::vec2(w, splitter),
                );
                let bottom = egui::Rect::from_min_size(
                    egui::pos2(area.min.x, area.min.y + h1 + splitter),
                    egui::vec2(w, h2),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(top), |ui| {
                    ui.set_clip_rect(top.intersect(ui.clip_rect()));
                    ui.set_min_size(top.size());
                    ui.set_max_size(top.size());
                    render_node(
                        ui,
                        tab,
                        theme,
                        first,
                        w,
                        h1,
                        splitter,
                        terminal_search_open,
                        pane_capture,
                        show_pane,
                        close_pane,
                        pane_focus,
                        swap_req,
                    );
                });
                let sep = ui.interact(
                    sep_rect,
                    ui.id().with((
                        "split_sep_v",
                        area.min.x.to_bits(),
                        area.min.y.to_bits(),
                    )),
                    egui::Sense::drag(),
                );
                if sep.dragged() {
                    *ratio = (*ratio + sep.drag_delta().y / h.max(1.0)).clamp(0.12, 0.88);
                }
                ui.painter()
                    .rect_filled(sep_rect, 0.0, theme.bg_body_color());
                ui.scope_builder(egui::UiBuilder::new().max_rect(bottom), |ui| {
                    ui.set_clip_rect(bottom.intersect(ui.clip_rect()));
                    ui.set_min_size(bottom.size());
                    ui.set_max_size(bottom.size());
                    render_node(
                        ui,
                        tab,
                        theme,
                        second,
                        w,
                        h2,
                        splitter,
                        terminal_search_open,
                        pane_capture,
                        show_pane,
                        close_pane,
                        pane_focus,
                        swap_req,
                    );
                });
            }
        }
    }
}

fn render_one_pane(
    ui: &mut egui::Ui,
    tab: &mut TerminalTab,
    theme: &crate::ui::theme::Theme,
    pane_idx: usize,
    w: f32,
    h: f32,
    terminal_search_open: bool,
    pane_capture: &mut impl FnMut(usize) -> bool,
    show_pane: &mut impl FnMut(&mut egui::Ui, &mut TerminalView, f32, bool, bool),
    close_pane: &mut impl FnMut(usize),
    show_chrome: bool,
    swap_req: &mut Option<(usize, usize)>,
) {
    let Some(pane) = tab.panes.get_mut(pane_idx) else {
        return;
    };
    let title = pane.title.clone();
    let active = tab.active_pane == pane_idx;
    let broadcast = tab.broadcast_input;
    ui.push_id(("split_pane", pane_idx), |ui| {
    // 固定窗格矩形。用 allocate_ui_with_layout 时，终端内容一高过视口就会把父级撑出窗口。
    let (pane_rect, _) = ui.allocate_exact_size(
        egui::vec2(w.max(1.0), h.max(1.0)),
        egui::Sense::hover(),
    );
    let mut pane_ui = ui.new_child(egui::UiBuilder::new().max_rect(pane_rect).layout(egui::Layout::top_down(egui::Align::LEFT)));
    pane_ui.set_clip_rect(pane_rect.intersect(pane_ui.clip_rect()));
    let ui = &mut pane_ui;
    {
            if show_chrome {
                let header_h = theme.font_size_caption() + 8.0;
                let (header_rect, header_resp) =
                    ui.allocate_exact_size(egui::vec2(w, header_h), egui::Sense::click_and_drag());
                let color = if active {
                    theme.accent_color()
                } else {
                    theme.text_tertiary()
                };
                ui.painter().text(
                    header_rect.left_center() + egui::vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &title,
                    egui::FontId::proportional(theme.font_size_caption()),
                    color,
                );
                if broadcast {
                    ui.painter().text(
                        header_rect.right_center() + egui::vec2(-26.0, 0.0),
                        egui::Align2::RIGHT_CENTER,
                        i18n::tr(ui.ctx(), "Broadcast Input", "广播输入"),
                        egui::FontId::proportional(theme.font_size_caption()),
                        theme.accent_color(),
                    );
                }
                let close_rect = egui::Rect::from_min_size(
                    header_rect.right_top() + egui::vec2(-22.0, 2.0),
                    egui::vec2(20.0, header_h - 4.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
                    if crate::ui::chrome::chrome_small_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Close,
                    )
                    .on_hover_text(i18n::tr(
                        ui.ctx(),
                        "Close split pane",
                        "关闭分屏窗格",
                    ))
                    .clicked()
                    {
                        close_pane(pane_idx);
                    }
                });
                if header_resp.drag_started() {
                    tab.drag_source_pane = Some(pane_idx);
                }
                if ui.input(|i| i.pointer.any_released()) {
                    if let Some(src) = tab.drag_source_pane {
                        if src != pane_idx {
                            *swap_req = Some((src, pane_idx));
                        }
                        tab.drag_source_pane = None;
                    }
                }
                if header_resp.clicked() {
                    tab.focus_pane(pane_idx);
                }
            }
            if let Some(pane) = tab.panes.get_mut(pane_idx) {
                let cap = pane_capture(pane_idx);
                show_pane(ui, &mut pane.terminal, w, terminal_search_open, cap);
                pane.last_term_rect = ui.max_rect();
            }
            // 点空白聚焦；若本帧已有控件吃掉点击（失败卡片按钮等），不要抢。
            let pane_rect = ui.max_rect();
            let clicked_empty = ui.input(|i| {
                i.pointer.primary_released()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| pane_rect.contains(p))
            }) && !ui.ctx().egui_wants_pointer_input();
            if clicked_empty {
                tab.focus_pane(pane_idx);
            }
    }
    });
}
