//! 失焦时的统一命令面板（#162）。
//!
//! 组合键（写进关于页）：终端未聚焦时，macOS 为 ⌘K，Windows / Linux 为 Ctrl+K。
//! 失焦的 Ctrl+K 与系统快捷键不冲突，因此不用 Ctrl+Shift+K。
//! 终端聚焦时 Ctrl+K 不进本面板，仍是 shell 删到行尾（#82）。
//! 面板打开后是阻塞弹窗，按键不得再进 PTY。

use crate::ui::theme::Theme;
use eframe::egui::{self, Key, Modifiers};

/// 关于页 / 手册共用的英文说明。须同时写出真实组合键，以及「不用 Ctrl+Shift+K」的原因。
pub const PALETTE_SHORTCUT_HELP_EN: &str = "Command palette when the terminal is not focused (macOS ⌘K, Windows/Linux Ctrl+K). Unfocused Ctrl+K does not conflict with the OS, so Ctrl+Shift+K is not used. Focused Ctrl+K goes to the shell and deletes to the end of the line";

/// 关于页 / 手册共用的中文说明。
pub const PALETTE_SHORTCUT_HELP_ZH: &str = "终端失焦时打开命令面板 (macOS 为 ⌘K, Windows/Linux 为 Ctrl+K). 失焦 Ctrl+K 与系统快捷键不冲突, 因此不用 Ctrl+Shift+K. 聚焦时 Ctrl+K 发给 shell, 删到行尾";

const EMPTY_CAP_ACTION: usize = 24;
const EMPTY_CAP_OTHER: usize = 8;
const QUERY_CAP_PER_KIND: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    Session,
    Snippet,
    /// 命名发送序列。和片段分开列出，避免和「执行片段」混在一类里。
    Sequence,
    History,
    Action,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    NewSession,
    NewTab,
    LocalShell,
    Sftp,
    Monitor,
    Ai,
    PortForward,
    Preferences,
    CommandHistory,
    SnippetLibrary,
    SnippetPanel,
    QuickSnippets,
    FindInTerminal,
    About,
    Immersive,
    Credentials,
    CloudSync,
    Disconnect,
    Reconnect,
    BatchExec,
    /// 打开发送序列管理，不执行片段。
    SendSequences,
    /// 工具 → 自动化规则（输出触发等）。
    RuleManager,
}

/// 已有标签则聚焦，否则连接。与侧栏 `select_session` 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionLaunch {
    FocusOpenTab,
    Connect,
}

pub fn session_launch(already_open: bool) -> SessionLaunch {
    if already_open {
        SessionLaunch::FocusOpenTab
    } else {
        SessionLaunch::Connect
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteId {
    Session(String),
    Snippet(String),
    /// 在当前已连接窗格按行重放。不填变量，也不当成片段执行。
    Sequence(String),
    /// 完整命令。选中后填入终端输入行，不自动回车。
    History(String),
    Action(PaletteAction),
}

#[derive(Clone, Debug)]
pub struct PaletteItem {
    pub id: PaletteId,
    pub kind: PaletteKind,
    pub title: String,
    pub detail: String,
    /// 模糊匹配用，中英关键词都放这里。
    pub search: String,
}

#[derive(Debug)]
pub struct CommandPalette {
    pub open: bool,
    query: String,
    selected: usize,
    focus_search: bool,
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            focus_search: false,
        }
    }
}

impl CommandPalette {
    pub fn search_id() -> egui::Id {
        egui::Id::new("mistterm_command_palette_search")
    }

    pub fn open_fresh(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_search = true;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        items: &[PaletteItem],
    ) -> PaletteOutcome {
        if !self.open {
            return PaletteOutcome::Idle;
        }
        let nav = ctx.input_mut(|i| take_palette_nav(i));
        let sz = crate::ui::layout_util::centered_window_default_size(ctx, 0.46, 0.56);
        let sz_v = egui::vec2(sz[0], sz[1]);
        let scroll_h = crate::ui::layout_util::dialog_scroll_max_height(ctx, 168.0);
        let shown = crate::ui::chrome::modal_window("mistterm_command_palette", theme, ctx)
            .movable(true)
            .resizable(true)
            .default_pos(crate::ui::layout_util::modal_center_pos(ctx, sz_v))
            .default_size(sz)
            .show(ctx, |ui| {
                let mut outcome = PaletteOutcome::Idle;
                let mut should_close = false;
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Command palette", "命令面板"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    let content_w = crate::ui::layout_util::finite_content_width(ui);
                    let query_before = self.query.clone();
                    let search_resp = crate::ui::chrome::search_field(
                        ui,
                        theme,
                        Self::search_id(),
                        &mut self.query,
                        crate::i18n::tr(
                            ctx,
                            "Sessions, snippets, send sequences, history, actions…",
                            "搜索会话, 片段, 发送序列, 历史, 动作…",
                        ),
                        content_w,
                    );
                    if self.focus_search {
                        search_resp.request_focus();
                        self.focus_search = false;
                    }
                    if self.query != query_before {
                        self.selected = 0;
                    }
                    let filtered = filter_palette(items, &self.query);
                    if filtered.is_empty() {
                        self.selected = 0;
                    } else if self.selected >= filtered.len() {
                        self.selected = filtered.len() - 1;
                    }
                    let keyboard_move = matches!(nav, PaletteNav::Up | PaletteNav::Down);
                    match nav {
                        PaletteNav::Up => {
                            self.selected = self.selected.saturating_sub(1);
                        }
                        PaletteNav::Down => {
                            if !filtered.is_empty() {
                                self.selected = (self.selected + 1).min(filtered.len() - 1);
                            }
                        }
                        PaletteNav::Confirm => {
                            if let Some(&src) = filtered.get(self.selected) {
                                outcome = PaletteOutcome::Run(items[src].id.clone());
                            }
                        }
                        PaletteNav::Close => should_close = true,
                        PaletteNav::None => {}
                    }
                    ui.add_space(theme.spacing_sm());
                    egui::ScrollArea::vertical()
                        .id_salt("mistterm_command_palette_scroll")
                        .max_height(scroll_h)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.set_width(content_w);
                            if filtered.is_empty() {
                                ui.label(
                                    egui::RichText::new(crate::i18n::tr(
                                        ctx,
                                        "No matches",
                                        "没有匹配",
                                    ))
                                    .size(theme.font_size_small())
                                    .color(theme.text_tertiary()),
                                );
                                return;
                            }
                            let mut prev: Option<PaletteKind> = None;
                            for (row_i, &src) in filtered.iter().enumerate() {
                                let item = &items[src];
                                if prev != Some(item.kind) {
                                    if prev.is_some() {
                                        ui.add_space(theme.spacing_sm());
                                    }
                                    ui.label(
                                        egui::RichText::new(kind_heading(ctx, item.kind))
                                            .size(theme.font_size_caption())
                                            .color(theme.text_tertiary()),
                                    );
                                    prev = Some(item.kind);
                                }
                                let selected = row_i == self.selected;
                                let resp = paint_row(ui, theme, content_w, item, selected);
                                if selected && keyboard_move {
                                    resp.scroll_to_me(Some(egui::Align::Center));
                                }
                                if resp.clicked() && matches!(outcome, PaletteOutcome::Idle) {
                                    self.selected = row_i;
                                    outcome = PaletteOutcome::Run(item.id.clone());
                                }
                            }
                        });
                    ui.add_space(theme.spacing_sm());
                    let chord = crate::platform::accel("K");
                    let footer = match crate::i18n::language(ctx) {
                        crate::i18n::UiLanguage::En => format!(
                            "{chord} opens this panel when the terminal is not focused. Focused Ctrl+K deletes to the end of the line."
                        ),
                        crate::i18n::UiLanguage::Zh => format!(
                            "{chord} 在终端失焦时打开本面板. 聚焦时 Ctrl+K 删到行尾."
                        ),
                    };
                    ui.label(
                        egui::RichText::new(footer)
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_sm());
                    if crate::ui::chrome::modal_secondary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Cross,
                        crate::i18n::tr(ctx, "Close (Esc)", "关闭 (Esc)"),
                    )
                    .clicked()
                    {
                        should_close = true;
                    }
                });
                if matches!(outcome, PaletteOutcome::Run(_)) {
                    outcome
                } else if should_close {
                    PaletteOutcome::Close
                } else {
                    outcome
                }
            });
        if let Some(inner) = &shown {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        shown
            .and_then(|r| r.inner)
            .unwrap_or(PaletteOutcome::Idle)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteOutcome {
    Idle,
    Close,
    Run(PaletteId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteNav {
    None,
    Up,
    Down,
    Confirm,
    Close,
}

/// 吃掉面板导航键和打开面板的主键，避免再进 PTY 或搜索框。
pub fn take_palette_nav(i: &mut egui::InputState) -> PaletteNav {
    if crate::ui::keyboard_shortcuts::consume_primary_key(i, Key::K) {
        return PaletteNav::Close;
    }
    if i.consume_key(Modifiers::NONE, Key::Escape) {
        return PaletteNav::Close;
    }
    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
        return PaletteNav::Up;
    }
    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
        return PaletteNav::Down;
    }
    if i.consume_key(Modifiers::NONE, Key::Enter) {
        return PaletteNav::Confirm;
    }
    PaletteNav::None
}

pub fn kind_heading(ctx: &egui::Context, kind: PaletteKind) -> &'static str {
    match kind {
        PaletteKind::Session => crate::i18n::tr(ctx, "Sessions", "会话"),
        PaletteKind::Snippet => crate::i18n::tr(ctx, "Snippets", "片段"),
        PaletteKind::Sequence => crate::i18n::tr(ctx, "Send sequences", "发送序列"),
        PaletteKind::History => crate::i18n::tr(ctx, "History", "历史"),
        PaletteKind::Action => crate::i18n::tr(ctx, "Actions", "动作"),
    }
}

fn kind_order(query_empty: bool) -> [PaletteKind; 5] {
    if query_empty {
        [
            PaletteKind::Action,
            PaletteKind::Session,
            PaletteKind::Snippet,
            PaletteKind::Sequence,
            PaletteKind::History,
        ]
    } else {
        [
            PaletteKind::Session,
            PaletteKind::Snippet,
            PaletteKind::Sequence,
            PaletteKind::History,
            PaletteKind::Action,
        ]
    }
}

fn empty_cap(kind: PaletteKind) -> usize {
    match kind {
        PaletteKind::Action => EMPTY_CAP_ACTION,
        _ => EMPTY_CAP_OTHER,
    }
}

/// 子序列模糊分。空查询返回 `Some(1)`。匹配不到返回 `None`。分越高越靠前。
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(1);
    }
    let t = text.to_lowercase();
    if t.is_empty() {
        return None;
    }
    if t == q {
        return Some(10_000);
    }
    if let Some(pos) = t.find(&q) {
        let mut score = 2_000;
        if pos == 0 {
            score += 600;
        }
        score += (240 - pos as i32).max(0);
        return Some(score);
    }
    let q_chars: Vec<char> = q.chars().collect();
    let t_chars: Vec<char> = t.chars().collect();
    let mut ti = 0;
    let mut last: Option<usize> = None;
    let mut consec = 0i32;
    let mut score = 0i32;
    for qc in q_chars {
        let mut found = false;
        while ti < t_chars.len() {
            if t_chars[ti] == qc {
                if let Some(prev) = last {
                    if ti == prev + 1 {
                        consec += 1;
                        score += 28 + consec * 8;
                    } else {
                        consec = 0;
                        score += 6;
                    }
                } else {
                    score += 14;
                    if ti == 0 {
                        score += 36;
                    }
                }
                if ti == 0 || is_boundary(t_chars[ti - 1]) {
                    score += 18;
                }
                last = Some(ti);
                ti += 1;
                found = true;
                break;
            }
            ti += 1;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '/' | '.' | ':' | '@')
}

fn item_score(query: &str, item: &PaletteItem) -> Option<i32> {
    let title = fuzzy_score(query, &item.title).map(|s| s + 120);
    let blob = fuzzy_score(query, &item.search);
    match (title, blob) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

pub fn filter_palette(items: &[PaletteItem], query: &str) -> Vec<usize> {
    let q = query.trim();
    let empty = q.is_empty();
    let mut out = Vec::new();
    for kind in kind_order(empty) {
        if empty {
            let cap = empty_cap(kind);
            let mut n = 0;
            for (i, item) in items.iter().enumerate() {
                if item.kind == kind {
                    out.push(i);
                    n += 1;
                    if n >= cap {
                        break;
                    }
                }
            }
            continue;
        }
        let mut group: Vec<(i32, usize)> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.kind == kind)
            .filter_map(|(i, item)| item_score(q, item).map(|s| (s, i)))
            .collect();
        group.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        group.truncate(QUERY_CAP_PER_KIND);
        for (_, i) in group {
            out.push(i);
        }
    }
    out
}

fn paint_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    item: &PaletteItem,
    selected: bool,
) -> egui::Response {
    let row_h = theme.font_size_normal() + theme.spacing_md();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(1.0), row_h), egui::Sense::click());
    if selected || resp.hovered() {
        ui.ctx().request_repaint();
        let fill = if selected {
            theme.bg_selected_color()
        } else {
            theme.bg_hover_color()
        };
        ui.painter()
            .rect_filled(rect, theme.radius_list_item(), fill);
    }
    let painter = ui.painter().with_clip_rect(rect);
    let pad = theme.spacing_sm();
    let title_color = theme.text_primary();
    let detail_color = theme.text_tertiary();
    let font = egui::FontId::proportional(theme.font_size_normal());
    let small = egui::FontId::proportional(theme.font_size_small());
    let detail_galley = painter.layout_no_wrap(item.detail.clone(), small, detail_color);
    let detail_w = detail_galley.size().x.min(rect.width() * 0.46);
    let title_max = (rect.width() - detail_w - pad * 3.0).max(48.0);
    let title_galley = painter.layout(item.title.clone(), font, title_color, title_max);
    let text_y = rect.center().y - theme.font_size_normal() * 0.5;
    painter.galley(egui::pos2(rect.left() + pad, text_y), title_galley, title_color);
    if !item.detail.is_empty() {
        let detail_x = rect.right() - pad - detail_w;
        let detail_y = rect.center().y - detail_galley.size().y * 0.5;
        painter.galley(egui::pos2(detail_x, detail_y), detail_galley, egui::Color32::WHITE);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, InputState};

    fn item(kind: PaletteKind, title: &str, search: &str, id: PaletteId) -> PaletteItem {
        PaletteItem {
            id,
            kind,
            title: title.to_string(),
            detail: String::new(),
            search: search.to_string(),
        }
    }

    fn sample() -> Vec<PaletteItem> {
        vec![
            item(
                PaletteKind::Session,
                "prod-db",
                "prod-db root@10.0.0.8",
                PaletteId::Session("s-prod".into()),
            ),
            item(
                PaletteKind::Session,
                "dev-web",
                "dev-web ubuntu@10.1.1.2",
                PaletteId::Session("s-dev".into()),
            ),
            item(
                PaletteKind::Snippet,
                "Disk usage",
                "Disk usage 磁盘 df -h",
                PaletteId::Snippet("frag-disk".into()),
            ),
            item(
                PaletteKind::History,
                "git status",
                "git status",
                PaletteId::History("git status".into()),
            ),
            item(
                PaletteKind::Action,
                "SFTP",
                "SFTP files 文件 打开 SFTP",
                PaletteId::Action(PaletteAction::Sftp),
            ),
            item(
                PaletteKind::Action,
                "Preferences",
                "Preferences 偏好设置 settings",
                PaletteId::Action(PaletteAction::Preferences),
            ),
        ]
    }

    #[test]
    fn shortcut_help_records_chord_decision() {
        for text in [PALETTE_SHORTCUT_HELP_EN, PALETTE_SHORTCUT_HELP_ZH] {
            assert!(text.contains("Ctrl+K"), "{text}");
            assert!(text.contains("Ctrl+Shift+K"), "{text}");
            assert!(text.contains("⌘K"), "{text}");
        }
        assert!(PALETTE_SHORTCUT_HELP_EN.contains("deletes to the end of the line"));
        assert!(PALETTE_SHORTCUT_HELP_ZH.contains("删到行尾"));
        assert!(PALETTE_SHORTCUT_HELP_EN.contains("does not conflict"));
        assert!(PALETTE_SHORTCUT_HELP_ZH.contains("不冲突"));
    }

    #[test]
    fn session_launch_focuses_open_tab_or_connects() {
        assert_eq!(session_launch(true), SessionLaunch::FocusOpenTab);
        assert_eq!(session_launch(false), SessionLaunch::Connect);
    }

    #[test]
    fn empty_query_lists_every_kind_actions_first() {
        let items = sample();
        let ids = filter_palette(&items, "");
        assert!(ids.len() >= 4);
        assert_eq!(items[ids[0]].kind, PaletteKind::Action);
        let kinds: Vec<_> = ids.iter().map(|&i| items[i].kind).collect();
        assert!(kinds.contains(&PaletteKind::Session));
        assert!(kinds.contains(&PaletteKind::Snippet));
        assert!(kinds.contains(&PaletteKind::History));
        assert!(kinds.contains(&PaletteKind::Action));
    }

    #[test]
    fn sequences_stay_a_separate_kind_from_snippets() {
        let items = vec![
            item(
                PaletteKind::Snippet,
                "Disk usage",
                "disk snippet df -h 片段",
                PaletteId::Snippet("frag-disk".into()),
            ),
            item(
                PaletteKind::Sequence,
                "After login",
                "after login 发送序列 send sequence macro 宏 echo ready",
                PaletteId::Sequence("seq-login".into()),
            ),
        ];
        let by_macro = filter_palette(&items, "宏");
        assert_eq!(by_macro.len(), 1);
        assert!(matches!(items[by_macro[0]].id, PaletteId::Sequence(_)));
        assert_eq!(items[by_macro[0]].kind, PaletteKind::Sequence);

        let empty = filter_palette(&items, "");
        let snippet_at = empty
            .iter()
            .position(|&i| items[i].kind == PaletteKind::Snippet)
            .expect("snippet");
        let sequence_at = empty
            .iter()
            .position(|&i| items[i].kind == PaletteKind::Sequence)
            .expect("sequence");
        assert!(snippet_at < sequence_at);
        assert_ne!(
            items[empty[snippet_at]].kind,
            items[empty[sequence_at]].kind
        );
    }

    #[test]
    fn query_finds_session_snippet_history_and_action() {
        let items = sample();
        let sessions = filter_palette(&items, "prod");
        assert!(sessions.iter().any(|&i| matches!(items[i].id, PaletteId::Session(ref id) if id == "s-prod")));
        assert!(!sessions.iter().any(|&i| matches!(items[i].id, PaletteId::Session(ref id) if id == "s-dev")));

        let snippets = filter_palette(&items, "disk");
        assert!(snippets.iter().any(|&i| matches!(items[i].id, PaletteId::Snippet(_))));

        let history = filter_palette(&items, "git st");
        assert!(history.iter().any(|&i| matches!(&items[i].id, PaletteId::History(cmd) if cmd == "git status")));

        let actions = filter_palette(&items, "sftp");
        assert!(actions.iter().any(|&i| matches!(items[i].id, PaletteId::Action(PaletteAction::Sftp))));
        let prefs = filter_palette(&items, "偏好");
        assert!(prefs.iter().any(|&i| matches!(items[i].id, PaletteId::Action(PaletteAction::Preferences))));
    }

    #[test]
    fn exact_name_outranks_loose_subsequence() {
        let items = vec![
            item(PaletteKind::History, "ls", "ls", PaletteId::History("ls".into())),
            item(
                PaletteKind::History,
                "false",
                "false",
                PaletteId::History("false".into()),
            ),
        ];
        let hit = filter_palette(&items, "ls");
        assert_eq!(hit.first().copied(), Some(0));
        assert!(fuzzy_score("ls", "ls").unwrap() > fuzzy_score("ls", "false").unwrap_or(0));
    }

    #[test]
    fn unknown_query_matches_nothing() {
        assert!(filter_palette(&sample(), "zzzz-not-a-thing").is_empty());
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn primary_k_mods() -> Modifiers {
        #[cfg(target_os = "macos")]
        {
            Modifiers {
                command: true,
                ..Modifiers::NONE
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            }
        }
    }

    #[test]
    fn open_palette_eats_primary_k_so_pty_does_not_see_it() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i: &mut InputState| {
                let mods = primary_k_mods();
                i.modifiers = mods;
                i.events.push(key(Key::K, mods));
                assert_eq!(take_palette_nav(i), PaletteNav::Close);
                let mut sent = Vec::new();
                assert!(!crate::ui::terminal_keys::forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(sent.is_empty(), "palette must not forward Ctrl+K as 0x0b");
            });
        });
    }

    #[test]
    fn palette_enter_is_consumed_and_typed_text_stays_for_the_field() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i: &mut InputState| {
                i.events.push(Event::Text("ls".into()));
                i.events.push(key(Key::Enter, Modifiers::NONE));
                i.events.push(key(Key::ArrowDown, Modifiers::NONE));
                assert_eq!(take_palette_nav(i), PaletteNav::Down);
                assert!(!i.key_pressed(Key::ArrowDown));
                assert!(i.events.iter().any(|e| matches!(e, Event::Text(t) if t == "ls")));
                assert_eq!(take_palette_nav(i), PaletteNav::Confirm);
                assert!(!i.key_pressed(Key::Enter));
            });
        });
    }

    #[test]
    fn ctrl_shift_k_is_not_the_palette_chord() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i: &mut InputState| {
                let mods = Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::NONE
                };
                i.modifiers = mods;
                i.events.push(key(Key::K, mods));
                assert_eq!(take_palette_nav(i), PaletteNav::None);
            });
        });
    }
}
