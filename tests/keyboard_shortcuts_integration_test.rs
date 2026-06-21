//! 快捷键集成测试：MistTerm 全局键 vs shell/PTY 常用 Ctrl 组合、终端 Ctrl+Shift+C/V。
//!
//! 纯 egui 输入模拟，不依赖 sshd/GUI。

use eframe::egui::{self, Event, Key, Modifiers};
use mistterm::ui::keyboard_shortcuts::{
    close_tab_shortcut_pressed, new_tab_shortcut_pressed, split_pane_focus_shortcut_pressed,
    tab_switch_modifiers,
};
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

fn ctrl_only() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    }
}

fn ctrl_shift() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::NONE
    }
}

fn alt_only() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::NONE
    }
}

fn feed_keys(i: &mut egui::InputState, mods: Modifiers, events: impl IntoIterator<Item = Event>) {
    i.modifiers = mods;
    i.events.extend(events);
}

// --- MistTerm 全局 vs shell（Win/Linux）---

#[test]
fn integration_ctrl_w_stays_with_shell_not_close_tab() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_only(), [key_press(Key::W, ctrl_only())]);
            assert!(
                !close_tab_shortcut_pressed(i),
                "Ctrl+W must stay available for shell backward-kill-word"
            );
        });
    });
}

#[test]
fn integration_ctrl_shift_w_closes_tab_not_shell_word() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::W, ctrl_shift())]);
            assert!(close_tab_shortcut_pressed(i));
        });
    });
}

#[test]
fn integration_ctrl_t_stays_with_shell_not_new_tab() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_only(), [key_press(Key::T, ctrl_only())]);
            assert!(
                !new_tab_shortcut_pressed(i),
                "Ctrl+T must stay available for shell transpose-chars"
            );
        });
    });
}

#[test]
fn integration_ctrl_shift_t_new_tab_not_shell_transpose() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::T, ctrl_shift())]);
            assert!(new_tab_shortcut_pressed(i));
        });
    });
}

#[test]
fn integration_alt_arrow_stays_with_shell_not_split_focus() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, alt_only(), [key_press(Key::ArrowLeft, alt_only())]);
            assert!(
                !split_pane_focus_shortcut_pressed(i),
                "Alt+← must stay available for shell word motion"
            );
        });
    });
}

#[test]
fn integration_ctrl_shift_arrow_split_pane_focus() {
    #[cfg(not(target_os = "macos"))]
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::ArrowRight, ctrl_shift())]);
            assert!(split_pane_focus_shortcut_pressed(i));
        });
    });
}

#[test]
fn integration_ctrl_digit_switches_tab_index() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_only(), [key_press(Key::Num3, ctrl_only())]);
            assert!(tab_switch_modifiers(i) && i.key_pressed(Key::Num3));
        });
    });
}

#[test]
fn integration_ctrl_shift_digit_does_not_switch_tab() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::Num3, ctrl_shift())]);
            assert!(!(tab_switch_modifiers(i) && i.key_pressed(Key::Num3)));
        });
    });
}

// --- PTY 转发 vs 终端剪贴板 ---

#[test]
fn integration_ctrl_c_forwards_interrupt_byte_to_pty() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_only(), [key_press(Key::C, ctrl_only())]);
            let mut sent = Vec::new();
            assert!(forward_ctrl_keys(i, |b| sent.push(b)));
            assert_eq!(sent, vec![0x03]);
            assert!(!consume_terminal_copy_shortcut(i));
        });
    });
}

#[test]
fn integration_ctrl_w_forwards_kill_word_byte_to_pty() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_only(), [key_press(Key::W, ctrl_only())]);
            let mut sent = Vec::new();
            assert!(forward_ctrl_keys(i, |b| sent.push(b)));
            assert_eq!(sent, vec![0x17]);
            assert!(!close_tab_shortcut_pressed(i));
        });
    });
}

#[test]
fn integration_ctrl_shift_c_is_terminal_copy_not_pty() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::C, ctrl_shift())]);
            let mut sent = Vec::new();
            assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
            assert!(sent.is_empty());
            assert!(consume_terminal_copy_shortcut(i));
        });
    });
}

#[test]
fn integration_ctrl_shift_v_is_terminal_paste_not_pty() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::V, ctrl_shift())]);
            let mut sent = Vec::new();
            assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
            assert!(sent.is_empty());
            assert!(consume_terminal_paste_shortcut(i));
        });
    });
}

#[test]
fn integration_ctrl_shift_c_windows_modifier_fallback() {
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(i, ctrl_shift(), [key_press(Key::C, Modifiers::NONE)]);
            let mut sent = Vec::new();
            assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
            assert!(consume_terminal_copy_shortcut(i));
        });
    });
}

#[test]
fn integration_terminal_clipboard_modifiers_match_consume_key() {
    assert_eq!(
        terminal_clipboard_modifiers(),
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        }
    );
    egui::__run_test_ui(|ui| {
        ui.input_mut(|i| {
            feed_keys(
                i,
                ctrl_shift(),
                [
                    key_press(Key::C, ctrl_shift()),
                    key_press(Key::V, ctrl_shift()),
                ],
            );
            assert!(consume_terminal_copy_shortcut(i));
            assert!(consume_terminal_paste_shortcut(i));
        });
    });
}
