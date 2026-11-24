//! #452: Software Update vs still-open Preferences — front-dialog policy.
//!
//! When both windows are open, Software Update must be the front surface for
//! painting and input so the user can read the result and use its buttons
//! without first closing Preferences. This is a narrow pair-wise policy, not a
//! global z-order scheme. It does not change check / download / install
//! semantics, and it does not speak for About, licenses, workspace confirms,
//! import, or help (those keep their existing draw / raise order).

use eframe::egui::{Pos2, Rect};

/// Which of the Preferences / Software Update pair is in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontDialog {
    None,
    Preferences,
    Update,
}

/// Software Update is in front whenever it is open, including over Preferences.
pub fn front_dialog(update_open: bool, preferences_open: bool) -> FrontDialog {
    if update_open {
        FrontDialog::Update
    } else if preferences_open {
        FrontDialog::Preferences
    } else {
        FrontDialog::None
    }
}

/// Escape (or any implicit dismiss) closes Preferences only when it is the
/// front dialog of this pair. A front Software Update absorbs Escape instead.
pub fn escape_closes_preferences(front: FrontDialog) -> bool {
    front == FrontDialog::Preferences
}

/// Escape closes Software Update when it is in front and no in-dialog confirm
/// is showing (existing confirm behavior is unchanged).
pub fn escape_closes_update(front: FrontDialog, update_confirm_open: bool) -> bool {
    front == FrontDialog::Update && !update_confirm_open
}

/// Dismissing Software Update (Escape or the window close button) must leave
/// Preferences in whatever open state it already had.
pub fn preferences_open_after_update_dismiss(preferences_was_open: bool) -> bool {
    preferences_was_open
}

/// A pointer event inside the front Software Update rect belongs to that
/// dialog, not to Preferences underneath (no click-through).
pub fn pointer_owned_by_update_dialog(update_open: bool, update_rect: Rect, pointer: Pos2) -> bool {
    update_open && update_rect.contains(pointer)
}

/// Preferences may receive a pointer event only if the click is inside it and
/// not on the front Software Update surface.
pub fn preferences_receives_pointer(
    update_open: bool,
    update_rect: Rect,
    preferences_open: bool,
    preferences_rect: Rect,
    pointer: Pos2,
) -> bool {
    if !preferences_open || !preferences_rect.contains(pointer) {
        return false;
    }
    !pointer_owned_by_update_dialog(update_open, update_rect, pointer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Rect {
        Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y))
    }

    #[test]
    fn update_is_in_front_when_both_open() {
        assert_eq!(front_dialog(true, true), FrontDialog::Update);
        assert_eq!(front_dialog(true, false), FrontDialog::Update);
        assert_eq!(front_dialog(false, true), FrontDialog::Preferences);
        assert_eq!(front_dialog(false, false), FrontDialog::None);
    }

    #[test]
    fn escape_does_not_dismiss_underlying_preferences() {
        let both = front_dialog(true, true);
        assert!(!escape_closes_preferences(both));
        assert!(escape_closes_update(both, false));
        assert!(preferences_open_after_update_dismiss(true));
    }

    #[test]
    fn escape_still_closes_preferences_when_update_is_closed() {
        let prefs = front_dialog(false, true);
        assert!(escape_closes_preferences(prefs));
        assert!(!escape_closes_update(prefs, false));
    }

    #[test]
    fn escape_does_not_close_update_while_in_dialog_confirm_is_open() {
        let front = front_dialog(true, true);
        assert!(!escape_closes_update(front, true));
        assert!(!escape_closes_preferences(front));
    }

    #[test]
    fn click_inside_update_is_not_click_through_to_preferences() {
        // Preferences is the larger, still-open window; Update is centered on top.
        let prefs = rect(0.0, 0.0, 800.0, 600.0);
        let update = rect(120.0, 80.0, 680.0, 520.0);
        let inside_update = Pos2::new(200.0, 200.0);
        let prefs_only_corner = Pos2::new(20.0, 20.0);
        let outside = Pos2::new(900.0, 20.0);

        assert!(pointer_owned_by_update_dialog(true, update, inside_update));
        assert!(!preferences_receives_pointer(
            true,
            update,
            true,
            prefs,
            inside_update
        ));
        // Exposed Preferences chrome around the update dialog can still be hit.
        assert!(preferences_receives_pointer(
            true,
            update,
            true,
            prefs,
            prefs_only_corner
        ));
        assert!(!preferences_receives_pointer(true, update, true, prefs, outside));
        // Closing Update must not itself close Preferences.
        assert!(preferences_open_after_update_dismiss(true));
    }

    #[test]
    fn preferences_receives_pointer_when_update_is_closed() {
        let prefs = rect(0.0, 0.0, 800.0, 600.0);
        let update = rect(120.0, 80.0, 680.0, 520.0);
        let inside_where_update_would_be = Pos2::new(200.0, 200.0);
        assert!(!pointer_owned_by_update_dialog(
            false,
            update,
            inside_where_update_would_be
        ));
        assert!(preferences_receives_pointer(
            false,
            update,
            true,
            prefs,
            inside_where_update_would_be
        ));
    }
}
