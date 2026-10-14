#!/usr/bin/env python3
"""Capture SFTP panel screenshots after connecting Local Test SSH."""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import TOGGLE_SFTP
from gui_common import click, client_rect, launch_mist_gui, scale_for
from gui_screen import screenshot
from pywinauto import Application
from pywinauto.keyboard import send_keys


def main() -> int:
    exe = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/Mist.exe")
    out = Path(sys.argv[2] if len(sys.argv) > 2 else "target/sftp-ui-shots")
    out.mkdir(parents=True, exist_ok=True)

    proc, hwnd = launch_mist_gui(exe)
    try:
        p1 = screenshot(hwnd, "connected", shot_dir=out, stable_name="01-connected", maximize=True)
        print("saved", p1)

        Application(backend="uia").connect(handle=hwnd).window(handle=hwnd).set_focus()
        send_keys(TOGGLE_SFTP)
        time.sleep(1.8)
        p2 = screenshot(
            hwnd, "sftp-panel", shot_dir=out, stable_name="02-sftp-panel", maximize=False
        )
        print("saved", p2)

        cl, ct, cr, _cb = client_rect(hwnd)
        s = scale_for(cl, cr)
        fx = cr - int(200 * s)
        fy = ct + int(175 * s)
        click(fx, fy)
        time.sleep(0.2)
        send_keys("^a{BACKSPACE}gui", with_spaces=False)
        time.sleep(0.6)
        p3 = screenshot(
            hwnd, "sftp-filter", shot_dir=out, stable_name="03-sftp-filter", maximize=False
        )
        print("saved", p3)

        print("OK: SFTP UI captures done")
        return 0
    finally:
        try:
            proc.terminate()
            proc.wait(timeout=5)
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
