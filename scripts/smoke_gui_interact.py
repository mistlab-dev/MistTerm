#!/usr/bin/env python3
"""MistTerm Windows GUI full feature walkthrough (egui menus + shortcuts + bottom bar)."""

from __future__ import annotations

import argparse
import ctypes
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import TOGGLE_SFTP, dismiss_new_session_dialog
from gui_common import automation_env, capture_failure, connect_local_session, ssh_preflight
from gui_coverage import CoverageTracker
from gui_screen import find_mist_window
from pywinauto import Application
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


def find_mist_hwnd(title_sub: str, timeout: float, proc: subprocess.Popen[bytes]) -> int:
    return find_mist_window(proc, timeout=timeout, title_sub=title_sub)


def click_xy(x: int, y: int) -> None:
    user32.SetCursorPos(x, y)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.12)


class GuiWalker:
    def __init__(
        self,
        proc: subprocess.Popen[bytes],
        hwnd: int,
        report: Report,
        coverage: CoverageTracker | None = None,
    ):
        self.proc = proc
        self.hwnd = hwnd
        self.report = report
        self.cov = coverage
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
        if os.environ.get("MISTTERM_GUI_AUTOMATION") == "1":
            try:
                self.focus()
            except Exception:
                pass
            dismiss_new_session_dialog(repeats=1)
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

    def run_step(self, name: str, action, *feature_ids: str) -> None:
        try:
            action()
            self.check(name)
            self.dismiss()
            self.report.ok(name)
            if self.cov and feature_ids:
                self.cov.mark_many(*feature_ids)
        except Exception as e:
            self.dismiss(3)
            self.report.fail(name, str(e))

    def walk(self) -> None:
        print("==> Connect Local Test SSH", flush=True)
        ssh_preflight()
        connect_local_session(self.hwnd, self.proc.pid)
        if self.cov:
            self.cov.mark("session.connect")

        # ── Keyboard shortcuts ──
        self.run_step(
            "shortcut Ctrl+N (new session)",
            lambda: self.shortcut("^n"),
            "session.new_dialog",
        )
        self.run_step(
            "shortcut Ctrl+Shift+Backspace (close new session)",
            lambda: dismiss_new_session_dialog(repeats=1),
            "automation.close_modal",
        )
        self.run_step("shortcut Ctrl+T (new tab)", lambda: self.shortcut("^t"), "tab.new")
        self.run_step("shortcut Ctrl+J (sidebar search)", lambda: self.shortcut("^j"))
        self.run_step("shortcut Ctrl+K (fragment search)", lambda: self.shortcut("^k"))
        self.run_step(
            "shortcut Ctrl+Shift+A (AI panel)",
            lambda: self.shortcut("+^a"),
            "panel.ai",
        )
        self.run_step(
            "shortcut Ctrl+Shift+S (SFTP panel)",
            lambda: self.shortcut(TOGGLE_SFTP),
            "sftp.toggle",
        )
        self.run_step("shortcut Ctrl+H (about)", lambda: self.shortcut("^h"), "dialog.about")
        self.run_step(
            "shortcut Ctrl+, (preferences)",
            lambda: self.shortcut("^,"),
            "dialog.preferences",
        )
        self.run_step(
            "shortcut F3 (terminal find)",
            lambda: self.shortcut("{F3}"),
            "terminal.find",
        )
        self.run_step(
            "shortcut Ctrl+R (command history)",
            lambda: self.shortcut("^r"),
            "terminal.history",
        )
        self.run_step(
            "shortcut Ctrl+E (edit session)",
            lambda: self.shortcut("^e"),
            "session.edit",
        )
        self.run_step(
            "shortcut Ctrl+Shift+L (send selection to AI)",
            lambda: self.shortcut("+^l"),
            "shortcut.ai_send_sel",
        )
        self.run_step(
            "shortcut Ctrl+Shift+D (split horizontal)",
            lambda: self.shortcut("+^d"),
            "terminal.split_h",
        )
        self.run_step(
            "shortcut Ctrl+Shift+U (split vertical)",
            lambda: self.shortcut("+^u"),
            "terminal.split_v",
        )
        self.run_step(
            "shortcut Alt+Left (pane focus)",
            lambda: self.shortcut("%{LEFT}"),
            "terminal.pane_focus",
        )
        self.run_step("shortcut Ctrl+Tab (next tab)", lambda: self.shortcut("^{TAB}"), "tab.cycle")
        self.run_step(
            "shortcut Ctrl+Shift+Tab (prev tab)",
            lambda: self.shortcut("+^{TAB}"),
            "tab.cycle",
        )

        # ── Terminal menu ──
        def terminal_import():
            x = self.open_menu(0)
            self.pick_item(x, 2)

        self.run_step("menu Terminal > Import SSH Config", terminal_import, "session.import_ssh")

        # Preferences: use shortcut only (menu row near Quit — mis-click risk)

        # ── Edit menu ──
        def edit_find():
            x = self.open_menu(1)
            self.pick_item(x, 3, SEP_H)

        self.run_step("menu Edit > Find in Terminal", edit_find, "menu.edit")

        for label, row in [("Copy", 0), ("Paste", 1), ("Select All", 2)]:
            self.run_step(
                f"menu Edit > {label}",
                lambda r=row: self.pick_item(self.open_menu(1), r),
                "menu.edit",
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
            fid = {
                "toggle sidebar": "menu.view",
                "maximize/restore window": "menu.view",
                "SFTP panel": "sftp.toggle",
                "Port forwarding panel": "panel.port_forward",
                "Fragment panel": "panel.snippets",
                "Monitor panel": "panel.monitor",
                "AI panel": "panel.ai",
            }.get(name, "menu.view")
            self.run_step(
                f"menu View > {name}",
                lambda r=row, e=extra: self.pick_item(self.open_menu(2), r, e),
                fid,
            )

        def view_theme():
            x = self.open_menu(2)
            # Theme submenu row; pick first theme to the right
            y = self.menu_y + int((7 * ITEM_H + SEP_H * 2) * self.scale)
            click_xy(x, y)
            time.sleep(0.3)
            click_xy(x + int(110 * self.scale), y + int(28 * self.scale))

        self.run_step("menu View > Theme (switch)", view_theme, "menu.view")

        # ── Tools menu ──
        tools_fids = {
            "AI Settings": "panel.ai_settings",
            "Fragment Library": "dialog.fragment_lib",
            "Quick Fragment Picker": "panel.snippets",
            "Command History": "terminal.history",
            "Batch Run on Servers": "dialog.batch_exec",
            "Credentials": "dialog.credentials",
            "Team Account": "dialog.team",
            "Cloud Sync": "dialog.cloud_sync",
            "Browse Session Logs": "dialog.session_logs",
        }
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

            def action(r=row, e=tools_extra):
                x = self.open_menu(3)
                self.pick_item(x, r, e)

            self.run_step(f"menu Tools > {name}", action, tools_fids.get(name, "menu.view"))

        # ── Help menu ──
        for name, row, extra in [
            ("Quick Start", 0, 0),
            ("Keyboard Shortcuts", 1, 0),
        ]:
            self.run_step(
                f"menu Help > {name}",
                lambda r=row, e=extra: self.pick_item(self.open_menu(4), r, e),
                "dialog.help",
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
            fid = {
                "AI": "panel.ai",
                "Monitor": "panel.monitor",
                "Port Forward": "panel.port_forward",
                "SFTP Files": "sftp.toggle",
                "Snippets": "panel.snippets",
            }[name]
            self.run_step(
                f"bottom bar > {name}",
                lambda o=off: self.bottom_btn_from_right(o),
                fid,
                "bar.bottom",
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

        self.run_step("status bar context > Import SSH", status_context, "bar.status_ctx")

        def terminal_close_tab():
            x = self.open_menu(0)
            self.pick_item(x, 3, SEP_H)

        self.run_step("menu Terminal > Close Tab", terminal_close_tab, "tab.close")

        connect_local_session(self.hwnd, self.proc.pid, wait=10.0)

        def terminal_disconnect():
            x = self.open_menu(0)
            self.pick_item(x, 4, SEP_H * 2)

        self.run_step("menu Terminal > Disconnect SSH", terminal_disconnect, "session.disconnect")

        def terminal_reconnect():
            x = self.open_menu(0)
            self.pick_item(x, 5, SEP_H * 2)

        self.run_step("menu Terminal > Reconnect Tab", terminal_reconnect, "session.reconnect")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--title", default="Mist")
    parser.add_argument("--timeout", type=float, default=90.0)
    args = parser.parse_args()

    report = Report()
    coverage = CoverageTracker("smoke")
    print(f"==> Launching {args.exe}", flush=True)
    proc = subprocess.Popen([args.exe], env=automation_env())
    hwnd: int | None = None
    try:
        hwnd = find_mist_hwnd(args.title, args.timeout, proc)
        print(f"    hwnd={hwnd} pid={proc.pid}", flush=True)
        walker = GuiWalker(proc, hwnd, report, coverage)
        walker.focus()
        time.sleep(0.8)
        print("==> Feature walkthrough", flush=True)
        walker.walk()
        code = report.summary()
        code = max(code, coverage.report())
        return code
    except Exception as e:
        capture_failure(hwnd, "menu_walkthrough")
        report.fail("fatal", str(e))
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
