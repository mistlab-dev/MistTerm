//! 将 egui `Key` 映射为 xterm 风格字节序列（无 `Text` 事件的键须在此编码）。

use eframe::egui::{Event, InputState, Key, Modifiers};

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

const CTRL_KEYS: [Key; 26] = [
    Key::A,
    Key::B,
    Key::C,
    Key::D,
    Key::E,
    Key::F,
    Key::G,
    Key::H,
    Key::I,
    Key::J,
    Key::K,
    Key::L,
    Key::M,
    Key::N,
    Key::O,
    Key::P,
    Key::Q,
    Key::R,
    Key::S,
    Key::T,
    Key::U,
    Key::V,
    Key::W,
    Key::X,
    Key::Y,
    Key::Z,
];

/// Ctrl+字母 → C0 控制字节（xterm / readline 惯例）。
pub fn ctrl_byte_for_key(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(0x01),
        Key::B => Some(0x02),
        Key::C => Some(0x03),
        Key::D => Some(0x04),
        Key::E => Some(0x05),
        Key::F => Some(0x06),
        Key::G => Some(0x07),
        Key::H => Some(0x08),
        Key::I => Some(0x09),
        Key::J => Some(0x0a),
        Key::K => Some(0x0b),
        Key::L => Some(0x0c),
        Key::M => Some(0x0d),
        Key::N => Some(0x0e),
        Key::O => Some(0x0f),
        Key::P => Some(0x10),
        Key::Q => Some(0x11),
        Key::R => Some(0x12),
        Key::S => Some(0x13),
        Key::T => Some(0x14),
        Key::U => Some(0x15),
        Key::V => Some(0x16),
        Key::W => Some(0x17),
        Key::X => Some(0x18),
        Key::Y => Some(0x19),
        Key::Z => Some(0x1a),
        _ => None,
    }
}

/// Win/Linux 终端剪贴板：`Ctrl+Shift+C` / `Ctrl+Shift+V`（与 Windows Terminal 一致，避开 shell 的 Ctrl+C/V）。
pub fn terminal_clipboard_modifiers() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::NONE
    }
}

fn consume_terminal_clipboard_key(i: &mut InputState, key: Key) -> bool {
    let mods = terminal_clipboard_modifiers();
    if i.consume_key(mods, key) {
        return true;
    }
    // winit/Windows 有时不在 Key 事件上附带 modifiers，以 InputState 为准。
    if i.modifiers.matches(mods) && i.key_pressed(key) {
        i.events.retain(|e| {
            !matches!(
                e,
                Event::Key {
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

/// 消费 `Ctrl+Shift+C`（复制终端选区）。
pub fn consume_terminal_copy_shortcut(i: &mut InputState) -> bool {
    consume_terminal_clipboard_key(i, Key::C)
}

/// 消费 `Ctrl+Shift+V`（粘贴到终端）。
pub fn consume_terminal_paste_shortcut(i: &mut InputState) -> bool {
    consume_terminal_clipboard_key(i, Key::V)
}

/// 消费 Ctrl(+Shift)+字母 Key 并编码为 C0 字节；跳过终端 Copy/Paste（Ctrl+Shift+C 复制选区、Ctrl+Shift+V 粘贴）。
pub fn forward_ctrl_keys(i: &mut egui::InputState, mut send: impl FnMut(u8)) -> bool {
    let combos = [
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        },
    ];
    let mut any = false;
    for mods in combos {
        for key in CTRL_KEYS {
            if mods.shift && (key == Key::C || key == Key::V) {
                continue;
            }
            if i.consume_key(mods, key) {
                if let Some(byte) = ctrl_byte_for_key(key) {
                    send(byte);
                    any = true;
                }
            }
        }
    }
    any
}

/// Windows 等平台常以 `Event::Text` 送达 Ctrl 组合（如 `\x03`）；`sent` 去重避免与 Key 双发。
pub fn try_forward_ctrl_text_byte(
    text: &str,
    ctrl: bool,
    sent: &mut [bool; 32],
    mut send: impl FnMut(u8),
) -> bool {
    if !ctrl {
        return false;
    }
    let bytes = text.as_bytes();
    if bytes.len() != 1 {
        return false;
    }
    let b = bytes[0];
    if b == b'\t' || b == b'\n' || b == b'\r' {
        return false;
    }
    if b >= 0x20 && b != 0x7f {
        return false;
    }
    if b < 0x20 {
        if sent[b as usize] {
            return true;
        }
        sent[b as usize] = true;
    }
    send(b);
    true
}

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
    fn ctrl_byte_for_common_keys() {
        assert_eq!(ctrl_byte_for_key(Key::C), Some(0x03));
        assert_eq!(ctrl_byte_for_key(Key::W), Some(0x17));
    }

    #[test]
    fn try_forward_ctrl_text_byte_sends_and_dedupes() {
        let mut sent = [false; 32];
        let mut out = Vec::new();
        assert!(try_forward_ctrl_text_byte("\x03", true, &mut sent, |b| out.push(b)));
        assert_eq!(out, vec![0x03]);
        assert!(try_forward_ctrl_text_byte("\x03", true, &mut sent, |b| out.push(b)));
        assert_eq!(out, vec![0x03]);
        assert!(!try_forward_ctrl_text_byte("a", true, &mut sent, |b| out.push(b)));
    }

    #[test]
    fn forward_ctrl_keys_forwards_ctrl_c() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::C,
                    Modifiers {
                        ctrl: true,
                        ..Default::default()
                    },
                ));
                let mut sent = Vec::new();
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x03]);
            });
        });
    }

    #[test]
    fn forward_ctrl_keys_skips_copy_paste_shift_combos() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::C,
                    Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                ));
                let mut sent = Vec::new();
                assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(sent.is_empty());
            });
        });
    }

    #[test]
    fn consume_terminal_copy_shortcut_uses_input_modifiers_fallback() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = Modifiers {
                    ctrl: true,
                    shift: true,
                    ..Modifiers::NONE
                };
                i.events.push(key_press(Key::C, Modifiers::NONE));
                assert!(consume_terminal_copy_shortcut(i));
            });
        });
    }

    #[test]
    fn consume_terminal_paste_shortcut_matches_ctrl_shift_v() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::V,
                    Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Modifiers::NONE
                    },
                ));
                assert!(consume_terminal_paste_shortcut(i));
            });
        });
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
