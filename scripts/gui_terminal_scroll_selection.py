#!/usr/bin/env python3
"""终端 UI 回归：滚动视口、输入回到底部、剪贴板快捷键。"""

from __future__ import annotations

import argparse
import ctypes
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_common import (
    LOCAL_TEST_SESSION,
    automation_env,
    capture_failure,
    clipboard_get,
    clipboard_set,
    click,
    connect_local_session,
    graceful_stop_mist_process,
    remote_assert_file,
    remote_exec,
    remote_temp_path,
    remote_text_file_contains,
    send_terminal_line,
    ssh_preflight,
    stop_existing_mist_processes,
)
from gui_screen import client_rect, find_mist_window
from pywinauto.keyboard import send_keys
from smoke_gui_interact import GuiWalker, Report

user32 = ctypes.windll.user32
MOUSEEVENTF_WHEEL = 0x0800
WHEEL_DELTA = 120


@dataclass
class UiReport:
    passed: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  [OK] {name}", flush=True)

    def fail(self, name: str, err: str) -> None:
        safe = err.encode("ascii", errors="replace").decode("ascii")[:240]
        self.failed.append((name, safe))
        print(f"  [FAIL] {name} — {safe}", flush=True)

    def summary(self) -> int:
        print("\n=== Terminal scroll/selection UI summary ===", flush=True)
        print(f"  passed: {len(self.passed)}", flush=True)
        print(f"  failed: {len(self.failed)}", flush=True)
        if self.failed:
            for name, err in self.failed:
                print(f"  - {name}: {err}", flush=True)
        return 1 if self.failed else 0


def wheel_scroll(*, lines: int = 3, direction: str = "up") -> None:
    sign = 1 if direction == "up" else -1
    for _ in range(max(1, lines)):
        user32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, sign * WHEEL_DELTA, 0)
        time.sleep(0.12)


def prep_terminal(walker: GuiWalker) -> None:
    walker.dismiss(2)
    walker.focus_terminal()
    send_keys("{VK_CONTROL up}{VK_SHIFT up}{VK_MENU up}")
    time.sleep(0.15)


def clear_terminal_selection(walker: GuiWalker) -> None:
    """单击终端清除选区，避免影响后续「复制全部」。"""
    cl, ct, cr, cb = client_rect(walker.hwnd)
    x = cl + int((cr - cl) * 0.45)
    y = cb - int((cb - ct) * 0.12)
    click(x, y, pause=0.2)


def menu_copy_terminal(walker: GuiWalker) -> None:
    walker.focus()
    mx = walker.open_menu(1)
    walker.pick_item(mx, 0)
    time.sleep(0.75)


def read_clipboard_marker(marker: str, *, retries: int = 10) -> str:
    clip = ""
    for _ in range(retries):
        clip = clipboard_get()
        if marker in clip:
            return clip
        time.sleep(0.25)
    return clip


def test_input_scrolls_back_to_bottom(walker: GuiWalker, report: UiReport) -> None:
    outfile = remote_temp_path("mistterm_scroll_input.txt")
    win_out = outfile.replace("/", "\\")
    remote_exec(f'del /q "{win_out}" 2>nul')
    marker = "MISTTERM_INPUT_BOTTOM"
    try:
        prep_terminal(walker)
        for i in range(28):
            send_terminal_line(f"echo SCROLL_FILL_{i:02d}")
            if i % 7 == 6:
                time.sleep(0.12)
        time.sleep(0.8)
        prep_terminal(walker)
        wheel_scroll(lines=8, direction="up")
        time.sleep(0.45)
        prep_terminal(walker)
        send_terminal_line(f"echo {marker}>{win_out}")
        time.sleep(2.0)
        if not remote_text_file_contains(outfile, marker):
            remote_assert_file(outfile, marker, what="input after scroll up")
        report.ok("typing scrolls back to live line")
    except Exception as e:
        report.fail("typing scrolls back to live line", str(e))


def test_scroll_viewport_and_restore(walker: GuiWalker, report: UiReport) -> None:
    """滚轮上翻后视口不再含最新行；任意输入滚回底部后视口恢复。"""
    marker = "MISTTERM_SCROLL_VIEW_OK"
    try:
        prep_terminal(walker)
        send_terminal_line(f"echo {marker}")
        time.sleep(1.2)
        clear_terminal_selection(walker)
        menu_copy_terminal(walker)
        if marker not in clipboard_get():
            raise RuntimeError("marker missing from viewport before scroll")
        prep_terminal(walker)
        wheel_scroll(lines=6, direction="up")
        time.sleep(0.45)
        clear_terminal_selection(walker)
        menu_copy_terminal(walker)
        clip_mid = clipboard_get()
        if marker in clip_mid:
            raise RuntimeError("marker still in viewport after scroll up")
        prep_terminal(walker)
        send_terminal_line("echo")
        time.sleep(1.0)
        clear_terminal_selection(walker)
        menu_copy_terminal(walker)
        if marker not in read_clipboard_marker(marker):
            raise RuntimeError("marker missing after input scrolls back to bottom")
        report.ok("scroll viewport + input restore live view")
    except Exception as e:
        report.fail("scroll viewport + input restore live view", str(e))


def test_menu_copy_after_echo(walker: GuiWalker, report: UiReport) -> None:
    marker = "MISTTERM_UI_COPY_OK"
    try:
        prep_terminal(walker)
        send_terminal_line(f"echo {marker}")
        time.sleep(1.2)
        clear_terminal_selection(walker)
        menu_copy_terminal(walker)
        if marker not in read_clipboard_marker(marker):
            raise RuntimeError(f"menu copy failed: {clipboard_get()[:120]!r}")
        report.ok("Edit menu copy (viewport)")
    except Exception as e:
        report.fail("Edit menu copy (viewport)", str(e))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--title", default="MistTerm")
    parser.add_argument("--timeout", type=float, default=25.0)
    args = parser.parse_args()

    report = UiReport()
    print("==> SSH preflight", flush=True)
    ssh_preflight()

    print("==> Stop lingering Mist windows from prior runs", flush=True)
    stop_existing_mist_processes()

    proc = subprocess.Popen([args.exe], env=automation_env())
    hwnd: int | None = None
    try:
        hwnd = find_mist_window(proc, timeout=args.timeout, title_sub=args.title)
        print(f"==> hwnd={hwnd} pid={proc.pid}", flush=True)
        walker = GuiWalker(proc, hwnd, Report())
        print(f"==> Connect: {LOCAL_TEST_SESSION}", flush=True)
        connect_local_session(hwnd, proc.pid, LOCAL_TEST_SESSION, wait=min(args.timeout, 15.0))
        time.sleep(1.0)

        print("==> UI: input returns to bottom", flush=True)
        test_input_scrolls_back_to_bottom(walker, report)
        print("==> UI: scroll viewport restore", flush=True)
        test_scroll_viewport_and_restore(walker, report)
        print("==> UI: menu copy", flush=True)
        test_menu_copy_after_echo(walker, report)

        return report.summary()
    except Exception as e:
        capture_failure(hwnd, "terminal_scroll_selection")
        report.fail("fatal", str(e))
        return report.summary()
    finally:
        graceful_stop_mist_process(proc, hwnd)


if __name__ == "__main__":
    sys.exit(main())
