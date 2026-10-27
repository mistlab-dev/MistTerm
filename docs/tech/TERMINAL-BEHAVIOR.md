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

## Command blocks (OSC 133)

Shell integration markers divide scrollback into command blocks. The UI does not guess a block from Enter or from a prompt line. Without these markers, previous/next and “copy this command block” do nothing to the viewport and do not invent a range around vim, less, or htop.

Parsed subset (BEL `0x07` or ST `ESC \`):

| Marker | Sequence | Meaning |
|--------|----------|---------|
| A | `OSC 133 ; A` or `OSC 633 ; A` | Prompt start |
| B | `OSC 133 ; B` or `OSC 633 ; B` | Prompt end, command input starts |
| C | `OSC 133 ; C` or `OSC 633 ; C` | Command submitted, output starts (optional) |
| D | `OSC 133 ; D [; exit]` or `OSC 633 ; D [; exit]` | Command finished. First decimal parameter is the exit code; extra parameters such as `aid=` are ignored |

Not parsed: OSC 1337 (iTerm properties), `OSC 133 ; P`, title OSC 0/2. OSC 633 is included because VS Code shell integration uses the same A/B/C/D letters. The interactive-exec audit (#156) may still read a `D` exit code; that path is not a UI block boundary.

A block is navigable only after `C` or `D`. A bare prompt (`A`, or `A`+`B` waiting for input) is not a command, so “previous” does not land on the empty prompt. Copy starts at `B` when present (so the prompt is left out) and ends at `D`, or at the cursor while the command is still running. The next `A` closes an open block without marking it finished.

Coordinates live on the primary grid and shift when rows enter scrollback. The alternate screen is not sampled. A command that enters the alternate screen (`CSI ?1049 h`, vim/less/htop) is one block flagged fullscreen; its copy is the command on the primary grid, not the fullscreen frame. Column resize drops stored coordinates (integration stays “seen”). Clearing history drops blocks that end above the new top line.

Copy is bounded: at most **4000** grid lines and **200000** characters, keeping the head. The suffixes are `\n… [truncated]` (line cap) and `\n… [truncated, N chars omitted]` (character cap).

Keyboard (consumed before the key is forwarded to the PTY): previous **Ctrl+Shift+↑** (macOS **⌘⇧↑**), next **Ctrl+Shift+↓** (macOS **⌘⇧↓**), copy **Ctrl+Shift+O** (macOS **⌘⇧O**). The terminal context menu shows the same three actions even when integration is off, so the degrade is discoverable: a toast points at the manual and the shortcut is not sent to the shell. The shortest enable path is in the user manual, section “Command blocks”.

## Refresh Behavior

- Connected terminal requests a fast repaint for live TUI/CLI (`top`, `vim`, `yes`, …) **only while the VTE is dirty or SSH output from this frame has not been painted yet**.
- Live repaint interval: **~33ms (30 FPS)** (`TERMINAL_LIVE_REPAINT_MS`) — common for desktop terminals when content is dirty; not 60 FPS continuous (wastes CPU) and not ≤20 FPS (feels sluggish). A quiet prompt does not stay on this timer.
- SSH output also calls `request_repaint()` from the shell pump as soon as bytes are queued, so new output is not waiting on the cursor timer. Ctrl+C during `top` / `yes` still wakes on that path.
- ZMODEM upload PTY capture requests an immediate repaint each frame so the UI-side `mpsc` bypass does not stall waiting for ZACK. Transfer progress events also request a repaint when they arrive.
- Idle + focused + connected, with no selection and no IME preedit: the block cursor schedules `request_repaint_after` for the **time until the next blink phase**, not a fixed 60ms (~16 FPS).
- Each blink phase lasts `TERMINAL_CURSOR_BLINK_PERIOD_SECS` (**0.53s**, xterm-like). The cursor is drawn on even phases and hidden on odd phases. The idle wait is whatever remains in the current phase (about **0–530ms**). Halfway through a phase that is about **265ms**. Just after a flip it is about **530ms** plus a few milliseconds of slack so the timer lands in the new phase.
- The output-to-frame probe is cleared once that frame has painted the terminal, including a plain prompt with no non-default cell background. Leaving the probe set used to keep the 33ms path running after the screen went quiet.
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
