//! MistTerm 全局快捷键检测(与 readline/shell 常用 Ctrl 组合错开)。

use eframe::egui::{self, Key};

pub fn input_primary_mod(i: &egui::InputState) -> bool {
    i.modifiers.command || i.modifiers.ctrl
}

/// macOS：⌘⇧；Win/Linux：Ctrl+Shift。用于 AI / 分屏等应用快捷键(终端聚焦时也应生效)。
pub fn primary_shift_modifiers() -> egui::Modifiers {
    #[cfg(target_os = "macos")]
    {
        egui::Modifiers {
            command: true,
            shift: true,
            ..egui::Modifiers::NONE
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        egui::Modifiers {
            ctrl: true,
            shift: true,
            ..egui::Modifiers::NONE
        }
    }
}

/// 消费主修饰键+Shift+指定键；用于应用快捷键，避免事件再进 PTY。
pub fn consume_primary_shift_key(i: &mut egui::InputState, key: Key) -> bool {
    let mods = primary_shift_modifiers();
    if i.consume_key(mods, key) {
        return true;
    }
    if i.modifiers.matches(mods) && i.key_pressed(key) {
        i.events.retain(|e| {
            !matches!(
                e,
                egui::Event::Key {
                    key: k,
                    pressed: true,
                    ..
                } if *k == key
            )
        });
        return true;
    }
    false
}

/// 消费主修饰键+指定键(无 Shift)；用于 Ctrl/⌘+J 等应用快捷键，避免再进 PTY。
pub fn consume_primary_key(i: &mut egui::InputState, key: Key) -> bool {
    if i.modifiers.shift || i.modifiers.alt {
        return false;
    }
    #[cfg(target_os = "macos")]
    let mods = egui::Modifiers {
        command: true,
        ..egui::Modifiers::NONE
    };
    #[cfg(not(target_os = "macos"))]
    let mods = egui::Modifiers {
        ctrl: true,
        ..egui::Modifiers::NONE
    };
    if i.consume_key(mods, key) {
        return true;
    }
    if i.modifiers.matches(mods) && i.key_pressed(key) {
        i.events.retain(|e| {
            !matches!(
                e,
                egui::Event::Key {
                    key: k,
                    pressed: true,
                    ..
                } if *k == key
            )
        });
        return true;
    }
    false
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

/// macOS：⌘W；Win/Linux：Ctrl+Shift+W(Ctrl+W 留给 shell 删词)。
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

/// macOS：⌘T；Win/Linux：Ctrl+Shift+T(Ctrl+T 留给 shell transpose-chars)。
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

/// macOS：⌘⌥←/→；Win/Linux：Ctrl+Shift+←/→(Alt+←/→ 留给 shell 按词移动)。
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

/// ⌘/Ctrl+, 打开偏好设置；命中时吞掉该 `","` 文本事件。
///
/// egui 0.23 无 `Key::Comma`，⌘/Ctrl+, 表现为 `Text(",")` + 主修饰键。仅匹配半角 `","`；
/// 全角 `"，"` 是用户输入，绝不当快捷键、也绝不吞掉。表单 / AI 等 TextEdit 聚焦时不抢占。
///
/// `wants_keyboard_input` 必须在 `input_mut` 之外求值：egui 上下文锁不可重入，
/// 在 `input_mut` 闭包里再读 `ctx` 会让 UI 线程永久自锁。
pub fn consume_preferences_shortcut(ctx: &egui::Context, app_overrides_terminal: bool) -> bool {
    if !app_overrides_terminal || ctx.wants_keyboard_input() {
        return false;
    }
    ctx.input_mut(|i| {
        if !input_primary_mod(i) {
            return false;
        }
        let mut hit = false;
        i.events.retain(|e| {
            if let egui::Event::Text(t) = e {
                if t.as_str() == "," {
                    hit = true;
                    return false;
                }
            }
            true
        });
        hit
    })
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

    fn primary_only() -> Modifiers {
        Modifiers {
            command: true,
            mac_cmd: cfg!(target_os = "macos"),
            ctrl: !cfg!(target_os = "macos"),
            ..Modifiers::NONE
        }
    }

    /// 在真实 `Context::run` 帧里按下 ⌘/Ctrl+, 调用 [`consume_preferences_shortcut`]。
    /// 放在子线程里跑：若实现在 `ctx.input_mut` 闭包内再读 `ctx`（egui 上下文锁不可重入），
    /// 主线程会永久死锁；这里用超时把「死锁」变成可断言的失败。
    fn run_prefs_shortcut_frame(focus_text_input: bool) -> Option<(bool, bool)> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ctx = egui::Context::default();
            // 首帧窗口焦点状态变化时 egui 会清空修饰键，先空跑一帧。
            let _ = ctx.run(egui::RawInput::default(), |_| {});
            let raw = egui::RawInput {
                modifiers: primary_only(),
                events: vec![Event::Text(",".to_string())],
                ..Default::default()
            };
            let mut hit = false;
            let mut comma_left = false;
            let _ = ctx.run(raw, |ctx| {
                if focus_text_input {
                    ctx.memory_mut(|m| m.request_focus(egui::Id::new("some_text_edit")));
                }
                hit = consume_preferences_shortcut(ctx, true);
                comma_left = ctx.input(|i| {
                    i.events
                        .iter()
                        .any(|e| matches!(e, Event::Text(t) if t == ","))
                });
            });
            let _ = tx.send((hit, comma_left));
        });
        rx.recv_timeout(std::time::Duration::from_secs(3)).ok()
    }

    // 回归：多开 SSH 标签切换后终端失焦，此时按住 ⌘ 会进入偏好快捷键检测；
    // 旧实现在 `ctx.input_mut` 闭包里调 `ctx.wants_keyboard_input()` → 主线程在 egui RwLock 上自锁，整界面冻死。
    #[test]
    fn preferences_shortcut_does_not_deadlock_and_consumes_comma() {
        let (hit, comma_left) = run_prefs_shortcut_frame(false)
            .expect("consume_preferences_shortcut deadlocked the UI thread (nested egui ctx lock)");
        assert!(hit, "⌘/Ctrl+, should open preferences");
        assert!(!comma_left, "the ',' text event should be consumed");
    }

    #[test]
    fn preferences_shortcut_ignored_while_text_input_focused() {
        let (hit, comma_left) = run_prefs_shortcut_frame(true)
            .expect("consume_preferences_shortcut deadlocked the UI thread (nested egui ctx lock)");
        assert!(!hit, "must not steal ⌘/Ctrl+, from a focused text input");
        assert!(comma_left, "the ',' text event must stay for the text input");
    }
}
