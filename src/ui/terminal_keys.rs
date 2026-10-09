//! 将 egui `Key` 映射为 xterm 风格字节序列(无 `Text` 事件的键须在此编码)。

use eframe::egui::{Event, InputState, Key, Modifiers};

/// 是否应交给 MistTerm 应用快捷键(⌘/Ctrl 组合)，勿发 PTY。
#[inline]
fn mods_reserved_for_app(mods: Modifiers) -> bool {
    mods.command
}

/// xterm 修饰参：1 + shift?1 + alt?2 + ctrl?4
#[inline]
fn xterm_modifier_param(mods: Modifiers) -> u8 {
    1 + (mods.shift as u8) + (mods.alt as u8) * 2 + (mods.ctrl as u8) * 4
}

/// 方向 / Home / End / PgUp / PgDn(含 Shift/Ctrl/Alt 组合)
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

/// Esc、F1–F12、Insert 等(通常无 `Event::Text`)
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

/// Ctrl+字母 → C0 控制字节(xterm / readline 惯例)。
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

/// 终端剪贴板快捷键修饰键：macOS `⌘C`/`⌘V`；Win/Linux `Ctrl+Shift+C`/`Ctrl+Shift+V`(避开 shell 的 Ctrl+C/V)。
pub fn terminal_clipboard_modifiers() -> Modifiers {
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
            shift: true,
            ..Modifiers::NONE
        }
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

/// 丢掉本帧 egui-winit 随 ⌘/Ctrl+C/X/V 附带发出的 `Copy` / `Cut` / `Paste` 事件。
/// 应用已经自己处理了这个按键时调用，避免终端里的透明 IME 框等再处理一遍。
pub fn drop_clipboard_events(i: &mut InputState) {
    i.events
        .retain(|e| !matches!(e, Event::Copy | Event::Cut | Event::Paste(_)));
}

/// 消费终端复制快捷键(macOS ⌘C，Win/Linux Ctrl+Shift+C)。
///
/// egui-winit 在 ⌘C / Ctrl+C(含 Ctrl+Shift+C) 时除了 Key 事件还会再发一个 `Event::Copy`。
/// 终端里那个透明的 IME 输入框(空字符串、持有焦点)收到 `Event::Copy` 会把「空串」写进
/// `copied_text`，把我们刚放进去的选区覆盖掉：提示「已复制」，剪贴板却没变。
/// 所以命中快捷键时把同一帧的 `Event::Copy` 一起吞掉。
pub fn consume_terminal_copy_shortcut(i: &mut InputState) -> bool {
    if consume_terminal_clipboard_key(i, Key::C) {
        i.events.retain(|e| !matches!(e, Event::Copy));
        return true;
    }
    false
}

/// 消费终端粘贴快捷键(macOS ⌘V，Win/Linux Ctrl+Shift+V)。
/// 同理吞掉同帧的 `Event::Paste`：粘贴由我们自己读剪贴板写进 PTY，别再交给其它输入框。
pub fn consume_terminal_paste_shortcut(i: &mut InputState) -> bool {
    if consume_terminal_clipboard_key(i, Key::V) {
        i.events.retain(|e| !matches!(e, Event::Paste(_)));
        return true;
    }
    false
}

/// 消费 Ctrl(+Shift)+字母 Key 并编码为 C0 字节。
///
/// **不转发 Ctrl+Shift+字母**：MistTerm / Windows Terminal 把 Ctrl+Shift 留给应用快捷键
/// (AI、分屏、复制等)；若再写成 C0 字节，会出现 `^A` 进 shell、会话名被当命令执行。
/// Copy/Paste 仍由 [`consume_terminal_copy_shortcut`] / paste 单独处理。
pub fn forward_ctrl_keys(i: &mut egui::InputState, mut send: impl FnMut(u8)) -> bool {
    let mods = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let mut any = false;
    for key in CTRL_KEYS {
        // 应用快捷键：Ctrl+J 连接搜索、Ctrl+K 片段搜索——绝不能变成 PTY 的 LF/VT。
        if matches!(key, Key::J | Key::K) {
            continue;
        }
        if i.consume_key(mods, key) {
            if let Some(byte) = ctrl_byte_for_key(key) {
                send(byte);
                any = true;
            }
        }
    }
    any
}

/// Windows 等平台常以 `Event::Text` 送达 Ctrl 组合(如 `\x03`)；`sent` 去重避免与 Key 双发。
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
/// 若本帧转发了任意键，返回 `true`(Esc/方向键等可能触发 egui 焦点变化时的兜底，见 `pending_focus_terminal`)。
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
    use eframe::egui;

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
    fn consume_primary_shift_key_eats_ctrl_shift_a() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = crate::ui::keyboard_shortcuts::primary_shift_modifiers();
                i.events.push(key_press(
                    Key::A,
                    crate::ui::keyboard_shortcuts::primary_shift_modifiers(),
                ));
                assert!(crate::ui::keyboard_shortcuts::consume_primary_shift_key(
                    i,
                    Key::A
                ));
                assert!(!i.key_pressed(Key::A));
            });
        });
    }

    #[test]
    fn forward_ctrl_keys_skips_ctrl_j_and_k() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::J,
                    Modifiers {
                        ctrl: true,
                        ..Default::default()
                    },
                ));
                let mut sent = Vec::new();
                assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(sent.is_empty(), "Ctrl+J must not become 0x0a for PTY");
            });
            ui.input_mut(|i| {
                i.events.clear();
                i.events.push(key_press(
                    Key::K,
                    Modifiers {
                        ctrl: true,
                        ..Default::default()
                    },
                ));
                let mut sent = Vec::new();
                assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(sent.is_empty(), "Ctrl+K must not become 0x0b for PTY");
            });
        });
    }

    #[test]
    fn forward_ctrl_keys_skips_ctrl_shift_letters() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::A,
                    Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                ));
                let mut sent = Vec::new();
                assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(sent.is_empty(), "Ctrl+Shift+A must not become 0x01 for PTY");
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
                i.modifiers = terminal_clipboard_modifiers();
                i.events.push(key_press(Key::C, Modifiers::NONE));
                assert!(consume_terminal_copy_shortcut(i));
            });
        });
    }

    #[test]
    fn consume_terminal_paste_shortcut_matches_platform_clipboard_mods() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(Key::V, terminal_clipboard_modifiers()));
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

    /// 回归：⌘C(Win/Linux 为 Ctrl+Shift+C) 时 egui-winit 同帧还会发 `Event::Copy`；
    /// 终端的透明 IME 框(空串、有焦点)收到它会把 `copied_text` 覆盖成空串。
    #[test]
    fn copy_shortcut_survives_focused_empty_ime_box() {
        fn run(consume: bool) -> String {
            let ctx = egui::Context::default();
            let ime_id = egui::Id::new("ime_capture_test");
            let mut input = egui::RawInput::default();
            input.modifiers = terminal_clipboard_modifiers();
            input.events = vec![
                Event::Copy,
                Event::Key {
                    key: Key::C,
                    pressed: true,
                    repeat: false,
                    modifiers: terminal_clipboard_modifiers(),
                },
            ];
            ctx.memory_mut(|m| m.request_focus(ime_id));
            let out = ctx.run(input, |ctx| {
                // 与 app.rs 的顺序一致：先处理复制快捷键，再画终端(含 IME 框)。
                let hit = ctx.input_mut(|i| {
                    if consume {
                        consume_terminal_copy_shortcut(i)
                    } else {
                        consume_terminal_clipboard_key(i, Key::C)
                    }
                });
                assert!(hit);
                ctx.copy_text("selected text".to_string());
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut ime = String::new();
                    ui.add(egui::TextEdit::singleline(&mut ime).id(ime_id));
                });
            });
            out.platform_output.copied_text
        }
        // 只吞 Key、不吞 Event::Copy(旧行为)：被 IME 框覆盖成空串。
        assert_eq!(run(false), "");
        // 修好后：选区保留。
        assert_eq!(run(true), "selected text");
    }

    #[test]
    fn paste_shortcut_drops_paste_event() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.modifiers = terminal_clipboard_modifiers();
        input.events = vec![
            Event::Paste("x".into()),
            Event::Key {
                key: Key::V,
                pressed: true,
                repeat: false,
                modifiers: terminal_clipboard_modifiers(),
            },
        ];
        let _ = ctx.run(input, |ctx| {
            assert!(ctx.input_mut(consume_terminal_paste_shortcut));
            assert!(ctx.input(|i| !i.events.iter().any(|e| matches!(e, Event::Paste(_)))));
        });
    }

    // ── 快捷键审计：同一帧里，egui-winit 附带的语义事件 + 有焦点的空 IME 框，会不会把应用动作搅掉 ──

    /// 按 egui-winit 0.23 在 Windows/Linux 上的做法生成一次按键的事件：
    /// `command` 跟随 Ctrl；⌘/Ctrl+C/X/V 额外发 Copy/Cut/Paste；按住 Ctrl 时不发 Text。
    fn winit_like(key: Key, mods: Modifiers, ch: Option<&str>) -> egui::RawInput {
        let mods = Modifiers {
            command: mods.ctrl || mods.command,
            ..mods
        };
        let mut events = Vec::new();
        if mods.command && key == Key::C {
            events.push(Event::Copy);
        }
        if mods.command && key == Key::X {
            events.push(Event::Cut);
        }
        if mods.command && key == Key::V {
            events.push(Event::Paste("CLIP".into()));
        }
        events.push(Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers: mods,
        });
        if let Some(c) = ch {
            if !mods.ctrl && !mods.command {
                events.push(Event::Text(c.into()));
            }
        }
        egui::RawInput {
            modifiers: mods,
            events,
            ..Default::default()
        }
    }

    struct Outcome {
        copied: String,
        ime_focused: bool,
        ime_text_seen: String,
        text_left_for_pty: usize,
    }

    /// 一帧：先跑应用的快捷键处理(`handler`；返回 true 表示应用自己往剪贴板放了 "APP")，
    /// 再画终端那个有焦点的空 IME 框；看剪贴板、焦点、IME 框里有没有被塞东西、还剩多少 Text 会进 PTY。
    fn one_frame(input: egui::RawInput, handler: impl FnOnce(&mut InputState) -> bool) -> Outcome {
        let ctx = egui::Context::default();
        let ime_id = egui::Id::new("ime_capture_audit");
        // 先跑三帧让 IME 框存在、拿到焦点并锁住焦点(和真实终端一样，按键之前它就已经有焦点)。
        for frame in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut t = String::new();
                    ui.add(
                        egui::TextEdit::singleline(&mut t)
                            .id(ime_id)
                            .lock_focus(true),
                    );
                });
                // 和真实终端一样只请求一次焦点(每帧请求会把焦点锁重置掉)
                if frame == 0 {
                    ctx.memory_mut(|m| m.request_focus(ime_id));
                }
            });
        }
        let mut ime_text_seen = String::new();
        let mut text_left_for_pty = 0;
        let out = ctx.run(input, |ctx| {
            let app_copied = ctx.input_mut(handler);
            if app_copied {
                ctx.copy_text("APP".into());
            }
            text_left_for_pty = ctx.input(|i| {
                i.events
                    .iter()
                    .filter(|e| matches!(e, Event::Text(_)))
                    .count()
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut t = String::new();
                ui.add(
                    egui::TextEdit::singleline(&mut t)
                        .id(ime_id)
                        .lock_focus(true),
                );
                ime_text_seen = t;
            });
        });
        Outcome {
            copied: out.platform_output.copied_text,
            ime_focused: ctx.memory(|m| m.has_focus(ime_id)),
            ime_text_seen,
            text_left_for_pty,
        }
    }

    fn cs() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        }
    }

    #[test]
    fn audit_copy_and_paste_shortcuts() {
        // Ctrl+Shift+C：修好前(只吞 Key)剪贴板被 IME 框改成空串；修好后保留。
        let before = one_frame(winit_like(Key::C, cs(), None), |i| {
            consume_terminal_clipboard_key(i, Key::C)
        });
        assert_eq!(before.copied, "", "旧行为：被覆盖");
        let after = one_frame(
            winit_like(Key::C, cs(), None),
            consume_terminal_copy_shortcut,
        );
        assert_eq!(after.copied, "APP");
        assert!(after.ime_focused);

        // Ctrl+Shift+V：旧行为下 Paste 事件被塞进 IME 框(随后被清空，所以没有实际危害)；现在直接丢掉。
        let before = one_frame(winit_like(Key::V, cs(), None), |i| {
            consume_terminal_clipboard_key(i, Key::V);
            false
        });
        assert_eq!(before.ime_text_seen, "CLIP");
        let after = one_frame(winit_like(Key::V, cs(), None), |i| {
            assert!(consume_terminal_paste_shortcut(i));
            false
        });
        assert_eq!(after.ime_text_seen, "");
        assert!(after.ime_focused);
    }

    #[test]
    fn audit_app_shortcuts_not_undone_by_ime_box() {
        use crate::ui::keyboard_shortcuts as ks;
        // 假设应用这一帧也往剪贴板放了东西(如复制)，看 IME 框会不会把它改掉、会不会丢焦点、会不会留下 Text 进 PTY。
        let cases: Vec<(
            &str,
            egui::RawInput,
            Box<dyn FnOnce(&mut InputState) -> bool>,
        )> = vec![
            (
                "Ctrl+Shift+A AI",
                winit_like(Key::A, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::A));
                    true
                }),
            ),
            (
                "Ctrl+Shift+L",
                winit_like(Key::L, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::L));
                    true
                }),
            ),
            (
                "Ctrl+Shift+D split",
                winit_like(Key::D, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::D));
                    true
                }),
            ),
            (
                "Ctrl+Shift+U",
                winit_like(Key::U, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::U));
                    true
                }),
            ),
            (
                "Ctrl+J search",
                winit_like(Key::J, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(ks::consume_primary_key(i, Key::J));
                    true
                }),
            ),
            (
                "Ctrl+K snippets",
                winit_like(Key::K, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(ks::consume_primary_key(i, Key::K));
                    true
                }),
            ),
            (
                "Ctrl+F find",
                winit_like(Key::F, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(i.consume_key(Modifiers::CTRL, Key::F));
                    true
                }),
            ),
            // 只看 key_pressed、不吞按键的：按键会留给 IME 框
            (
                "Ctrl+Shift+W close tab",
                winit_like(Key::W, cs(), None),
                Box::new(|i| {
                    assert!(ks::close_tab_shortcut_pressed(i));
                    true
                }),
            ),
            (
                "Ctrl+Shift+T new tab",
                winit_like(Key::T, cs(), None),
                Box::new(|i| {
                    assert!(ks::new_tab_shortcut_pressed(i));
                    true
                }),
            ),
            (
                "Ctrl+1 tab",
                winit_like(Key::Num1, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(ks::tab_switch_modifiers(i) && i.key_pressed(Key::Num1));
                    true
                }),
            ),
            (
                "Ctrl+Tab",
                winit_like(Key::Tab, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(i.key_pressed(Key::Tab));
                    true
                }),
            ),
            (
                "Ctrl+Shift+Right pane",
                winit_like(Key::ArrowRight, cs(), None),
                Box::new(|i| {
                    assert!(ks::split_pane_focus_shortcut_pressed(i));
                    true
                }),
            ),
            (
                "Ctrl+Shift+J picker",
                winit_like(Key::J, cs(), None),
                Box::new(|i| {
                    assert!(i.key_pressed(Key::J));
                    true
                }),
            ),
            (
                "Ctrl+N new session",
                winit_like(Key::N, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(i.key_pressed(Key::N));
                    true
                }),
            ),
            // 片段快捷键 Ctrl+Shift+X：egui-winit 还会发 Cut；片段处理时一并丢掉
            (
                "snippet Ctrl+Shift+X",
                winit_like(Key::X, cs(), None),
                Box::new(|i| {
                    drop_clipboard_events(i);
                    true
                }),
            ),
            // 片段快捷键 Ctrl+Shift+Alt+Y(Windows 上 Ctrl+Alt 可能是 AltGr，能出字符)：按住 Ctrl 时 egui-winit 不发 Text
            (
                "snippet Ctrl+Shift+Alt+Y",
                winit_like(Key::Y, Modifiers { alt: true, ..cs() }, Some("¥")),
                Box::new(|_| true),
            ),
        ];
        for (name, input, handler) in cases {
            let o = one_frame(input, handler);
            assert_eq!(o.copied, "APP", "{name}: 剪贴板被改掉");
            assert!(o.ime_focused, "{name}: IME 框丢了焦点");
            assert_eq!(o.ime_text_seen, "", "{name}: IME 框被塞了文字");
            assert_eq!(o.text_left_for_pty, 0, "{name}: 还有 Text 会进终端");
        }
    }

    #[test]
    fn audit_snippet_cut_without_drop_would_clobber() {
        // 证明 drop_clipboard_events 有必要：Ctrl+Shift+X 的 Cut 留给 IME 框会把本帧剪贴板改成空串。
        let o = one_frame(winit_like(Key::X, cs(), None), |_| true);
        assert_eq!(o.copied, "");
    }
}
