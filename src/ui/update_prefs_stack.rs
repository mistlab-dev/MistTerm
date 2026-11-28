//! #452: Software Update vs still-open Preferences — front-dialog policy.
//!
//! When both windows are open, Software Update must be the front surface for
//! painting and input so the user can read the result and use its buttons
//! without first closing Preferences. This is a narrow pair-wise policy, not a
//! global z-order scheme. It does not change check / download / install
//! semantics, and it does not speak for About, licenses, workspace confirms,
//! import, or help (those keep their existing draw / raise order).
//!
//! egui 0.37 `Areas::end_pass` stable-sorts layers by `(Order, wants_to_be_on_top)`
//! and keeps relative order among layers that all want the top. After Software
//! Update is closed, Preferences is raised alone and therefore sits above the
//! still-remembered update Area. A later reopen that raises *both* windows
//! (explicit raise plus Window auto-raise on click / first-shown) leaves
//! Preferences in front — the repeated-open failure. A new Area id per open
//! appends the dialog at the end of the order list; Preferences is not raised
//! while the dialog is open, so the dock raise is unchanged when it is closed.

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


/// Preferences still raises itself above the right dock when Software Update is
/// *not* the front dialog. While Update is open, do not raise Preferences:
/// both would enter `wants_to_be_on_top` and keep their stored relative order.
pub fn preferences_raises_above_dock(update_open: bool) -> bool {
    !update_open
}

/// Window id for one showing of Software Update. A new `generation` after each
/// close makes egui allocate a fresh Area, which is appended on top of the
/// persistent layer list.
pub fn update_dialog_window_id(generation: u64) -> String {
    format!("update_modal-{generation}")
}

/// Bump `generation` when the dialog goes from closed to open.
pub fn note_update_dialog_opening(was_open: bool, generation: &mut u64) {
    if !was_open {
        *generation = generation.wrapping_add(1);
    }
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

    #[test]
    fn generation_bumps_only_on_closed_to_open_and_changes_window_id() {
        let mut g = 0u64;
        note_update_dialog_opening(false, &mut g);
        assert_eq!(g, 1);
        let first = update_dialog_window_id(g);
        note_update_dialog_opening(true, &mut g);
        assert_eq!(g, 1);
        assert_eq!(update_dialog_window_id(g), first);
        note_update_dialog_opening(false, &mut g);
        assert_eq!(g, 2);
        assert_ne!(update_dialog_window_id(g), first);
        assert!(preferences_raises_above_dock(false));
        assert!(!preferences_raises_above_dock(true));
    }

    // Overlapping windows, same sizes/positions as a compact Preferences + Update pair.
    const PREFS_ID: &str = "preferences_modal";
    fn prefs_pos() -> Pos2 {
        Pos2::new(80.0, 40.0)
    }
    fn prefs_size() -> eframe::egui::Vec2 {
        eframe::egui::vec2(720.0, 560.0)
    }
    fn update_pos() -> Pos2 {
        Pos2::new(200.0, 120.0)
    }
    fn update_size() -> eframe::egui::Vec2 {
        eframe::egui::vec2(400.0, 360.0)
    }

    fn raw_input() -> eframe::egui::RawInput {
        let screen = Rect::from_min_size(Pos2::ZERO, eframe::egui::vec2(1280.0, 800.0));
        let mut input = eframe::egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        if let Some(vp) = input.viewports.get_mut(&eframe::egui::ViewportId::ROOT) {
            vp.inner_rect = Some(screen);
            vp.native_pixels_per_point = Some(1.0);
        }
        input
    }

    fn run_frame(ctx: &eframe::egui::Context, f: impl FnMut(&eframe::egui::Context)) {
        ctx.run_pass(raw_input(), f).textures_delta.clear();
    }

    fn show_window(ctx: &eframe::egui::Context, id: &str, pos: Pos2, size: eframe::egui::Vec2) -> eframe::egui::LayerId {
        let resp = eframe::egui::Window::new(id)
            .title_bar(false)
            .collapsible(false)
            .constrain(false)
            .current_pos(pos)
            .fixed_pos(pos)
            .fixed_size(size)
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                let _ = ui.allocate_exact_size(size, eframe::egui::Sense::hover());
            });
        let inner = resp.expect("window shown");
        inner.response.layer_id
    }

    fn raise(ctx: &eframe::egui::Context, layer: eframe::egui::LayerId) {
        ctx.move_to_top(layer);
    }

    /// Production raise rules: Preferences raises only when Update is closed;
    /// Update always raises when shown. `update_id` is the current generation.
    fn show_pair(
        ctx: &eframe::egui::Context,
        update_open: bool,
        update_id: &str,
    ) -> (eframe::egui::LayerId, Option<eframe::egui::LayerId>) {
        let prefs_layer = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
        if preferences_raises_above_dock(update_open) {
            raise(ctx, prefs_layer);
        }
        let update_layer = if update_open {
            let layer = show_window(ctx, update_id, update_pos(), update_size());
            raise(ctx, layer);
            Some(layer)
        } else {
            None
        };
        (prefs_layer, update_layer)
    }

    /// Documents why the first-open raise was not enough: a reused Area id
    /// plus raising both windows keeps Preferences in front after close+reopen.
    #[test]
    fn reused_update_area_id_stays_behind_preferences_after_close_and_reopen() {
        let ctx = eframe::egui::Context::default();
        let stale_id = "update_modal";
        // First open: new Area is appended last, so Update is in front.
        let mut layers = (None, None);
        run_frame(&ctx, |ctx| {
            let p = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
            raise(ctx, p);
            let u = show_window(ctx, stale_id, update_pos(), update_size());
            raise(ctx, u);
            layers = (Some(p), Some(u));
        });
        assert_eq!(
            ctx.top_layer_id(),
            layers.1,
            "first open with a new Area should be in front"
        );
        // Close Update; Preferences keeps raising (dock behavior).
        for _ in 0..3 {
            run_frame(&ctx, |ctx| {
                let p = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
                raise(ctx, p);
                layers.0 = Some(p);
            });
            assert_eq!(ctx.top_layer_id(), layers.0, "prefs only after dismiss");
        }
        // Reopen with the SAME id and raise both, as the first fix did. Also
        // mark Preferences as wanting the top (the second 「检查更新」 click).
        run_frame(&ctx, |ctx| {
            let p = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
            raise(ctx, p);
            let u = show_window(ctx, stale_id, update_pos(), update_size());
            raise(ctx, u);
            layers = (Some(p), Some(u));
        });
        // Following frames: both still raised every frame.
        run_frame(&ctx, |ctx| {
            let p = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
            raise(ctx, p);
            let u = show_window(ctx, stale_id, update_pos(), update_size());
            raise(ctx, u);
            layers = (Some(p), Some(u));
        });
        assert_eq!(
            ctx.top_layer_id(),
            layers.0,
            "reusing the Area id and raising both windows must keep Preferences in front (egui relative order)"
        );
    }

    /// Production close+reopen: new generation id, do not raise Preferences
    /// while Update is open. Must stay in front for several frames, including
    /// a reopen after Preferences has been raised alone.
    #[test]
    fn close_and_reopen_keeps_update_in_front_for_repeated_frames() {
        let ctx = eframe::egui::Context::default();
        let mut generation = 0u64;
        let mut update_open = false;

        note_update_dialog_opening(update_open, &mut generation);
        update_open = true;
        let first_id = update_dialog_window_id(generation);
        let mut layers = (None, None);
        run_frame(&ctx, |ctx| {
            let (p, u) = show_pair(ctx, update_open, &first_id);
            layers = (Some(p), u);
        });
        assert_eq!(ctx.top_layer_id(), layers.1, "first open");

        // Dismiss Update; Preferences stays open and raises vs dock.
        update_open = false;
        for _ in 0..3 {
            run_frame(&ctx, |ctx| {
                let (p, u) = show_pair(ctx, false, &first_id);
                layers = (Some(p), u);
            });
            assert_eq!(ctx.top_layer_id(), layers.0, "prefs only after dismiss");
            assert!(layers.1.is_none());
        }

        // Second 「检查更新」: bump generation, open again. Simulate the click
        // also wanting Preferences on top (Window auto-raise on the button).
        note_update_dialog_opening(update_open, &mut generation);
        update_open = true;
        let second_id = update_dialog_window_id(generation);
        assert_ne!(second_id, first_id);
        run_frame(&ctx, |ctx| {
            let p = show_window(ctx, PREFS_ID, prefs_pos(), prefs_size());
            // Click on still-open Preferences starts the second check.
            raise(ctx, p);
            let (_, u) = show_pair(ctx, true, &second_id);
            layers = (Some(p), u);
        });
        assert_eq!(ctx.top_layer_id(), layers.1, "second open, click frame");

        // Result frame and later frames (check completed, dialog still open).
        for i in 0..4 {
            run_frame(&ctx, |ctx| {
                let (p, u) = show_pair(ctx, true, &second_id);
                layers = (Some(p), u);
            });
            assert_eq!(
                ctx.top_layer_id(),
                layers.1,
                "second open still in front on follow-up frame {i}"
            );
        }
        assert!(update_open);
        assert!(!preferences_raises_above_dock(update_open));
    }
}
