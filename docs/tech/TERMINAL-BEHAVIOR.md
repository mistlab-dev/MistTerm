# Terminal Behavior Notes

本文档记录 MistTerm 当前终端交互的既定行为，避免后续改动引入回归。

## VT/ANSI Rendering

- Terminal parser/runtime uses `alacritty_terminal`.
- SSH output is fed directly into terminal emulator state.
- UI renders emulator screen content (not sanitized plain text fallback).
- Dynamic programs like `top`, `vim`, `less` rely on terminal semantics and should update continuously.

## Input Behavior

- `Tab` / `Shift+Tab` are intercepted and sent to PTY as `\t`.
- `Tab` focus traversal in UI is suppressed while terminal is connected/active.
- `Enter` sends `\r`.
- `Backspace` sends `0x7f`.
- `Delete` sends `\x1b[3~` (xterm-compatible forward delete).
- `Ctrl+C` sends `0x03`, `Ctrl+D` sends `0x04`. A selection does not turn Ctrl+C into copy.
- `Ctrl+J` (`0x0a`) and `Ctrl+K` (`0x0b`) go to the PTY while the terminal pane is focused. Connection search (`Ctrl/⌘+J`) and the command palette (`Ctrl/⌘+K`) use those chords only when the terminal is not focused. Chord decision: unfocused Ctrl+K does not conflict with the OS, so Windows/Linux stay on Ctrl+K and macOS stays on ⌘K; Ctrl+Shift+K is not used. While the palette is open it is a blocking modal, so keys are not forwarded to the PTY. The quick snippet picker remains `Ctrl+Shift+J` / `⌘⇧J`, and only when the terminal is not focused.
- `Ctrl+R` goes to the PTY while the terminal is focused (shell reverse-i-search). When the terminal is not focused it opens Mist command history.
- While the terminal pane is focused, Alt (macOS: Option) plus a letter, digit, or ASCII punctuation is a Meta sequence: ESC then that character. `Alt+B` / `Alt+F` move by word, `Alt+D` deletes the next word, `Alt+.` inserts the previous command's last argument. `Alt+Backspace` is ESC DEL (`0x1b 0x7f`), readline backward-kill-word. Ctrl+Alt and ⌘⌥ are not Meta (AltGr, local shell, pane focus).
- On macOS the window uses winit `OptionAsAlt::Both`, so Option delivers the base character with Alt set. The terminal prefixes ESC. There is no key-rebinding UI.

## IME

- Committed text (`CompositionEnd`) is written to the PTY. The preedit string is drawn at the block cursor and is not sent until commit.
- The candidate list is owned by the system IME panel. egui 0.23 / winit 0.28 preedit events do not include candidates.
- On X11, Control chords during preedit bypass `XFilterEvent` and are still delivered as key events (see `vendor/winit/MISTTERM_PATCH.md`), so Ctrl+C / Ctrl+R reach the PTY while composing. Letters, Space, Backspace, and arrows stay with the IME. The preedit is not cancelled by those chords.
- On macOS the same policy skips `interpretKeyEvents` while marked text is showing, so SCIM does not clear the preedit and swallow Ctrl+C / Ctrl+R. `keyUp` is still delivered. The system candidate panel stays with the IME.

## Copy / Paste / Selection

- Terminal viewport supports mouse selection.
- **Known limitation:** Alt+click does not move the cursor. MistTerm does not send xterm mouse-tracking reports to the PTY, so tmux mouse mode (and click-to-position inside the shell) is unavailable. Mouse input is selection and scrolling only.
- Copy uses the selection plus macOS `⌘C` or Windows/Linux `Ctrl+Shift+C`, or the context menu. A selection does not turn `Ctrl+C` into copy.
- Paste is macOS `⌘V` or Windows/Linux `Ctrl+Shift+V`, or the context menu. Raw text goes to the PTY.
- Paste does **not** auto-append newline; pasted content is not auto-executed.
- Command fragments use explicit `send_command` path and may execute with newline by design.

## Refresh Behavior

- Connected terminal requests periodic repaint for live TUI/CLI (`top`, `vim`, …).
- Live repaint interval: **~33ms (30 FPS)** (`TERMINAL_LIVE_REPAINT_MS`) — common for desktop terminals when content is dirty; not 60 FPS continuous (wastes CPU) and not ≤20 FPS (feels sluggish).
- Cursor blink period: **~530ms** (xterm-like).
- SSH shell pump poll: **~8ms** non-blocking timeout (latency-oriented; fine for interactive SSH).
- UI hang watchdog: **~3s** stale heartbeat threshold (2–5s is typical for desktop diagnostics).
- Toast auto-dismiss: Info/Success **5s**, Warn **7s**, Error **8s** (readable without lingering).

## Font & Color Behavior

- Monospace rendering uses egui monospace font path (system Regular preferred; never Bold font files as default).
- CJK fallback is placed after monospace primary font to reduce non-monospace appearance.
- Cell foreground colors are rendered from terminal color attributes (including indexed/truecolor paths).
- Bold uses classic ANSI brightening only for indexed/named base colors `0..7`; default foreground and truecolor are not forced toward white.
- Line height is clamped to about `1.1 × font_size` when the font’s egui `row_height` (includes line_gap) is looser, reducing the “glyph stuck to top of cell” look; terminal mono fonts get a slight `y_offset` tweak to sit nearer mid-cell.
- VT grid is laid out **top-aligned** in the viewport after syncing PTY rows to the visible height first.
- Cell / selection backgrounds are painted by MistTerm (row-clamped, no egui `expand(1)`); `TextFormat.background` stays transparent so highlights cannot crush the next line.
- Block cursor uses font row height and `TextEdit`’s real `text_draw_pos` (top-aligned with glyphs).

## Regression Checklist

Before release, verify:

1. `top` refreshes continuously and columns stay aligned.
2. `vim` can enter/exit insert mode and redraw properly.
3. `ls --color=always` shows directory/file colors (e.g. directories in blue).
4. Mouse text selection works inside terminal area.
5. `⌘V` pastes without auto-execution.
6. `Tab` triggers shell completion and does not move UI focus.
