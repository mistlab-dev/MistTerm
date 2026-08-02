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
- `Ctrl+C` sends `0x03`, `Ctrl+D` sends `0x04`.

## Copy / Paste / Selection

- Terminal viewport supports mouse selection.
- Copy uses system selection + `⌘C` / context menu.
- Paste (`⌘V` / context menu) sends raw text to PTY.
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
