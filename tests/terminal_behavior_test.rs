//! 终端滚动、选区坐标、平台剪贴板快捷键（纯逻辑，无 GUI/sshd）。

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::{point_to_viewport, viewport_to_point};
use eframe::egui::{self, Event, Key, Modifiers};
use mistterm::terminal::Terminal;
use mistterm::ui::terminal_keys::{
    consume_terminal_copy_shortcut, consume_terminal_paste_shortcut, forward_ctrl_keys,
    terminal_clipboard_modifiers,
};

fn key_press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn selection_absolute_coords_follow_viewport_scroll() {
    let offset_before = 5usize;
    let vp_row = 2usize;
    let col = 4usize;
    let abs = viewport_to_point(offset_before, Point::new(vp_row, Column(col)));
    // 向底部滚（display_offset 减小）：vp_new = vp_old - offset_before + offset_after
    let offset_after = 3usize;
    let vp = point_to_viewport(offset_after, abs).expect("still visible");
    let expected_vp =
        (vp_row as i32 - offset_before as i32 + offset_after as i32).max(0) as usize;
    assert_eq!(
        vp.line, expected_vp,
        "absolute grid coords should track content when scroll offset changes"
    );
    assert_eq!(vp.column.0, col);
}

#[test]
fn scroll_up_then_to_bottom_restores_cursor_view() {
    let mut t = Terminal::new(20, 5);
    for i in 0..12 {
        t.feed(format!("line-{i:02}\r\n").as_bytes());
    }
    t.scroll_display(Scroll::Delta(3));
    assert!(!t.is_scrolled_to_bottom());
    t.scroll_to_bottom();
    assert!(t.is_scrolled_to_bottom());
    assert!(t.viewport_cursor().is_some());
}

#[test]
fn terminal_copy_shortcut_does_not_steal_ctrl_c_interrupt() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            let mods = Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            };
            i.modifiers = mods;
            i.events.push(key_press(Key::C, mods));
            let mut sent = Vec::new();
            assert!(forward_ctrl_keys(i, |b| sent.push(b)));
            assert_eq!(sent, vec![0x03]);
            assert!(!consume_terminal_copy_shortcut(i));
        });
    });
}

#[test]
fn terminal_clipboard_shortcut_matches_platform_modifiers() {
    let mods = terminal_clipboard_modifiers();
    #[cfg(target_os = "macos")]
    {
        assert!(mods.command && !mods.ctrl && !mods.shift);
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert!(mods.ctrl && mods.shift && !mods.command);
    }

    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            i.modifiers = mods;
            i.events.push(key_press(Key::C, Modifiers::NONE));
            assert!(consume_terminal_copy_shortcut(i));
            i.modifiers = mods;
            i.events.push(key_press(Key::V, Modifiers::NONE));
            assert!(consume_terminal_paste_shortcut(i));
        });
    });
}

#[test]
fn plain_ctrl_c_not_terminal_copy_on_windows() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            i.modifiers = Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            };
            i.events.push(key_press(Key::C, Modifiers::NONE));
            assert!(!consume_terminal_copy_shortcut(i));
        });
    });
}
