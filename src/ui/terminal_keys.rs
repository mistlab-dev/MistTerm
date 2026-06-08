//! 将 egui `Key` 映射为 xterm 风格字节序列（无 `Text` 事件的键须在此编码）。

use eframe::egui::{Key, Modifiers};

/// 是否应交给 MistTerm 应用快捷键（⌘/Ctrl 组合），勿发 PTY。
#[inline]
fn mods_reserved_for_app(mods: Modifiers) -> bool {
    mods.command
}

/// xterm 修饰参：1 + shift?1 + alt?2 + ctrl?4
#[inline]
fn xterm_modifier_param(mods: Modifiers) -> u8 {
    1 + (mods.shift as u8) + (mods.alt as u8) * 2 + (mods.ctrl as u8) * 4
}

/// 方向 / Home / End / PgUp / PgDn（含 Shift/Ctrl/Alt 组合）
pub fn encode_nav_key(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
    if mods_reserved_for_app(mods) {
        return None;
    }
    let suffix = match key {
        Key::ArrowUp => 'A',
        Key::ArrowDown => 'B',
        Key::ArrowRight => 'C',
        Key::ArrowLeft => 'D',
        Key::Home => 'H',
        Key::End => 'F',
        Key::PageUp => '5',
        Key::PageDown => '6',
        _ => return None,
    };
    let param = xterm_modifier_param(mods);
    if key == Key::PageUp || key == Key::PageDown {
        if param == 1 {
            return Some(format!("\x1b[{suffix}~").into_bytes());
        }
        return Some(format!("\x1b[1;{param}{suffix}~").into_bytes());
    }
    if param == 1 {
        return Some(format!("\x1b[{suffix}").into_bytes());
    }
    Some(format!("\x1b[1;{param}{suffix}").into_bytes())
}

/// Esc、F1–F12、Insert 等（通常无 `Event::Text`）
pub fn encode_other_special_key(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
    if mods_reserved_for_app(mods) {
        return None;
    }
    if !mods.shift && !mods.ctrl && !mods.alt {
        return match key {
            Key::Escape => Some(vec![0x1b]),
            Key::Insert => Some(b"\x1b[2~".to_vec()),
            Key::F1 => Some(b"\x1bOP".to_vec()),
            Key::F2 => Some(b"\x1bOQ".to_vec()),
            Key::F3 => Some(b"\x1bOR".to_vec()),
            Key::F4 => Some(b"\x1bOS".to_vec()),
            Key::F5 => Some(b"\x1b[15~".to_vec()),
            Key::F6 => Some(b"\x1b[17~".to_vec()),
            Key::F7 => Some(b"\x1b[18~".to_vec()),
            Key::F8 => Some(b"\x1b[19~".to_vec()),
            Key::F9 => Some(b"\x1b[20~".to_vec()),
            Key::F10 => Some(b"\x1b[21~".to_vec()),
            Key::F11 => Some(b"\x1b[23~".to_vec()),
            Key::F12 => Some(b"\x1b[24~".to_vec()),
            _ => None,
        }
        .map(|v| v);
    }
    None
}

const NAV_KEYS: [Key; 8] = [
    Key::ArrowUp,
    Key::ArrowDown,
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
];

const FN_KEYS: [Key; 12] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
];

const MOD_COMBOS: [Modifiers; 8] = [
    Modifiers::NONE,
    Modifiers {
        shift: true,
        ..Modifiers::NONE
    },
    Modifiers {
        alt: true,
        ..Modifiers::NONE
    },
    Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    },
    Modifiers {
        shift: true,
        alt: true,
        ..Modifiers::NONE
    },
    Modifiers {
        shift: true,
        ctrl: true,
        ..Modifiers::NONE
    },
    Modifiers {
        alt: true,
        ctrl: true,
        ..Modifiers::NONE
    },
    Modifiers {
        shift: true,
        alt: true,
        ctrl: true,
        ..Modifiers::NONE
    },
];

/// egui 焦点锁：终端 select 层持有键盘焦点时，阻止 Tab/Esc/方向键触发焦点遍历。
pub fn terminal_keyboard_event_filter() -> egui::EventFilter {
    egui::EventFilter {
        tab: true,
        arrows: true,
        escape: true,
    }
}

/// 消费并编码本帧内「无 Text」的特殊键；`send` 写入 PTY 或离线缓冲。
/// 若本帧转发了任意键，返回 `true`（Esc/方向键等可能触发 egui 焦点变化时的兜底，见 `pending_focus_terminal`）。
pub fn forward_non_text_keys(i: &mut egui::InputState, mut send: impl FnMut(&[u8])) -> bool {
    let mut any_sent = false;
    for mods in MOD_COMBOS {
        if i.consume_key(mods, Key::Escape) {
            send(b"\x1b");
            any_sent = true;
        }
        for key in NAV_KEYS {
            if i.consume_key(mods, key) {
                if let Some(bytes) = encode_nav_key(key, mods) {
                    send(&bytes);
                    any_sent = true;
                }
            }
        }
    }
    for key in FN_KEYS {
        if i.consume_key(Modifiers::NONE, key) {
            if let Some(bytes) = encode_other_special_key(key, Modifiers::NONE) {
                send(&bytes);
                any_sent = true;
            }
        }
    }
    if i.consume_key(Modifiers::NONE, Key::Insert) {
        send(b"\x1b[2~");
        any_sent = true;
    }
    any_sent
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Key, Modifiers};

    fn key_press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn escape_key() {
        assert_eq!(encode_other_special_key(Key::Escape, Modifiers::NONE).unwrap(), b"\x1b");
    }

    #[test]
    fn f1_and_insert_sequences() {
        assert_eq!(encode_other_special_key(Key::F1, Modifiers::NONE).unwrap(), b"\x1bOP");
        assert_eq!(
            encode_other_special_key(Key::Insert, Modifiers::NONE).unwrap(),
            b"\x1b[2~"
        );
    }

    #[test]
    fn shift_arrow_uses_modify_sequence() {
        assert_eq!(
            encode_nav_key(
                Key::ArrowUp,
                Modifiers {
                    shift: true,
                    ..Default::default()
                }
            )
            .unwrap(),
            b"\x1b[1;2A"
        );
    }

    #[test]
    fn terminal_event_filter_blocks_focus_navigation_keys() {
        let filter = terminal_keyboard_event_filter();
        assert!(filter.tab);
        assert!(filter.arrows);
        assert!(filter.escape);
        assert!(filter.matches(&key_press(Key::Tab, Modifiers::NONE)));
        assert!(filter.matches(&key_press(
            Key::Tab,
            Modifiers {
                shift: true,
                ..Default::default()
            }
        )));
        assert!(filter.matches(&key_press(Key::Escape, Modifiers::NONE)));
        assert!(filter.matches(&key_press(Key::ArrowUp, Modifiers::NONE)));
    }

    #[test]
    fn forward_non_text_keys_returns_false_without_events() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                assert!(!forward_non_text_keys(i, |_| {}));
            });
        });
    }

    #[test]
    fn forward_non_text_keys_forwards_and_flags_esc() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(Key::Escape, Modifiers::NONE));
                let mut sent = Vec::new();
                assert!(forward_non_text_keys(i, |b| sent.push(b.to_vec())));
                assert_eq!(sent, vec![b"\x1b".to_vec()]);
            });
        });
    }

    #[test]
    fn forward_non_text_keys_forwards_and_flags_arrow() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(Key::ArrowDown, Modifiers::NONE));
                let mut sent = Vec::new();
                assert!(forward_non_text_keys(i, |b| sent.push(b.to_vec())));
                assert_eq!(sent, vec![b"\x1b[B".to_vec()]);
            });
        });
    }

    #[test]
    fn forward_non_text_keys_forwards_and_flags_f_key() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(Key::F5, Modifiers::NONE));
                let mut sent = Vec::new();
                assert!(forward_non_text_keys(i, |b| sent.push(b.to_vec())));
                assert_eq!(sent, vec![b"\x1b[15~".to_vec()]);
            });
        });
    }
}
