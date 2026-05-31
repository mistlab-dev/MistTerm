//! Tab 内终端分屏：树形嵌套（最多 8 窗格），可关窗格、拖放换位。

use eframe::egui;

use crate::i18n;
use crate::ui::terminal::TerminalView;

pub const MAX_PANES_PER_TAB: usize = 8;
pub const NARROW_SPLIT_COLLAPSE_W: f32 = 480.0;

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
            Self::Branch {
                horizontal,
                ratio,
                first,
                second,
            } => Self::Branch {
                horizontal,
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
}

impl TerminalTab {
    pub fn single(pane: TerminalPane) -> Self {
        Self {
            split_root: SplitNode::Pane(0),
            active_pane: 0,
            panes: vec![pane],
            drag_source_pane: None,
        }
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
    }

    pub fn add_pane_with_layout(&mut self, pane: TerminalPane, layout: TabLayout) {
        let horizontal = layout == TabLayout::SplitHorizontal;
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
            .replace_leaf(active, horizontal, new_idx);
        self.active_pane = new_idx;
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
            if let Some(pane) = tab.panes.get(*idx) {
                let focus = ui.interact(
                    pane.last_term_rect,
                    ui.id().with(("pane_focus_tree", *idx)),
                    egui::Sense::click(),
                );
                if focus.clicked() {
                    *pane_focus = Some(*idx);
                }
            }
        }
        SplitNode::Branch {
            horizontal,
            ratio,
            first,
            second,
        } => {
            if *horizontal {
                let main = w - splitter;
                let w1 = main * *ratio;
                let w2 = main - w1;
                ui.horizontal(|ui| {
                    ui.set_max_width(w);
                    ui.set_min_height(h);
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
                    let (_id, sep) =
                        ui.allocate_exact_size(egui::vec2(splitter, h), egui::Sense::drag());
                    if sep.dragged() {
                        *ratio = (*ratio + sep.drag_delta().x / w.max(1.0)).clamp(0.12, 0.88);
                    }
                    ui.painter().rect_filled(sep.rect, 0.0, theme.bg_body_color());
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
                let main = h - splitter;
                let h1 = main * *ratio;
                let h2 = main - h1;
                ui.vertical(|ui| {
                    ui.set_max_width(w);
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
                    let (_id, sep) =
                        ui.allocate_exact_size(egui::vec2(w, splitter), egui::Sense::drag());
                    if sep.dragged() {
                        *ratio = (*ratio + sep.drag_delta().y / h.max(1.0)).clamp(0.12, 0.88);
                    }
                    ui.painter().rect_filled(sep.rect, 0.0, theme.bg_body_color());
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
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
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
                let close_rect = egui::Rect::from_min_size(
                    header_rect.right_top() + egui::vec2(-22.0, 2.0),
                    egui::vec2(20.0, header_h - 4.0),
                );
                if ui
                    .put(close_rect, egui::Button::new("×").small())
                    .on_hover_text(i18n::tr(
                        ui.ctx(),
                        "Close split pane",
                        "关闭分屏窗格",
                    ))
                    .clicked()
                {
                    close_pane(pane_idx);
                }
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
        },
    );
}
