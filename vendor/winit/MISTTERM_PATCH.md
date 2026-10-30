# MistTerm patch on winit 0.28.7

Upstream crate: `winit 0.28.7`.

Changed files:

- `src/platform_impl/linux/x11/event_processor.rs` (X11)
- `src/platform_impl/macos/view.rs` (macOS)
- `src/platform_impl/ime_bypass.rs` (shared Control-chord policy, used by macOS; unit-tested on every OS)
- `src/platform_impl/redraw_guard.rs` (shared illegal-size predicate for #188; unit-tested on every OS)
- `src/platform/macos.rs` (`set_caught_draw_rect_panic_hook` for #188)

## Why

While an IME preedit is active, Control chords that the IME does not use for editing (Ctrl+C, Ctrl+R, Ctrl+K, …) never reached MistTerm. Shell SIGINT and reverse-i-search died during Chinese composition. egui 0.23 only receives what winit emits, so this cannot be fixed in the app alone.

Letters, Space, Backspace, arrows, and digits stay on the IME path. The preedit is left active; cancelling it is still the IME's job (Escape / Backspace). Candidate lists stay with the system IME panel.

## X11

On X11, while `is_composing`:

1. `XFilterEvent` may swallow the key before the application sees it.
2. Even when the filter returns false, winit skips `KeyboardInput` for every non-zero keycode.

If Control is already held and the key is not itself a modifier, do not give the event to `XFilterEvent`, and still emit `KeyboardInput` during preedit. Do not pass that chord through `Xutf8LookupString` (that would hand it back to XIM). `keycode == 0` stays with XIM (synthetic commit).

Wayland in this winit version does not use the same `is_composing` guard. If a compositor delivers the key, MistTerm forwards it; if the compositor eats it, this patch cannot see it.

## macOS

macOS does not use `XFilterEvent`. `WinitView::keyDown` calls `interpretKeyEvents`, which may call `setMarkedText`, `insertText`, or `doCommandBySelector`. `KeyboardInput` is emitted only when `!had_ime_input || forward_key_to_app`.

`doCommandBySelector` sets `forward_key_to_app` (and drops to `ImeState::Ground` so `keyUp` is reported). SCIM 简体拼音 does not take that path for Ctrl+C / Ctrl+R during preedit. It clears marked text via `setMarkedText("")` / `unmarkText` and never sets `forward_key_to_app`. winit treats the state change as IME input and drops the key. The preedit disappears and the PTY sees nothing. That matches the Mac smoke failure: Ctrl+C only cancels the preedit.

Fix, same policy as X11 (`ime_bypass::macos_skip_interpret_key_events`):

1. While IME is allowed and marked text is showing (or `ImeState::Preedit`), if Control is held and the key is not a modifier, **do not call `interpretKeyEvents`**.
2. Set `forward_key_to_app` so `keyDown` still emits `KeyboardInput` (egui turns that into `Event::Key`; Ctrl+C is `0x03`).
3. Do **not** clear `marked_text` and do **not** emit `Ime::Preedit("", None)`. The overlay and the system candidate panel stay.
4. Temporarily move `ImeState::Preedit` to `Ground` so `keyUp` is delivered (`keyUp` is ignored in `Preedit`). On that `keyUp`, if marked text is still present, return to `Preedit` so the next letter still composes and a following Ctrl chord still bypasses.

Modifier keys themselves arrive as `flagsChanged`, not `keyDown`, and are not bypassed. Command-only chords (⌘C copy) are unchanged: the bypass requires Control.

`retrieve_keycode` maps Ctrl+C through `charactersIgnoringModifiers` (`c`) after `characters()` (`\u{3}`) fails `char_to_keycode`. egui-winit drops `ReceivedCharacter` while Control is down on macOS, so the chord is not also inserted as text.

## macOS drawRect panic_cannot_unwind (#188)

Lock-screen / Spaces tile-resize can synchronously enter `WinitView::drawRect:` while
glow/eframe still sees a 0 or absurd framebuffer size. A panic there crosses the ObjC
FFI boundary (`extern "C"` / `#[sel(...)]`) and becomes `panic_cannot_unwind` → abort.

MistTerm hotfix (scheme A; does **not** bump eframe/egui/winit):

1. **Size gate** — before emitting `WindowEvent::Resized` (`frameDidChange:`) and before
   `AppState::handle_redraw` (`drawRect:`), drop / skip frames whose physical size is
   0 or above 16384 on either axis (`redraw_guard::is_sane_redraw_physical_size`). Log a
   warn and `queue_redraw` so a later sane frame recovers.
2. **`catch_unwind`** — wrap `AppState::handle_redraw` inside `drawRect:`. On panic: log
   an error, invoke the optional `set_caught_draw_rect_panic_hook` (MistTerm writes a
   caught-in-FFI crash note), and `queue_redraw`. The Rust panic hook still runs before
   the catch completes, so the usual `crash-*.log` backtrace is preserved while the
   process stays alive.
3. **Out of scope for this patch** — upgrading eframe/egui/winit, switching the runtime
   renderer to wgpu.

