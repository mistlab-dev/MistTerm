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

- Terminal view requests periodic repaint while connected (for live updates).
- This is required for dynamic TUI/CLI apps even when input is idle.

## Font & Color Behavior

- Monospace rendering uses egui monospace font path.
- CJK fallback is placed after monospace primary font to reduce non-monospace appearance.
- Cell foreground colors are rendered from terminal color attributes (including indexed/truecolor paths).

## Regression Checklist

Before release, verify:

1. `top` refreshes continuously and columns stay aligned.
2. `vim` can enter/exit insert mode and redraw properly.
3. `ls --color=always` shows directory/file colors (e.g. directories in blue).
4. Mouse text selection works inside terminal area.
5. `⌘V` pastes without auto-execution.
6. `Tab` triggers shell completion and does not move UI focus.
