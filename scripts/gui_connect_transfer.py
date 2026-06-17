#!/usr/bin/env python3
"""Launch Mist, connect to Local Test SSH, open SFTP panel (smoke)."""

from __future__ import annotations

import argparse
import ctypes
import subprocess
import sys
import time

from pywinauto import Application, Desktop
from pywinauto.keyboard import send_keys


user32 = ctypes.windll.user32


def find_hwnd(title: str, timeout: float) -> int:
    deadline = time.time() + timeout
    while time.time() < deadline:
        for w in Desktop(backend="uia").windows():
            if title in w.window_text():
                return int(w.handle)
        time.sleep(0.25)
    raise RuntimeError("Mist window not found")


def client_rect(hwnd: int) -> tuple[int, int, int, int]:
    class RECT(ctypes.Structure):
        _fields_ = [("l", ctypes.c_long), ("t", ctypes.c_long), ("r", ctypes.c_long), ("b", ctypes.c_long)]

    class POINT(ctypes.Structure):
        _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]

    rect = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y, pt.x + rect.r, pt.y + rect.b


def click(x: int, y: int) -> None:
    user32.SetCursorPos(x, y)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.2)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--session", default="Local Test SSH")
    parser.add_argument("--timeout", type=float, default=20.0)
    args = parser.parse_args()

    print(f"==> Launch {args.exe}")
    proc = subprocess.Popen([args.exe])
    try:
        hwnd = find_hwnd("Mist", args.timeout)
        app = Application(backend="uia").connect(process=proc.pid)
        win = app.window(handle=hwnd)
        win.set_focus()
        time.sleep(1.0)

        cl, ct, cr, cb = client_rect(hwnd)
        scale = max(0.85, min(1.35, (cr - cl) / 1200.0))

        # Sidebar session row (~y=120, x=center of sidebar ~100px)
        print(f"==> Double-click session '{args.session}'")
        send_keys("^j")
        time.sleep(0.4)
        send_keys(args.session.replace(" ", "{SPACE}"))
        time.sleep(0.5)
        send_keys("{ENTER}")
        time.sleep(0.8)
        # Double-click sidebar area to connect
        click(cl + int(100 * scale), ct + int(140 * scale))
        time.sleep(0.15)
        click(cl + int(100 * scale), ct + int(140 * scale))
        time.sleep(3.0)

        if proc.poll() is not None:
            raise RuntimeError("Mist exited during connect")

        # Bottom bar SFTP (Files) button
        print("==> Open SFTP panel (bottom bar)")
        status_y = cb - int(18 * scale)
        click(cr - int(126 * scale), status_y)
        time.sleep(1.5)

        print("==> Toggle monitor panel")
        click(cr - int(58 * scale), status_y)
        time.sleep(1.0)

        send_keys("{ESC}")
        print("OK: GUI connect + SFTP/monitor panel smoke passed")
        return 0
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
        print(f"FAIL: {e}", file=sys.stderr)
        sys.exit(1)
