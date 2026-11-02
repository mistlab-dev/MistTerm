# MistTerm patch on winit 0.30.13

Upstream crate: `winit 0.30.13` (eframe/egui 0.37 stack).

Changed files:

- `src/platform_impl/linux/x11/event_processor.rs` (X11)
- `src/platform_impl/macos/view.rs` (macOS)
- `src/platform_impl/ime_bypass.rs` (shared Control-chord policy, used by macOS; unit-tested on every OS)
- `src/platform_impl/redraw_guard.rs` (shared illegal-size predicate for #188; unit-tested on every OS)
- `src/platform_impl/macos/mod.rs` (`pub(crate) use` of the drawRect panic hook)
- `src/platform_impl/mod.rs` (registers the two MistTerm modules)
- `src/platform/macos.rs` (`set_caught_draw_rect_panic_hook` for #188; base file = upstream 0.30.13)

## Why

While an IME preedit is active, Control chords that the IME does not use for editing (Ctrl+C, Ctrl+R, Ctrl+K, …) never reached MistTerm. Shell SIGINT and reverse-i-search died during Chinese composition. egui only receives what winit emits, so this cannot be fixed in the app alone.

Letters, Space, Backspace, arrows, and digits stay on the IME path. The preedit is left active; cancelling it is still the IME's job (Escape / Backspace). Candidate lists stay with the system IME panel.

S1 (#206) re-vendored a **clean** 0.30.13 tree and deferred this port to S3 (#208). Until this patch lands, main/v1.3.4 does **not** ship IME Control bypass or #188 vendor hardening (and `platform/macos.rs` briefly carried a broken 0.28 hook stub).

## X11

On X11, while `is_composing`:

1. `XFilterEvent` may swallow the key before the application sees it.
2. Even when the filter returns false, winit skips `KeyboardInput` for every non-zero keycode during composition.

If Control is already held and the key is not itself a modifier, do not give the event to `XFilterEvent`, and still emit `KeyboardInput` during preedit. Do not pass that chord through `lookup_utf8` / XIM. `keycode == 0` stays with XIM (synthetic commit).

Wayland does not use the same `is_composing` guard. If a compositor delivers the key, MistTerm forwards it; if the compositor eats it, this patch cannot see it. Wayland Control bypass is a non-blocking follow-up (see #208).

## macOS

macOS does not use `XFilterEvent`. `WinitView::keyDown` calls `interpretKeyEvents`, which may call `setMarkedText`, `insertText`, or `doCommandBySelector`. `KeyboardInput` is emitted only when `!had_ime_input || forward_key_to_app`.

SCIM 简体拼音 does not take the `doCommandBySelector` path for Ctrl+C / Ctrl+R during preedit. It clears marked text via `setMarkedText("")` / `unmarkText` and never sets `forward_key_to_app`. winit treats the state change as IME input and drops the key.

Fix, same policy as X11 (`ime_bypass::macos_skip_interpret_key_events`):

1. While IME is allowed and marked text is showing (or `ImeState::Preedit`), if Control is held and the key is not a modifier, **do not call `interpretKeyEvents`**.
2. Set `forward_key_to_app` so `keyDown` still emits `KeyboardInput`.
3. Do **not** clear `marked_text` and do **not** emit `Ime::Preedit("", None)`.
4. Temporarily move `ImeState::Preedit` to `Ground` so `keyUp` is delivered. On that `keyUp`, if marked text is still present, return to `Preedit`.

## macOS drawRect panic_cannot_unwind (#188)

Lock-screen / Spaces tile-resize can synchronously enter `WinitView::drawRect:` while glow/eframe still sees a 0 or absurd framebuffer size. A panic there crosses the ObjC FFI boundary and becomes `panic_cannot_unwind` → abort.

1. **Size gate** — before emitting `WindowEvent::Resized` (`frameDidChange:`) and before `ApplicationDelegate::handle_redraw` (`drawRect:`), drop / skip frames whose physical size is 0 or above 16384 on either axis (`redraw_guard::is_sane_redraw_physical_size`). Log a warn and `queue_redraw` so a later sane frame recovers.
2. **`catch_unwind`** — wrap `handle_redraw` inside `drawRect:`. On panic: log an error, invoke the optional `set_caught_draw_rect_panic_hook` (MistTerm writes a caught-in-FFI crash note), and `queue_redraw`.
3. Public hook in `platform/macos.rs` calls `crate::platform_impl::set_caught_draw_rect_panic_hook` (re-exported from `macos/view.rs` via `macos/mod.rs` + `platform_impl::pub use platform::*`).

## How to rebase

1. Extract stock `winit-0.30.x` from crates.io (or tag `rust-windowing/winit`).
2. Replace `vendor/winit` with that tree (keep this `MISTTERM_PATCH.md`).
3. Re-apply by **selector / call-site anchors**, not line cherry-picks from 0.28:
   - X11: `process_xevent` filter path + `xinput_key_input` composing guard
   - macOS: `frameDidChange:`, `drawRect:`, `keyDown:`, `keyUp:`
4. Restore `ime_bypass.rs` / `redraw_guard.rs` modules + `platform/macos.rs` hook.
5. Run `cargo test -p winit --lib` (predicates) and MistTerm `control_chords_bypass_ime_*` / crash_reporter note tests.
6. macOS R1/R2 lock-screen samples remain on #210 (S5).

## Out of scope

- Switching the runtime renderer to wgpu
- Wayland-specific Control bypass
- Deleting this vendor path while the patches above are still required
