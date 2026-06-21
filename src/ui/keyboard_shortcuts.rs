//! MistTerm 全局快捷键检测（与 readline/shell 常用 Ctrl 组合错开）。

use eframe::egui::{self, Key};

pub fn input_primary_mod(i: &egui::InputState) -> bool {
    i.modifiers.command || i.modifiers.ctrl
}

pub fn tab_switch_modifiers(i: &egui::InputState) -> bool {
    input_primary_mod(i) && !i.modifiers.shift
}

pub fn tab_index_key(n: u8) -> Option<Key> {
    match n {
        1 => Some(Key::Num1),
        2 => Some(Key::Num2),
        3 => Some(Key::Num3),
        4 => Some(Key::Num4),
        5 => Some(Key::Num5),
        6 => Some(Key::Num6),
        7 => Some(Key::Num7),
        8 => Some(Key::Num8),
        9 => Some(Key::Num9),
        _ => None,
    }
}

/// macOS：⌘W；Win/Linux：Ctrl+Shift+W（Ctrl+W 留给 shell 删词）。
pub fn close_tab_shortcut_pressed(i: &egui::InputState) -> bool {
    if !i.key_pressed(Key::W) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.shift
    }
    #[cfg(not(target_os = "macos"))]
    {
        i.modifiers.ctrl && i.modifiers.shift && !i.modifiers.command
    }
}

/// macOS：⌘T；Win/Linux：Ctrl+Shift+T（Ctrl+T 留给 shell transpose-chars）。
pub fn new_tab_shortcut_pressed(i: &egui::InputState) -> bool {
    if !i.key_pressed(Key::T) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        i.modifiers.command && !i.modifiers.ctrl && !i.modifiers.shift
    }
    #[cfg(not(target_os = "macos"))]
    {
        i.modifiers.ctrl && i.modifiers.shift && !i.modifiers.command
    }
}

/// macOS：⌘⌥←/→；Win/Linux：Ctrl+Shift+←/→（Alt+←/→ 留给 shell 按词移动）。
pub fn split_pane_focus_shortcut_pressed(i: &egui::InputState) -> bool {
    if !i.key_pressed(Key::ArrowLeft) && !i.key_pressed(Key::ArrowRight) {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        i.modifiers.command
            && i.modifiers.alt
            && !i.modifiers.ctrl
            && !i.modifiers.shift
    }
    #[cfg(not(target_os = "macos"))]
    {
        i.modifiers.ctrl
            && i.modifiers.shift
            && !i.modifiers.command
            && !i.modifiers.alt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers};

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

    #[test]
    fn ctrl_w_not_close_tab_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = ctrl_only();
                    i.events.push(key_press(Key::W, ctrl_only()));
                    assert!(
                        !close_tab_shortcut_pressed(i),
                        "Ctrl+W must stay available for shell backward-kill-word"
                    );
                });
            });
        }
    }

    #[test]
    fn ctrl_shift_w_closes_tab_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = ctrl_shift();
                    i.events.push(key_press(Key::W, ctrl_shift()));
                    assert!(close_tab_shortcut_pressed(i));
                });
            });
        }
    }

    #[test]
    fn ctrl_t_not_new_tab_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = ctrl_only();
                    i.events.push(key_press(Key::T, ctrl_only()));
                    assert!(
                        !new_tab_shortcut_pressed(i),
                        "Ctrl+T must stay available for shell transpose-chars"
                    );
                });
            });
        }
    }

    #[test]
    fn ctrl_shift_t_new_tab_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = ctrl_shift();
                    i.events.push(key_press(Key::T, ctrl_shift()));
                    assert!(new_tab_shortcut_pressed(i));
                });
            });
        }
    }

    #[test]
    fn alt_arrow_not_split_focus_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = alt_only();
                    i.events.push(key_press(Key::ArrowLeft, alt_only()));
                    assert!(
                        !split_pane_focus_shortcut_pressed(i),
                        "Alt+← must stay available for shell word motion"
                    );
                });
            });
        }
    }

    #[test]
    fn ctrl_shift_arrow_split_focus_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        {
            egui::__run_test_ui(|ui| {
                ui.input_mut(|i| {
                    i.modifiers = ctrl_shift();
                    i.events.push(key_press(Key::ArrowRight, ctrl_shift()));
                    assert!(split_pane_focus_shortcut_pressed(i));
                });
            });
        }
    }

    #[test]
    fn ctrl_digit_switches_tab_index() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = ctrl_only();
                i.events.push(key_press(Key::Num2, ctrl_only()));
                assert!(tab_switch_modifiers(i) && i.key_pressed(Key::Num2));
            });
        });
    }

    #[test]
    fn ctrl_shift_digit_does_not_switch_tab_index() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = ctrl_shift();
                i.events.push(key_press(Key::Num2, ctrl_shift()));
                assert!(!(tab_switch_modifiers(i) && i.key_pressed(Key::Num2)));
            });
        });
    }
}
