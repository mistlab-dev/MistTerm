#!/usr/bin/env python3
"""MistTerm Windows GUI full feature walkthrough (egui menus + shortcuts + bottom bar)."""

from __future__ import annotations

import argparse
import ctypes
import subprocess
import sys
import time
from dataclasses import dataclass, field

from pywinauto import Application, Desktop
from pywinauto.keyboard import send_keys


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", ctypes.c_long),
        ("top", ctypes.c_long),
        ("right", ctypes.c_long),
        ("bottom", ctypes.c_long),
    ]


user32 = ctypes.windll.user32
MENU_X = [16, 82, 132, 178, 228]  # Terminal, Edit, View, Tools, Help (en UI ~1200px)
ITEM_H = 28
SEP_H = 8


@dataclass
class Report:
    passed: list[str] = field(default_factory=list)
    skipped: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  [OK] {name}", flush=True)

    def skip(self, name: str, reason: str) -> None:
        self.skipped.append(f"{name}: {reason}")
        print(f"  [SKIP] {name} — {reason}", flush=True)

    def fail(self, name: str, err: str) -> None:
        self.failed.append((name, err))
        print(f"  [FAIL] {name} — {err}", flush=True)

    def summary(self) -> int:
        total = len(self.passed) + len(self.skipped) + len(self.failed)
        print("\n=== GUI walkthrough summary ===", flush=True)
        print(f"  passed : {len(self.passed)}", flush=True)
        print(f"  skipped: {len(self.skipped)}", flush=True)
        print(f"  failed : {len(self.failed)}", flush=True)
        print(f"  total  : {total}", flush=True)
        if self.failed:
            print("\nFailures:", flush=True)
            for name, err in self.failed:
                print(f"  - {name}: {err}", flush=True)
        if self.skipped:
            print("\nSkipped:", flush=True)
            for s in self.skipped:
                print(f"  - {s}", flush=True)
        return 1 if self.failed else 0


def client_screen_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y, pt.x + rect.right, pt.y + rect.bottom


def find_mist_hwnd(title_sub: str, timeout: float) -> int:
    deadline = time.time() + timeout
    while time.time() < deadline:
        for w in Desktop(backend="uia").windows():
            if title_sub in w.window_text():
                return int(w.handle)
        time.sleep(0.2)
    raise RuntimeError(f"window '{title_sub}' not found")


def click_xy(x: int, y: int) -> None:
    user32.SetCursorPos(x, y)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.12)


class GuiWalker:
    def __init__(self, proc: subprocess.Popen[bytes], hwnd: int, report: Report):
        self.proc = proc
        self.hwnd = hwnd
        self.report = report
        self.app = Application(backend="uia").connect(process=proc.pid)
        self.win = self.app.window(handle=hwnd)
        cl, ct, cr, cb = client_screen_rect(hwnd)
        self.cl, self.ct, self.cr, self.cb = cl, ct, cr, cb
        self.cw, self.ch = cr - cl, cb - ct
        self.scale = max(0.85, min(1.35, self.cw / 1200.0))
        self.menu_y = ct + int(16 * self.scale)
        self.status_y = cb - int(18 * self.scale)

    def alive(self) -> bool:
        return self.proc.poll() is None

    def focus(self) -> None:
        if not self.alive():
            raise RuntimeError("process exited")
        try:
            self.win.set_focus()
        except Exception:
            click_xy(self.cl + int(80 * self.scale), self.menu_y)
        time.sleep(0.25)

    def dismiss(self, times: int = 3) -> None:
        if not self.alive():
            return
        for _ in range(times):
            try:
                self.focus()
            except Exception:
                pass
            send_keys("{ESC}")
            time.sleep(0.18)

    def check(self, name: str) -> None:
        if not self.alive():
            raise RuntimeError("process exited")

    def menu_x(self, index: int) -> int:
        return self.cl + int(MENU_X[index] * self.scale)

    def open_menu(self, index: int) -> int:
        x = self.menu_x(index)
        click_xy(x, self.menu_y)
        time.sleep(0.3)
        return x

    def pick_item(self, menu_x: int, row: int, extra_y: int = 0) -> None:
        y = self.menu_y + int((row * ITEM_H + extra_y) * self.scale)
        click_xy(menu_x, y)
        time.sleep(0.35)

    def shortcut(self, keys: str) -> None:
        self.focus()
        send_keys(keys)
        time.sleep(0.45)

    def bottom_btn_from_right(self, offset: int) -> None:
        x = self.cr - int(offset * self.scale)
        click_xy(x, self.status_y)
        time.sleep(0.35)

    def run_step(self, name: str, action) -> None:
        try:
            action()
            self.check(name)
            self.dismiss()
            self.report.ok(name)
        except Exception as e:
            self.dismiss(3)
            self.report.fail(name, str(e))

    def walk(self) -> None:
        # ── Keyboard shortcuts ──
        self.run_step("shortcut Ctrl+N (new session)", lambda: self.shortcut("^n"))
        self.run_step("shortcut Ctrl+T (new tab)", lambda: self.shortcut("^t"))
        self.run_step("shortcut Ctrl+J (sidebar search)", lambda: self.shortcut("^j"))
        self.run_step("shortcut Ctrl+K (fragment search)", lambda: self.shortcut("^k"))
        self.run_step(
            "shortcut Ctrl+Shift+A (AI panel)",
            lambda: self.shortcut("+^a"),
        )
        self.run_step("shortcut Ctrl+H (about)", lambda: self.shortcut("^h"))
        self.run_step("shortcut Ctrl+, (preferences)", lambda: self.shortcut("^,"))
        self.run_step("shortcut F3 (terminal find)", lambda: self.shortcut("{F3}"))
        self.run_step(
            "shortcut Ctrl+Shift+D (split horizontal)",
            lambda: self.shortcut("+^d"),
        )
        self.run_step(
            "shortcut Ctrl+Shift+U (split vertical)",
            lambda: self.shortcut("+^u"),
        )
        self.run_step("shortcut Ctrl+Tab (next tab)", lambda: self.shortcut("^{TAB}"))
        self.run_step(
            "shortcut Ctrl+Shift+Tab (prev tab)",
            lambda: self.shortcut("+^{TAB}"),
        )

        # ── Terminal menu ──
        def terminal_import():
            x = self.open_menu(0)
            self.pick_item(x, 2)

        self.run_step("menu Terminal > Import SSH Config", terminal_import)

        # Preferences: use shortcut only (menu row near Quit — mis-click risk)

        # ── Edit menu ──
        def edit_find():
            x = self.open_menu(1)
            self.pick_item(x, 3, SEP_H)

        self.run_step("menu Edit > Find in Terminal", edit_find)

        for label, row in [("Copy", 0), ("Paste", 1), ("Select All", 2)]:
            self.run_step(
                f"menu Edit > {label}",
                lambda r=row: self.pick_item(self.open_menu(1), r),
            )

        # ── View menu (panels + window) ──
        view_items = [
            ("toggle sidebar", 0),
            ("maximize/restore window", 1),
            ("SFTP panel", 2),
            ("Port forwarding panel", 3),
            ("Fragment panel", 4),
            ("Monitor panel", 5),
            ("AI panel", 6),
        ]
        extra = SEP_H
        for name, row in view_items:
            self.run_step(
                f"menu View > {name}",
                lambda r=row, e=extra: self.pick_item(self.open_menu(2), r, e),
            )

        def view_theme():
            x = self.open_menu(2)
            # Theme submenu row; pick first theme to the right
            y = self.menu_y + int((7 * ITEM_H + SEP_H * 2) * self.scale)
            click_xy(x, y)
            time.sleep(0.3)
            click_xy(x + int(110 * self.scale), y + int(28 * self.scale))

        self.run_step("menu View > Theme (switch)", view_theme)

        # ── Tools menu ──
        tools_rows = [
            ("AI Settings", 0),
            ("Fragment Library", 1),
            ("Quick Fragment Picker", 2),
            ("Command History", 3),
            ("Batch Run on Servers", 4),
            ("Credentials", 5),
            ("Team Account", 6),
            ("Cloud Sync", 7),
            ("Browse Session Logs", 8),
        ]
        tools_extra = 0
        for name, row in tools_rows:
            if row >= 5:
                tools_extra = SEP_H
            if row >= 8:
                tools_extra = SEP_H * 2

            def action(r=row, e=tools_extra, n=name):
                x = self.open_menu(3)
                self.pick_item(x, r, e)

            self.run_step(f"menu Tools > {name}", action)

        # ── Help menu ──
        for name, row, extra in [
            ("Quick Start", 0, 0),
            ("Keyboard Shortcuts", 1, 0),
        ]:
            self.run_step(
                f"menu Help > {name}",
                lambda r=row, e=extra: self.pick_item(self.open_menu(4), r, e),
            )

        self.report.skip("menu Help > About Mist", "covered by Ctrl+H shortcut")

        self.report.skip("menu Help > Online Documentation", "opens external browser")
        self.report.skip("menu Help > Report an Issue", "opens external browser")

        # ── Bottom status bar (right → left: AI, Monitor, Forward, Files, Snippets) ──
        for name, off in [
            ("AI", 24),
            ("Monitor", 58),
            ("Port Forward", 92),
            ("SFTP Files", 126),
            ("Snippets", 160),
        ]:
            self.run_step(
                f"bottom bar > {name}",
                lambda o=off: self.bottom_btn_from_right(o),
            )

        # ── Status bar context menu (right-click) ──
        def status_context():
            x = self.cl + int(self.cw * 0.5)
            y = self.status_y
            user32.SetCursorPos(x, y)
            user32.mouse_event(0x0008, 0, 0, 0, 0)  # RIGHTDOWN
            user32.mouse_event(0x0010, 0, 0, 0, 0)  # RIGHTUP
            time.sleep(0.35)
            click_xy(x, y - int(28 * self.scale))

        self.run_step("status bar context > Import SSH", status_context)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--title", default="Mist")
    parser.add_argument("--timeout", type=float, default=20.0)
    args = parser.parse_args()

    report = Report()
    print(f"==> Launching {args.exe}", flush=True)
    proc = subprocess.Popen([args.exe])
    try:
        hwnd = find_mist_hwnd(args.title, args.timeout)
        print(f"    hwnd={hwnd} pid={proc.pid}", flush=True)
        walker = GuiWalker(proc, hwnd, report)
        walker.focus()
        time.sleep(0.8)
        print("==> Feature walkthrough", flush=True)
        walker.walk()
        return report.summary()
    finally:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        print(f"FATAL: {e}", file=sys.stderr)
        sys.exit(2)
