//! 终端键盘焦点：egui 多帧模拟 Tab/Esc/方向键是否抢焦点。

use eframe::egui::{self, Event, Id, Key, Modifiers, RawInput, Sense};
use mistterm::ui::terminal_keys::terminal_keyboard_event_filter;

fn key_press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// 一帧 UI：侧栏按钮 + 终端式 interact；返回 (终端是否聚焦, 按钮是否聚焦)。
fn focus_snapshot(ctx: &egui::Context, terminal_id: Id, apply_filter: bool) -> (bool, bool) {
    let mut term_focus = false;
    let mut other_focus = false;
    egui::CentralPanel::default().show(ctx, |ui| {
        let other = ui.button("other");
        ui.interact(ui.max_rect(), terminal_id, Sense::click());
        if apply_filter && ui.memory(|m| m.has_focus(terminal_id)) {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(terminal_id, terminal_keyboard_event_filter());
            });
        }
        term_focus = ui.memory(|m| m.has_focus(terminal_id));
        other_focus = other.has_focus();
    });
    (term_focus, other_focus)
}

/// 预热两帧并锁定 EventFilter（与 [`TerminalView::show`] 中 select 层一致）。
fn warm_terminal_focus(ctx: &egui::Context, terminal_id: Id) {
    let input = RawInput::default();
    let _ = ctx.run(input.clone(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.interact(ui.max_rect(), terminal_id, Sense::click());
            ui.memory_mut(|m| m.request_focus(terminal_id));
        });
    });
    for _ in 0..2 {
        let _ = ctx.run(input.clone(), |ctx| {
            let _ = focus_snapshot(ctx, terminal_id, true);
        });
    }
}

#[test]
fn tab_with_event_filter_keeps_terminal_focus() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::default());
    let terminal_id = Id::new("terminal_focus_test_tab");

    warm_terminal_focus(&ctx, terminal_id);

    let mut input = RawInput::default();
    input.events.push(key_press(Key::Tab, Modifiers::NONE));
    let _ = ctx.run(input, |ctx| {
        let (term, other) = focus_snapshot(ctx, terminal_id, true);
        assert!(term, "Tab must not steal terminal focus when EventFilter.tab is set");
        assert!(!other);
    });
}

#[test]
fn escape_with_event_filter_keeps_terminal_focus() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::default());
    let terminal_id = Id::new("terminal_focus_test_esc");

    warm_terminal_focus(&ctx, terminal_id);

    let mut input = RawInput::default();
    input.events.push(key_press(Key::Escape, Modifiers::NONE));
    let _ = ctx.run(input, |ctx| {
        let (term, other) = focus_snapshot(ctx, terminal_id, true);
        assert!(term, "Esc must not clear terminal focus when EventFilter.escape is set");
        assert!(!other);
    });
}

#[test]
fn arrow_with_event_filter_keeps_terminal_focus() {
    let ctx = egui::Context::default();
    ctx.set_fonts(egui::FontDefinitions::default());
    let terminal_id = Id::new("terminal_focus_test_arrow");

    warm_terminal_focus(&ctx, terminal_id);

    let mut input = RawInput::default();
    input.events.push(key_press(Key::ArrowUp, Modifiers::NONE));
    let _ = ctx.run(input, |ctx| {
        let (term, other) = focus_snapshot(ctx, terminal_id, true);
        assert!(
            term,
            "Arrow keys must not move focus when EventFilter.arrows is set"
        );
        assert!(!other);
    });
}
