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
    if i.modifiers.matches_logically(mods) && i.key_pressed(key) {
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

/// 终端剪贴板和弦是否为「仅主修饰键」(macOS ⌘C/⌘V)。
/// Win/Linux 为 Ctrl+Shift+C/V，egui-winit 不会按 copy-command 早退，仍有 Key 事件。
fn terminal_clipboard_is_command_only() -> bool {
    !terminal_clipboard_modifiers().shift
}

/// 消费终端复制快捷键(macOS ⌘C，Win/Linux Ctrl+Shift+C)。
///
/// egui-winit 0.23：⌘C / Ctrl+C 会同时发 `Key` + `Event::Copy`。
/// egui-winit ≥0.37：对 `command_only`+C/V（macOS 即 ⌘C/⌘V）**只**推 `Event::Copy`/`Paste`
/// 并 `return`，不再附带 `Key` 事件。macOS 终端复制就是 ⌘C，必须认裸 `Event::Copy`，
/// 否则 `consume_key(Key::C)` 永远 miss，快捷键全废（#336）。
///
/// 命中后仍吞掉同帧的 `Event::Copy`，避免终端透明 IME 框再处理一遍。
pub fn consume_terminal_copy_shortcut(i: &mut InputState) -> bool {
    if consume_terminal_clipboard_key(i, Key::C) {
        i.events.retain(|e| !matches!(e, Event::Copy));
        return true;
    }
    // 裸 Event::Copy = egui-winit 对 command_only+C 的交付；仅当终端复制和弦本身是
    // command_only（macOS）时认作终端复制。Win/Linux 的裸 Copy 是 Ctrl+C，应留给 PTY。
    if terminal_clipboard_is_command_only()
        && i.events.iter().any(|e| matches!(e, Event::Copy))
    {
        i.events.retain(|e| !matches!(e, Event::Copy));
        return true;
    }
    false
}

/// 消费终端粘贴快捷键(macOS ⌘V，Win/Linux Ctrl+Shift+V)。
/// 同理吞掉同帧的 `Event::Paste`：粘贴由我们自己读剪贴板写进 PTY，别再交给其它输入框。
/// egui-winit ≥0.37 在 macOS 上 ⌘V 只发 `Event::Paste`，见 [`consume_terminal_copy_shortcut`]。
///
/// #367: egui-winit only pushes `Event::Paste` when its clipboard backend returns
/// non-empty text. eframe must enable the `clipboard` feature (OS arboard); otherwise
/// external ⌘V never produces Paste, while Edit/右键粘贴 (direct arboard) still work.
pub fn consume_terminal_paste_shortcut(i: &mut InputState) -> bool {
    if consume_terminal_clipboard_key(i, Key::V) {
        i.events.retain(|e| !matches!(e, Event::Paste(_)));
        return true;
    }
    if terminal_clipboard_is_command_only()
        && i.events.iter().any(|e| matches!(e, Event::Paste(_)))
    {
        i.events.retain(|e| !matches!(e, Event::Paste(_)));
        return true;
    }
    false
}

/// Take a leftover `Event::Paste` payload and remove it from the frame.
///
/// The focused terminal IME `TextEdit` reads Paste into `ime_capture` but does not
/// remove the event; we then clear `ime_capture` (preedit must not reach the PTY).
/// If an earlier shortcut route missed the paste (#367), drain it here so ⌘V still
/// reaches the PTY instead of vanishing with the cleared capture buffer.
pub fn take_leftover_paste_event(i: &mut InputState) -> Option<String> {
    let text = i.events.iter().find_map(|e| match e {
        Event::Paste(t) if !t.is_empty() => Some(t.clone()),
        _ => None,
    })?;
    i.events.retain(|e| !matches!(e, Event::Paste(_)));
    Some(text)
}

/// 消费 Ctrl(+Shift)+字母 Key 并编码为 C0 字节。
///
/// **不转发 Ctrl+Shift+字母**：MistTerm / Windows Terminal 把 Ctrl+Shift 留给应用快捷键
/// (AI、分屏、复制等)；若再写成 C0 字节，会出现 `^A` 进 shell、会话名被当命令执行。
/// Copy/Paste 仍由 [`consume_terminal_copy_shortcut`] / paste 单独处理。
///
/// Ctrl+J / Ctrl+K 在终端聚焦时必须进 PTY（换行 / 删到行尾）。连接搜索与命令面板只在
/// 终端失焦时由应用消费，见 [`app_jk_search_armed`]。面板打开时不得再把按键送进 PTY。
pub fn forward_ctrl_keys(i: &mut egui::InputState, mut send: impl FnMut(u8)) -> bool {
    let mods = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    // 0.37 的 matches_logically 会忽略多余的 Shift，Ctrl+Shift+C/A 会被误当成 Ctrl+C/A
    // 转发进 PTY；这些组合留给剪贴板/应用快捷键。
    // 注意：shift 状态既可能在 InputState 级（ModifiersChanged），也可能只挂在事件上
    // （winit 偶发不带 ModifiersChanged），两处都要查。
    let event_has_shifted_key = i.events.iter().any(|e| {
        matches!(
            e,
            Event::Key { key: k, modifiers: m, pressed: true, .. }
                if m.shift && m.ctrl
        )
    });
    if i.modifiers.shift || event_has_shifted_key {
        return false;
    }
    let mut any = false;
    for key in CTRL_KEYS {
        if i.consume_key(mods, key) {
            if let Some(byte) = ctrl_byte_for_key(key) {
                send(byte);
                any = true;
            }
        }
    }
    any
}

/// 连接搜索（Ctrl/⌘+J）和命令面板（Ctrl/⌘+K）只在终端未持有键盘时生效。
/// 终端聚焦时这两个和弦交给 PTY（shell / readline / vim）。
/// 命令面板不用 Ctrl+Shift+K：失焦 Ctrl+K / ⌘K 与系统快捷键不冲突。
#[inline]
pub fn app_jk_search_armed(terminal_focused: bool) -> bool {
    !terminal_focused
}

/// Alt/Option（可带 Shift）、且没有 Ctrl / ⌘ 时，编码成 readline Meta（ESC 前缀）。
///
/// Ctrl+Alt（Windows 上的 AltGr、本机终端 Ctrl+Alt+T）和 ⌘⌥ 不走这里。
/// 字母、数字、退格来自 `Key`。egui 0.23 没有句号等标点键，那些由 [`encode_alt_meta_text`] 处理。
pub fn encode_alt_meta_key(key: Key, mods: Modifiers) -> Option<Vec<u8>> {
    if !mods.alt || mods.ctrl || mods.command || mods.mac_cmd {
        return None;
    }
    if key == Key::Backspace {
        if mods.shift {
            return None;
        }
        // 与普通 Backspace（0x7f）一致：M-DEL，readline 的 backward-kill-word。
        return Some(vec![0x1b, 0x7f]);
    }
    let base = alt_meta_base_byte(key)?;
    if base.is_ascii_digit() && mods.shift {
        return None;
    }
    let byte = if mods.shift && base.is_ascii_alphabetic() {
        base.to_ascii_uppercase()
    } else {
        base
    };
    Some(vec![0x1b, byte])
}

fn alt_meta_base_byte(key: Key) -> Option<u8> {
    Some(match key {
        Key::A => b'a',
        Key::B => b'b',
        Key::C => b'c',
        Key::D => b'd',
        Key::E => b'e',
        Key::F => b'f',
        Key::G => b'g',
        Key::H => b'h',
        Key::I => b'i',
        Key::J => b'j',
        Key::K => b'k',
        Key::L => b'l',
        Key::M => b'm',
        Key::N => b'n',
        Key::O => b'o',
        Key::P => b'p',
        Key::Q => b'q',
        Key::R => b'r',
        Key::S => b's',
        Key::T => b't',
        Key::U => b'u',
        Key::V => b'v',
        Key::W => b'w',
        Key::X => b'x',
        Key::Y => b'y',
        Key::Z => b'z',
        Key::Num0 => b'0',
        Key::Num1 => b'1',
        Key::Num2 => b'2',
        Key::Num3 => b'3',
        Key::Num4 => b'4',
        Key::Num5 => b'5',
        Key::Num6 => b'6',
        Key::Num7 => b'7',
        Key::Num8 => b'8',
        Key::Num9 => b'9',
        _ => return None,
    })
}

/// Alt 按下时的单个 ASCII 可见字符（含空格）。`"."` 是 yank-last-arg。
pub fn encode_alt_meta_text(text: &str) -> Option<Vec<u8>> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if ch.is_ascii() && !ch.is_control() {
        return Some(vec![0x1b, ch as u8]);
    }
    None
}

#[derive(Default)]
struct AltMetaSent {
    /// 已从 Key 发出的字母/数字（小写），用来丢掉同帧附带的 Text。
    bases: Vec<u8>,
    backspaces: usize,
}

/// 终端聚焦时把 Alt/Option 行编辑键编成 Meta 序列发给 PTY。
///
/// 按事件顺序发送，避免同一帧里先按句号再按退格时字节颠倒。
/// 只应由终端键盘路径调用。失焦时这些键仍留给输入框，不在这里消费。
pub fn forward_alt_meta_keys(i: &mut egui::InputState, mut send: impl FnMut(&[u8])) -> bool {
    let state_mods = i.modifiers;
    let rewrite_text =
        state_mods.alt && !state_mods.ctrl && !state_mods.command && !state_mods.mac_cmd;
    let mut sent = AltMetaSent::default();
    let mut any = false;
    let events = std::mem::take(&mut i.events);
    let mut kept = Vec::with_capacity(events.len());
    for event in events {
        let keep = match event {
            Event::Key {
                key,
                physical_key: _,
                pressed: true,
                repeat,
                modifiers,
            } => {
                // 事件上有时不带修饰键（winit/Windows），这时以 InputState 为准。
                let mods = if modifiers.alt
                    || modifiers.ctrl
                    || modifiers.command
                    || modifiers.shift
                    || modifiers.mac_cmd
                {
                    modifiers
                } else if rewrite_text {
                    state_mods
                } else {
                    kept.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat,
                        modifiers,
                    });
                    continue;
                };
                if let Some(bytes) = encode_alt_meta_key(key, mods) {
                    if key == Key::Backspace {
                        sent.backspaces += 1;
                    } else if let Some(base) = bytes.get(1).copied() {
                        sent.bases.push(base.to_ascii_lowercase());
                    }
                    send(&bytes);
                    any = true;
                    continue;
                }
                Some(Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat,
                    modifiers,
                })
            }
            Event::Text(text) => {
                if sent.backspaces > 0 && (text == "\u{7f}" || text == "\u{8}") {
                    sent.backspaces -= 1;
                    continue;
                }
                if let Some(base) = single_ascii_base(&text) {
                    if let Some(idx) = sent.bases.iter().position(|b| *b == base) {
                        sent.bases.remove(idx);
                        continue;
                    }
                }
                if rewrite_text {
                    // macOS 未把 Option 当 Alt 时，Option+字母会附带布局字符（如 ∫）。
                    if text.chars().count() == 1 && !text.is_ascii() && !sent.bases.is_empty() {
                        sent.bases.remove(0);
                        continue;
                    }
                    if let Some(bytes) = encode_alt_meta_text(&text) {
                        send(&bytes);
                        any = true;
                        continue;
                    }
                }
                Some(Event::Text(text))
            }
            other => Some(other),
        };
        if let Some(event) = keep {
            kept.push(event);
        }
    }
    i.events = kept;
    any
}

fn single_ascii_base(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || !ch.is_ascii() || ch.is_control() {
        return None;
    }
    Some((ch as u8).to_ascii_lowercase())
}

/// 未提交的 IME 预编辑文本。只用于终端上的叠加绘制，不写入 PTY。
/// 候选窗仍由系统输入法面板负责（egui 0.23 / winit 0.28 的 preedit 事件不含候选列表）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImePreedit {
    text: String,
}

impl ImePreedit {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
    }

    /// `CompositionStart` 清空，`CompositionUpdate` 替换，`CompositionEnd` 清空。
    /// 上屏字符串由调用方从 `CompositionEnd` 另发给 PTY。
    pub fn apply_event(&mut self, event: &Event) {
        match event {
            Event::Ime(egui::ImeEvent::Preedit { text, .. }) => self.text.clone_from(text),
            Event::Ime(egui::ImeEvent::Commit(_)) => self.text.clear(),
            _ => {}
        }
    }
}


/// Keep keyboard + IME on the terminal capture widget without cancelling composition.
///
/// egui 0.37's [`egui::Memory::request_focus`] always calls `interrupt_ime()`. The
/// terminal used to call it every frame while focused; egui-winit then toggles
/// `set_ime_allowed(false/true)` and macOS/Sogou preedit never sticks (#352).
/// Only request focus when the capture box does not already own it.
pub fn ensure_terminal_ime_focus(memory: &mut egui::Memory, ime_id: egui::Id) {
    if !memory.has_focus(ime_id) {
        memory.request_focus(ime_id);
    }
}

/// Whether plain `Event::Text` must be ignored this frame so IME does not leak into the PTY.
///
/// Terminals own composition: preedit is painted at the block cursor and only
/// [`egui::ImeEvent::Commit`] is written to the PTY. egui 0.37 `TextEdit` still inserts
/// preedit into the capture buffer, and some platforms may also emit `Event::Text` for
/// the same keystrokes or the commit string. Feeding those bytes would leave Latin
/// pinyin at the caret while Chinese commits afterward (Tian: 汉字在行尾 / 前面留下 ls).
/// #430：本帧是否处于 IME 活动窗口（组字中 / 本帧有 Ime 事件）。
///
/// winit X11 在 `is_composing` 状态失步时会把组字期间的**原始键事件**（字母、
/// 空格、候选数字键）照常发给应用——这些键随后被终端转发写进 PTY，表现就是
/// 「提交『你好』却收到 Tab/数字/乱码」。ibus-libpinyin 的 XIM preedit 回调
/// 与 winit 的 `is_composing` 同步存在缺口（见 winit 0.30.13 X11
/// event_processor 的 `keycode != 0 && !is_composing` 原始转发分支）。
///
/// 终端侧兜底：只要本帧出现任何 Ime 事件（含空 Preedit 的 Start/End 边沿），
/// 就认为这个帧在组字窗口内，把原始键转发全部让位——Commit 文本本身仍单独写
/// PTY，不受此开关影响。方向键/F 键等导航键也一并让位：组字会话内它们属于
/// 候选翻页/移动，不构成终端输入。
pub fn ime_activity_frame(events: &[Event], composing_at_frame_start: bool) -> bool {
    composing_at_frame_start
        || events
            .iter()
            .any(|event| matches!(event, Event::Ime(_)))
}

pub fn suppress_plain_text_for_ime(events: &[Event], composing_at_frame_start: bool) -> bool {
    if composing_at_frame_start {
        return true;
    }
    let has_commit = events.iter().any(|e| matches!(e, Event::Ime(egui::ImeEvent::Commit(_))));
    events.iter().any(|event| match event {
        // Commit 帧：X11 上提交串可能同时以 Ime::Commit 与合成 Text 到达，
        // 由调用方去重；不再一律压掉 Text（否则 commit 串直接丢失）。
        Event::Ime(egui::ImeEvent::Commit(_)) => false,
        Event::Ime(egui::ImeEvent::Preedit { text, .. }) => !text.is_empty(),
        // #430 四轮：Commit 帧上若还有原始 Key，说明 winit is_composing 失步仍
        // 在发原始键——其合成 Text 是组字键本身（字母/空格/数字），不是提交串。
        // 提交串由 Ime::Commit 臂单独写 PTY；这类 Text 若放行，PTY 会多出拼音
        // 首字母（Tian: 「你好」后只剩 n / 「中文」后只剩 z）。
        Event::Text(_) => has_commit && events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. })),
        _ => false,
    })
}

/// Keep a non-inverted IME area inside `clip`.
///
/// [`egui::Rect::intersect`] yields min>max when the cursor sits outside the visible
/// text clip; winit then gets a nonsense `set_ime_cursor_area` and macOS candidate
/// UI can jump to the line/window edge (行尾).
pub fn clamp_ime_rect_to_clip(rect: egui::Rect, clip: egui::Rect, min_size: egui::Vec2) -> egui::Rect {
    let hit = rect.intersect(clip);
    if hit.width() > 0.0 && hit.height() > 0.0 {
        return hit;
    }
    let w = min_size.x.max(1.0).min(clip.width().max(1.0));
    let h = min_size.y.max(1.0).min(clip.height().max(1.0));
    let x = rect.min.x.clamp(clip.min.x, (clip.max.x - w).max(clip.min.x));
    let y = rect.min.y.clamp(clip.min.y, (clip.max.y - h).max(clip.min.y));
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

/// 与 `vendor/winit` 里 X11 / macOS 补丁同一条策略：Control 已按下、键不是修饰键本身、
/// 也不是 XIM 的 keycode 0 提交事件时，不把该键交给输入法。
/// 字母、空格、退格、方向键仍走 IME。macOS 在 preedit 期间据此跳过
/// `interpretKeyEvents`（见 `vendor/winit/MISTTERM_PATCH.md`）。
#[inline]
pub fn control_chord_bypasses_ime(
    ctrl_held: bool,
    keycode_nonzero: bool,
    key_is_modifier: bool,
) -> bool {
    ctrl_held && keycode_nonzero && !key_is_modifier
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
        horizontal_arrows: true,
        vertical_arrows: true,
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
    use eframe::egui;
    use eframe::egui::{Event, Key, Modifiers};

    pub(super) fn key_press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn escape_key() {
        assert_eq!(
            encode_other_special_key(Key::Escape, Modifiers::NONE).unwrap(),
            b"\x1b"
        );
    }

    #[test]
    fn f1_and_insert_sequences() {
        assert_eq!(
            encode_other_special_key(Key::F1, Modifiers::NONE).unwrap(),
            b"\x1bOP"
        );
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
    fn forward_ctrl_keys_forwards_ctrl_j_and_k_to_pty() {
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
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x0a], "focused Ctrl+J is linefeed for the PTY");
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
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(
                    sent,
                    vec![0x0b],
                    "focused Ctrl+K is VT / kill-to-EOL for the PTY"
                );
            });
        });
    }

    /// Win/Linux 上 egui-winit 把 `command` 设成与 Ctrl 相同。聚焦时搜索未武装，
    /// 这个形状的 Ctrl+J/K 仍必须变成 PTY 的 LF/VT，不能被连接/片段搜索吃掉。
    #[test]
    fn focused_ctrl_j_and_k_reach_pty_when_command_tracks_ctrl() {
        let mods = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        assert!(!app_jk_search_armed(true));
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = mods;
                i.events.push(key_press(Key::J, mods));
                if app_jk_search_armed(true) {
                    let _ = crate::ui::keyboard_shortcuts::consume_primary_key(i, Key::J);
                }
                let mut sent = Vec::new();
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x0a]);
            });
            ui.input_mut(|i| {
                i.events.clear();
                i.modifiers = mods;
                i.events.push(key_press(Key::K, mods));
                if app_jk_search_armed(true) {
                    let _ = crate::ui::keyboard_shortcuts::consume_primary_key(i, Key::K);
                }
                let mut sent = Vec::new();
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x0b]);
            });
        });
    }

    #[test]
    fn app_search_shortcuts_only_when_terminal_unfocused() {
        assert!(!app_jk_search_armed(true));
        assert!(app_jk_search_armed(false));
    }

    /// 失焦主键打开命令面板，并且这个键不能再变成 PTY 的 VT（0x0b）。
    #[test]
    fn unfocused_primary_k_opens_palette_and_does_not_reach_pty() {
        assert!(app_jk_search_armed(false));
        assert!(!app_jk_search_armed(true));
        let mods = {
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
        };
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = mods;
                i.events.push(key_press(Key::K, mods));
                assert!(app_jk_search_armed(false));
                assert!(crate::ui::keyboard_shortcuts::consume_primary_key(
                    i,
                    Key::K
                ));
                let mut sent = Vec::new();
                assert!(!forward_ctrl_keys(i, |b| sent.push(b)));
                assert!(
                    sent.is_empty(),
                    "unfocused primary K is the command palette, not kill-to-EOL"
                );
            });
        });
    }

    fn alt_only() -> Modifiers {
        Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    #[test]
    fn encode_alt_meta_readline_chords() {
        assert_eq!(encode_alt_meta_key(Key::B, alt_only()).unwrap(), b"\x1bb");
        assert_eq!(encode_alt_meta_key(Key::F, alt_only()).unwrap(), b"\x1bf");
        assert_eq!(encode_alt_meta_key(Key::D, alt_only()).unwrap(), b"\x1bd");
        assert_eq!(
            encode_alt_meta_key(Key::Backspace, alt_only()).unwrap(),
            b"\x1b\x7f"
        );
        assert_eq!(encode_alt_meta_text(".").unwrap(), b"\x1b.");
        assert_eq!(
            encode_alt_meta_key(
                Key::B,
                Modifiers {
                    alt: true,
                    shift: true,
                    ..Default::default()
                }
            )
            .unwrap(),
            b"\x1bB"
        );
    }

    #[test]
    fn encode_alt_meta_skips_ctrl_and_command() {
        let ctrl_alt = Modifiers {
            ctrl: true,
            alt: true,
            command: true,
            ..Default::default()
        };
        assert!(encode_alt_meta_key(Key::B, ctrl_alt).is_none());
        assert!(encode_alt_meta_key(Key::T, ctrl_alt).is_none());
        assert!(encode_alt_meta_key(
            Key::B,
            Modifiers {
                command: true,
                alt: true,
                ..Default::default()
            }
        )
        .is_none());
        assert!(encode_alt_meta_key(Key::B, Modifiers::NONE).is_none());
        assert!(encode_alt_meta_text("ab").is_none());
        assert!(encode_alt_meta_text("∫").is_none());
    }

    #[test]
    fn forward_alt_meta_sends_word_motion_and_drops_option_glyph() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = alt_only();
                i.events.push(key_press(Key::B, alt_only()));
                i.events.push(Event::Text("∫".into()));
                let mut sent = Vec::new();
                assert!(forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert_eq!(sent, b"\x1bb");
                assert!(
                    !i.events.iter().any(|e| matches!(e, Event::Text(_))),
                    "Option glyph must not also be written to the PTY"
                );
            });
        });
    }

    #[test]
    fn forward_alt_meta_period_yank_and_backspace_kill_word() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = alt_only();
                i.events.push(Event::Text(".".into()));
                i.events.push(Event::Text(".".into()));
                i.events.push(key_press(Key::Backspace, alt_only()));
                let mut sent = Vec::new();
                assert!(forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert_eq!(sent, b"\x1b.\x1b.\x1b\x7f");
            });
        });
    }

    #[test]
    fn forward_alt_meta_letter_text_is_not_sent_twice() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = alt_only();
                i.events.push(key_press(Key::F, alt_only()));
                i.events.push(Event::Text("f".into()));
                let mut sent = Vec::new();
                assert!(forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert_eq!(sent, b"\x1bf");
            });
        });
    }

    #[test]
    fn forward_alt_meta_uses_input_state_when_key_event_omits_modifiers() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.modifiers = alt_only();
                i.events.push(key_press(Key::D, Modifiers::NONE));
                let mut sent = Vec::new();
                assert!(forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert_eq!(sent, b"\x1bd");
                assert!(!i.key_pressed(Key::D));
            });
        });
    }

    #[test]
    fn forward_alt_meta_does_not_take_ctrl_or_plain_keys() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                let ctrl = Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                };
                i.modifiers = ctrl;
                i.events.push(key_press(Key::J, ctrl));
                i.events.push(key_press(Key::K, ctrl));
                let mut sent = Vec::new();
                assert!(!forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert!(sent.is_empty());
                assert!(i.key_pressed(Key::J));
                assert!(i.key_pressed(Key::K));
            });
            ui.input_mut(|i| {
                i.events.clear();
                let ctrl_alt = Modifiers {
                    ctrl: true,
                    alt: true,
                    command: true,
                    ..Default::default()
                };
                i.modifiers = ctrl_alt;
                i.events.push(key_press(Key::B, ctrl_alt));
                let mut sent = Vec::new();
                assert!(!forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert!(
                    i.key_pressed(Key::B),
                    "Ctrl+Alt+B stays available (AltGr / app)"
                );
            });
            ui.input_mut(|i| {
                i.events.clear();
                i.modifiers = Modifiers::NONE;
                i.events.push(key_press(Key::B, Modifiers::NONE));
                i.events.push(Event::Text("b".into()));
                let mut sent = Vec::new();
                assert!(!forward_alt_meta_keys(i, |b| sent.extend_from_slice(b)));
                assert!(
                    i.events
                        .iter()
                        .any(|e| matches!(e, Event::Text(t) if t == "b")),
                    "plain b is not a meta sequence"
                );
            });
        });
    }

    #[test]
    fn control_chords_bypass_ime_but_plain_keys_do_not() {
        assert!(control_chord_bypasses_ime(true, true, false));
        assert!(!control_chord_bypasses_ime(false, true, false));
        assert!(!control_chord_bypasses_ime(true, false, false));
        assert!(!control_chord_bypasses_ime(true, true, true));

        // macOS keyDown: skip interpretKeyEvents only while IME is allowed,
        // a preedit is showing, and the chord itself would bypass.
        // Must match vendor/winit/src/platform_impl/ime_bypass.rs.
        let skip = |ime_allowed, composing, ctrl, key_is_modifier| {
            ime_allowed && composing && control_chord_bypasses_ime(ctrl, true, key_is_modifier)
        };
        assert!(skip(true, true, true, false));
        assert!(!skip(true, true, false, false));
        assert!(!skip(true, false, true, false));
        assert!(!skip(false, true, true, false));
        assert!(!skip(true, true, true, true));
    }

    #[test]
    fn ime_preedit_tracks_updates_and_clears_on_commit() {
        let mut preedit = ImePreedit::default();
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None })); // 0.37: Disabled 已弃用，空 Preedit 表示 IME 关闭
        assert!(preedit.is_empty());
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Preedit { text: "ni".into(), active_range_chars: None }));
        assert_eq!(preedit.text(), "ni");
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Preedit { text: "你好".into(), active_range_chars: None }));
        assert_eq!(preedit.text(), "你好");
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Commit("你好".into())));
        assert!(preedit.is_empty());
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Preedit { text: "ni".into(), active_range_chars: None }));
        preedit.apply_event(&Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None }));
        assert!(
            preedit.is_empty(),
            "empty CompositionUpdate clears a cancelled preedit"
        );
    }


    /// #352: egui 0.37 request_focus always interrupt_ime. Re-requesting every
    /// frame while the terminal IME box already owns focus cancels preedit
    /// (egui-winit toggles set_ime_allowed). ensure_terminal_ime_focus must not.
    #[test]
    fn ensure_terminal_ime_focus_does_not_interrupt_when_already_focused() {
        let ctx = egui::Context::default();
        let ime_id = egui::Id::unique("ime_focus_interrupt_352");

        // Warm-up: create + focus the capture TextEdit so PlatformOutput::ime is set.
        let mut warm = ctx.run_pass(egui::RawInput::default(), |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut t = String::new();
                    ui.add(egui::TextEdit::singleline(&mut t).id(ime_id).lock_focus(true));
                });
            });
            ctx.memory_mut(|m| m.request_focus(ime_id));
        });
        warm.textures_delta.clear();
        assert!(ctx.memory(|m| m.has_focus(ime_id)));

        // Bad path (old terminal): request_focus every frame → interrupt.
        let mut bad = ctx.run_pass(egui::RawInput::default(), |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut t = String::new();
                    ui.add(egui::TextEdit::singleline(&mut t).id(ime_id).lock_focus(true));
                });
            });
            ctx.memory_mut(|m| m.request_focus(ime_id));
        });
        bad.textures_delta.clear();
        let bad_interrupt = bad
            .platform_output
            .ime
            .as_ref()
            .map(|ime| ime.should_interrupt_composition)
            .unwrap_or(false);
        assert!(
            bad_interrupt,
            "egui 0.37 contract: request_focus while focused must interrupt IME"
        );

        // Good path: ensure_terminal_ime_focus skips when already focused.
        let mut good = ctx.run_pass(egui::RawInput::default(), |ctx| {
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut t = String::new();
                    ui.add(egui::TextEdit::singleline(&mut t).id(ime_id).lock_focus(true));
                });
            });
            ctx.memory_mut(|m| ensure_terminal_ime_focus(m, ime_id));
        });
        good.textures_delta.clear();
        let good_interrupt = good
            .platform_output
            .ime
            .as_ref()
            .map(|ime| ime.should_interrupt_composition)
            .unwrap_or(true);
        assert!(
            !good_interrupt,
            "ensure_terminal_ime_focus must not interrupt an owned IME composition"
        );
        assert!(ctx.memory(|m| m.has_focus(ime_id)));
    }

    #[test]
    fn suppress_plain_text_for_ime_while_composing_or_commit() {
        let preedit = Event::Ime(egui::ImeEvent::Preedit {
            text: "ls".into(),
            active_range_chars: None,
        });
        let commit = Event::Ime(egui::ImeEvent::Commit("了".into()));
        let empty_preedit = Event::Ime(egui::ImeEvent::Preedit {
            text: String::new(),
            active_range_chars: None,
        });
        let plain = Event::Text("ls".into());

        assert!(
            suppress_plain_text_for_ime(&[preedit.clone(), plain.clone()], false),
            "non-empty preedit in the frame must suppress Text"
        );
        // #430 四轮：无原始 Key 的 Commit 帧上，合成 Text 改由调用方
        // dedupe_commit_texts 去重（压掉会丢掉非重复的提交串副本）。
        assert!(
            !suppress_plain_text_for_ime(&[commit.clone(), plain.clone()], false),
            "commit-frame Text without raw keys is deduped by the caller, not suppressed"
        );
        assert!(
            suppress_plain_text_for_ime(&[plain.clone()], true),
            "already composing at frame start must suppress Text"
        );
        assert!(
            !suppress_plain_text_for_ime(&[empty_preedit, plain.clone()], false),
            "empty preedit alone (IME idle/clear) must not block normal typing"
        );
        assert!(
            !suppress_plain_text_for_ime(&[plain], false),
            "plain Text with no IME must pass through"
        );
    }

    #[test]
    fn clamp_ime_rect_to_clip_avoids_inverted_intersect() {
        let clip = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(200.0, 100.0));
        let inside = egui::Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(16.0, 14.0));
        let hit = clamp_ime_rect_to_clip(inside, clip, egui::vec2(8.0, 12.0));
        assert!(hit.width() > 0.0 && hit.height() > 0.0);
        assert!(clip.contains(hit.center()));

        let outside = egui::Rect::from_min_size(egui::pos2(250.0, 40.0), egui::vec2(16.0, 14.0));
        let clamped = clamp_ime_rect_to_clip(outside, clip, egui::vec2(8.0, 12.0));
        assert!(
            clamped.width() > 0.0 && clamped.height() > 0.0,
            "clamped rect must stay positive, got {clamped:?}"
        );
        assert!(
            clip.intersects(clamped),
            "clamped IME area must sit on the clip, got {clamped:?} clip={clip:?}"
        );
        assert!(
            clamped.max.x <= clip.max.x + 0.5,
            "must not hang past the right edge: {clamped:?}"
        );
    }

    #[test]
    fn composing_ctrl_c_and_ctrl_r_still_forward_to_pty() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                let ctrl = Modifiers {
                    ctrl: true,
                    ..Default::default()
                };
                i.events.push(Event::Ime(egui::ImeEvent::Preedit { text: String::new(), active_range_chars: None })); // 0.37: Disabled 已弃用
                i.events.push(Event::Ime(egui::ImeEvent::Preedit { text: "nihao".into(), active_range_chars: None }));
                i.events.push(key_press(Key::C, ctrl));
                let mut preedit = ImePreedit::default();
                for event in &i.events {
                    preedit.apply_event(event);
                }
                assert_eq!(preedit.text(), "nihao");
                let mut sent = Vec::new();
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x03]);
                assert_eq!(
                    preedit.text(),
                    "nihao",
                    "forwarding Ctrl+C must not drop preedit"
                );
            });
            ui.input_mut(|i| {
                i.events.clear();
                let ctrl = Modifiers {
                    ctrl: true,
                    ..Default::default()
                };
                i.events.push(Event::Ime(egui::ImeEvent::Preedit { text: "ceshi".into(), active_range_chars: None }));
                i.events.push(key_press(Key::R, ctrl));
                let mut sent = Vec::new();
                assert!(forward_ctrl_keys(i, |b| sent.push(b)));
                assert_eq!(sent, vec![0x12]);
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
    fn ctrl_shift_p_is_not_the_paste_shortcut() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(key_press(
                    Key::P,
                    Modifiers {
                        ctrl: true,
                        shift: true,
                        ..Default::default()
                    },
                ));
                assert!(
                    !consume_terminal_paste_shortcut(i),
                    "Ctrl+Shift+P must not paste; paste stays Ctrl+Shift+V / ⌘V"
                );
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
                i.events
                    .push(key_press(Key::V, terminal_clipboard_modifiers()));
                assert!(consume_terminal_paste_shortcut(i));
            });
        });
    }

    #[test]
    fn terminal_event_filter_blocks_focus_navigation_keys() {
        let filter = terminal_keyboard_event_filter();
        assert!(filter.tab);
        assert!(filter.horizontal_arrows);
        assert!(filter.vertical_arrows);
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
            let ime_id = egui::Id::unique("ime_capture_test");
            let mut input = egui::RawInput::default();
            input.events = vec![
                Event::ModifiersChanged(terminal_clipboard_modifiers()),
                Event::Copy,
                Event::Key {
                    key: Key::C,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: terminal_clipboard_modifiers(),
                },
            ];
            ctx.memory_mut(|m| m.request_focus(ime_id));
            let mut out = ctx.run_pass(input, |ctx| {
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
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut ime = String::new();
                        ui.add(egui::TextEdit::singleline(&mut ime).id(ime_id));
                    });
                });
            });
            out.textures_delta.clear();
                        out.platform_output
                .commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default()
        }
        // 0.37：空 IME 框收到 Event::Copy 不再往剪贴板写空串，两种行为选区都保留。
        // 该回归测试保留为守护：确认 Copy 路径不会把剪贴板改成空串。
        assert_eq!(run(false), "selected text");
        assert_eq!(run(true), "selected text");
    }


    /// egui-winit ≥0.37：command_only+C 只发 Event::Copy，无 Key。
    /// 在「终端复制 = command_only」平台（macOS）上必须命中；Win/Linux 有 Shift，跳过裸事件。
    #[test]
    fn consume_copy_shortcut_accepts_bare_event_copy_when_command_only() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(Event::Copy);
                let hit = consume_terminal_copy_shortcut(i);
                if terminal_clipboard_is_command_only() {
                    assert!(hit, "macOS ⌘C arrives as bare Event::Copy");
                    assert!(
                        !i.events.iter().any(|e| matches!(e, Event::Copy)),
                        "Event::Copy must be consumed"
                    );
                } else {
                    assert!(!hit, "Win/Linux must not treat bare Event::Copy as terminal copy");
                    assert!(
                        i.events.iter().any(|e| matches!(e, Event::Copy)),
                        "bare Event::Copy left for Ctrl+C → PTY path"
                    );
                }
            });
        });
    }

    #[test]
    fn consume_paste_shortcut_accepts_bare_event_paste_when_command_only() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(Event::Paste("hello from clipboard".into()));
                let hit = consume_terminal_paste_shortcut(i);
                if terminal_clipboard_is_command_only() {
                    assert!(hit, "macOS ⌘V arrives as bare Event::Paste");
                    assert!(
                        !i.events.iter().any(|e| matches!(e, Event::Paste(_))),
                        "Event::Paste must be consumed"
                    );
                } else {
                    assert!(!hit, "Win/Linux must not treat bare Event::Paste as terminal paste");
                }
            });
        });
    }

    /// #367: IME TextEdit may leave Event::Paste in the queue after inserting into
    /// ime_capture; we must take that payload before clearing the capture buffer.
    #[test]
    fn take_leftover_paste_event_drains_non_empty_paste() {
        egui::__run_test_ui(|ui| {
            ui.input_mut(|i| {
                i.events.push(Event::Paste("CLIP".into()));
                i.events.push(Event::Text("x".into()));
                assert_eq!(take_leftover_paste_event(i).as_deref(), Some("CLIP"));
                assert!(
                    !i.events.iter().any(|e| matches!(e, Event::Paste(_))),
                    "Paste must be drained"
                );
                assert!(
                    i.events.iter().any(|e| matches!(e, Event::Text(_))),
                    "unrelated events must stay"
                );
                assert_eq!(take_leftover_paste_event(i), None);
            });
        });
    }

    #[test]
    fn paste_shortcut_drops_paste_event() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events = vec![
            Event::ModifiersChanged(terminal_clipboard_modifiers()),
            Event::Paste("x".into()),
            Event::Key {
                key: Key::V,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: terminal_clipboard_modifiers(),
            },
        ];
        let mut frame = ctx.run_pass(input, |ctx| {
            assert!(ctx.input_mut(consume_terminal_paste_shortcut));
            assert!(ctx.input(|i| !i.events.iter().any(|e| matches!(e, Event::Paste(_)))));
        });
        frame.textures_delta.clear();
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
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: mods,
        });
        if let Some(c) = ch {
            if !mods.ctrl && !mods.command {
                events.push(Event::Text(c.into()));
            }
        }
        let mut all_events = vec![Event::ModifiersChanged(mods)];
        all_events.extend(events);
        egui::RawInput {
            events: all_events,
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
        let ime_id = egui::Id::unique("ime_capture_audit");
        // 先跑三帧让 IME 框存在、拿到焦点并锁住焦点(和真实终端一样，按键之前它就已经有焦点)。
        for frame_idx in 0..3 {
            let mut frame = ctx.run_pass(egui::RawInput::default(), |ctx| {
                ctx.root_ui(|root| {
                    egui::CentralPanel::default().show(root, |ui| {
                        let mut t = String::new();
                        ui.add(
                            egui::TextEdit::singleline(&mut t)
                                .id(ime_id)
                                .lock_focus(true),
                        );
                    });
                });
                // 和真实终端一样只请求一次焦点(每帧请求会把焦点锁重置掉)
                if frame_idx == 0 {
                    ctx.memory_mut(|m| m.request_focus(ime_id));
                }
            });
            frame.textures_delta.clear();
        }
        let mut ime_text_seen = String::new();
        let mut text_left_for_pty = 0;
        let handler = std::cell::RefCell::new(Some(handler));
        let mut out = ctx.run_pass(input, |ctx| {
            let h = handler.borrow_mut().take().unwrap();
            let app_copied = ctx.input_mut(|i| h(i));
            if app_copied {
                ctx.copy_text("APP".into());
            }
            text_left_for_pty = ctx.input(|i| {
                i.events
                    .iter()
                    .filter(|e| matches!(e, Event::Text(_)))
                    .count()
            });
            ctx.root_ui(|root| {
                egui::CentralPanel::default().show(root, |ui| {
                    let mut t = String::new();
                    ui.add(
                        egui::TextEdit::singleline(&mut t)
                            .id(ime_id)
                            .lock_focus(true),
                    );
                    ime_text_seen = t;
                });
            });
        });
        // 0.37: run_pass 产出的字体纹理增量必须显式消费，否则 Drop 断言 panic。
        out.textures_delta.clear();
        Outcome {
            copied: out
                .platform_output
                .commands
                .iter()
                .find_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
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
        // Ctrl+Shift+C。0.37：空 IME TextEdit 收到 Event::Copy 不再把空串写入剪贴板，
        // 「旧行为被覆盖」断言过时；改为守护只吞 Key 的路径也不丢/不改写剪贴板内容。
        let before = one_frame(winit_like(Key::C, cs(), None), |i| {
            consume_terminal_clipboard_key(i, Key::C)
        });
        assert_eq!(before.copied, "APP");
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
                "Ctrl+Shift+A Select All",
                winit_like(Key::A, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::A));
                    true
                }),
            ),
            (
                "Ctrl+Shift+I AI",
                winit_like(Key::I, cs(), None),
                Box::new(|i| {
                    assert!(ks::consume_primary_shift_key(i, Key::I));
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
                "unfocused Ctrl+J connection search",
                winit_like(Key::J, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(
                        app_jk_search_armed(false),
                        "connection search is only armed when the terminal is not focused"
                    );
                    assert!(ks::consume_primary_key(i, Key::J));
                    true
                }),
            ),
            (
                "unfocused Ctrl+K command palette",
                winit_like(Key::K, Modifiers::CTRL, None),
                Box::new(|i| {
                    assert!(app_jk_search_armed(false));
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
        // 0.37：IME 框不再覆盖剪贴板；此测试守护 Cut 路径不会把 "APP" 之外的内容放进剪贴板。
        let o = one_frame(winit_like(Key::X, cs(), None), |_| true);
        assert_eq!(o.copied, "APP");
    }
}
#[cfg(test)]
mod ime_activity_tests {
    use super::*;
    use super::tests::key_press;

    #[test]
    fn ime_activity_frame_detects_any_ime_event() {
        // #430：winit X11 is_composing 失步会把组字期间的原始键（空格/数字/字母）
        // 照常发进应用。本帧只要出现任何 Ime 事件（含空 Preedit 的 Start/End 边沿），
        // 就让位原始键转发，避免「提交你好却收到 Tab/数字」。
        let events = vec![
            Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None,
            }),
            key_press(Key::Space, Modifiers::NONE),
        ];
        assert!(ime_activity_frame(&events, false));
        assert!(!ime_activity_frame(&[key_press(Key::Space, Modifiers::NONE)], false));
        assert!(ime_activity_frame(&[], true), "组字态延续也要让位");
        // Commit 帧同样属于活动窗口；Commit 文本本身由 Ime 臂单独写 PTY。
        let commit = vec![
            Event::Ime(egui::ImeEvent::Commit("你好".into())),
            key_press(Key::Space, Modifiers::NONE),
        ];
        assert!(ime_activity_frame(&commit, false));
    }

    #[test]
    fn commit_frame_suppresses_raw_key_text_but_keeps_dedup_text() {
        // #430 四轮：Commit 帧带原始 Key 时，其合成 Text 是组字键本身
        // （拼音首字母），必须压掉；Ime::Commit 由调用方单独写 PTY。
        let raw_key_text = vec![
            Event::Ime(egui::ImeEvent::Commit("你好".into())),
            key_press(Key::N, Modifiers::NONE),
            Event::Text("n".into()),
        ];
        assert!(
            suppress_plain_text_for_ime(&raw_key_text, false),
            "原始键同帧的 Text(n) 必须被压掉"
        );

        // 无原始 Key 的 Commit 帧：合成 Text 可能是提交串副本，仍走调用方去重
        // （dedupe_commit_texts），不在此压掉。
        let dedup_case = vec![
            Event::Ime(egui::ImeEvent::Commit("你好".into())),
            Event::Text("你好".into()),
        ];
        assert!(!suppress_plain_text_for_ime(&dedup_case, false));
    }
}

/// #430：Commit 帧里 Ime::Commit 与合成 Text 可能携带同一提交串；写 PTY 前去重。
/// 返回真正要写的文本（Commit 优先；Text 与 Commit 重复时丢弃）。
pub fn dedupe_commit_texts<'a>(commit: &'a str, texts: &[&'a str]) -> Vec<&'a str> {
    texts
        .iter()
        .filter(|t| **t != commit)
        .copied()
        .collect()
}
