# MistTerm patch on winit 0.28.7

Upstream crate: `winit 0.28.7` (unchanged except `src/platform_impl/linux/x11/event_processor.rs`).

## Why

On X11, while an IME preedit is active (`is_composing`):

1. `XFilterEvent` may swallow the key before the application sees it.
2. Even when the filter returns false, winit skips `KeyboardInput` for every non-zero keycode.

ibus + libpinyin does not use Control chords (Ctrl+C, Ctrl+R, Ctrl+K, …) to edit the preedit, but those keys never reached MistTerm, so SIGINT and reverse-i-search died during Chinese composition. egui 0.23 only receives what winit emits, so this cannot be fixed in the app alone.

## Policy

If Control is already held and the key is not itself a modifier, do not give the event to `XFilterEvent`, and still emit `KeyboardInput` during preedit. Do not pass that chord through `Xutf8LookupString` (that would hand it back to XIM). Letters, Space, Backspace, arrows, and digits stay on the IME path. The preedit is left active; cancelling it is still the IME's job (Escape / Backspace).

Candidate lists are not part of the XIM preedit callback. The system IME panel still owns them.

Wayland in this winit version does not use the same `is_composing` guard. If a compositor delivers the key, MistTerm forwards it; if the compositor eats it, this patch cannot see it.
