# MistTerm patch on egui-winit 0.23.0

Upstream crate: `egui-winit 0.23.0`. The only change is `WindowEvent::Ime` handling in `src/lib.rs`.

winit 0.28 ends an X11 preedit with `Ime::Preedit("", None)`. Stock egui-winit 0.23 ignores that event, so MistTerm never learned the preedit was cancelled (Escape) and kept drawing it. An empty `CompositionUpdate` clears the overlay and does not write the PTY. A real commit is still `Ime::Commit` → `CompositionEnd`.
