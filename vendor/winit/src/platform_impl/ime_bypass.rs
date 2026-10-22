#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

//! MistTerm: which keys stay with the IME during preedit.
//!
//! Letters, Space, Backspace, arrows, and digits stay with the IME.
//! A non-modifier key while Control is already held (Ctrl+C, Ctrl+R, …)
//! must reach the application. macOS uses [`macos_skip_interpret_key_events`]
//! so `interpretKeyEvents` never sees the chord (SCIM would otherwise clear
//! marked text and winit would drop `KeyboardInput`).

use crate::event::VirtualKeyCode;

pub(crate) fn is_modifier_virtual_key(keycode: VirtualKeyCode) -> bool {
    matches!(
        keycode,
        VirtualKeyCode::LControl
            | VirtualKeyCode::RControl
            | VirtualKeyCode::LShift
            | VirtualKeyCode::RShift
            | VirtualKeyCode::LAlt
            | VirtualKeyCode::RAlt
            | VirtualKeyCode::LWin
            | VirtualKeyCode::RWin
    )
}

/// Control is held and this key is not itself a modifier.
///
/// `None` still bypasses: on macOS, modifier keys arrive via `flagsChanged`,
/// not `keyDown`. An unnamed `keyDown` with Control held is a chord.
pub(crate) fn control_chord_bypasses_ime(
    ctrl_held: bool,
    keycode: Option<VirtualKeyCode>,
) -> bool {
    if !ctrl_held {
        return false;
    }
    match keycode {
        Some(code) if is_modifier_virtual_key(code) => false,
        Some(_) | None => true,
    }
}

/// Skip `NSView::interpretKeyEvents` while a preedit is on screen.
pub(crate) fn macos_skip_interpret_key_events(
    ime_allowed: bool,
    composing: bool,
    ctrl_held: bool,
    keycode: Option<VirtualKeyCode>,
) -> bool {
    ime_allowed && composing && control_chord_bypasses_ime(ctrl_held, keycode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_letter_during_preedit_skips_ime() {
        assert!(macos_skip_interpret_key_events(
            true,
            true,
            true,
            Some(VirtualKeyCode::C),
        ));
        assert!(macos_skip_interpret_key_events(
            true,
            true,
            true,
            Some(VirtualKeyCode::R),
        ));
        assert!(macos_skip_interpret_key_events(
            true,
            true,
            true,
            Some(VirtualKeyCode::Left),
        ));
    }

    #[test]
    fn plain_editing_keys_stay_with_ime() {
        for key in [
            VirtualKeyCode::C,
            VirtualKeyCode::Space,
            VirtualKeyCode::Back,
            VirtualKeyCode::Left,
            VirtualKeyCode::Key1,
        ] {
            assert!(
                !macos_skip_interpret_key_events(true, true, false, Some(key)),
                "{key:?} without Control must stay with the IME"
            );
        }
    }

    #[test]
    fn no_preedit_or_ime_disabled_does_not_skip() {
        assert!(!macos_skip_interpret_key_events(
            true,
            false,
            true,
            Some(VirtualKeyCode::C),
        ));
        assert!(!macos_skip_interpret_key_events(
            false,
            true,
            true,
            Some(VirtualKeyCode::C),
        ));
    }

    #[test]
    fn modifier_key_itself_does_not_bypass() {
        assert!(!control_chord_bypasses_ime(
            true,
            Some(VirtualKeyCode::LControl),
        ));
        assert!(!control_chord_bypasses_ime(
            true,
            Some(VirtualKeyCode::RShift),
        ));
        assert!(control_chord_bypasses_ime(true, None));
        assert!(!control_chord_bypasses_ime(false, Some(VirtualKeyCode::C)));
    }
}
