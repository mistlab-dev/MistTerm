//! MistTerm #188: reject illegal framebuffer sizes before macOS `drawRect:` paint.
//!
//! Lock-screen / tile-resize / Spaces wake can briefly report 0 or absurd physical
//! sizes. Feeding those into glow/eframe panics inside an ObjC callback and aborts
//! via `panic_cannot_unwind`. Shared here so Linux CI can unit-test the predicate.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// Conservative upper bound (physical px), aligned with common GPU max-texture sizes.
pub(crate) const MAX_REDRAW_PHYSICAL_DIM: u32 = 16_384;

/// `true` when width/height are safe to hand to the painter for one frame.
pub(crate) fn is_sane_redraw_physical_size(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_REDRAW_PHYSICAL_DIM
        && height <= MAX_REDRAW_PHYSICAL_DIM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_axes() {
        assert!(!is_sane_redraw_physical_size(0, 100));
        assert!(!is_sane_redraw_physical_size(100, 0));
        assert!(!is_sane_redraw_physical_size(0, 0));
    }

    #[test]
    fn rejects_above_max_texture_guard() {
        assert!(!is_sane_redraw_physical_size(MAX_REDRAW_PHYSICAL_DIM + 1, 1080));
        assert!(!is_sane_redraw_physical_size(1920, MAX_REDRAW_PHYSICAL_DIM + 1));
    }

    #[test]
    fn accepts_common_and_boundary_sizes() {
        assert!(is_sane_redraw_physical_size(1, 1));
        assert!(is_sane_redraw_physical_size(1920, 1080));
        assert!(is_sane_redraw_physical_size(3840, 2160));
        assert!(is_sane_redraw_physical_size(
            MAX_REDRAW_PHYSICAL_DIM,
            MAX_REDRAW_PHYSICAL_DIM
        ));
    }
}
