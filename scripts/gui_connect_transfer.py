#!/usr/bin/env python3
"""Launch Mist, connect to Local Test SSH, open SFTP / monitor panels (smoke)."""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import TOGGLE_SFTP, dismiss_new_session_dialog
from gui_common import (
    LOCAL_TEST_SESSION,
    automation_env,
    capture_failure,
    click,
    client_rect,
    scale_for,
    ssh_preflight,
)
from gui_coverage import CoverageTracker
from gui_screen import find_mist_window
from pywinauto import Application
from pywinauto.keyboard import send_keys


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--session", default=LOCAL_TEST_SESSION)
    parser.add_argument("--timeout", type=float, default=30.0)
    args = parser.parse_args()

    print("==> SSH preflight")
    ssh_preflight()
    coverage = CoverageTracker("connect")

    print(f"==> Launch {args.exe}")
    proc: subprocess.Popen[bytes] | None = None
    hwnd: int | None = None
    try:
        proc = subprocess.Popen([args.exe], env=automation_env())
        hwnd = find_mist_window(proc, timeout=args.timeout)
        app = Application(backend="uia").connect(process=proc.pid)
        app.window(handle=hwnd).set_focus()
        time.sleep(1.0)

        cl, ct, cr, cb = client_rect(hwnd)
        s = scale_for(cl, cr)

        print(f"==> Connect session '{args.session}'")
        dismiss_new_session_dialog()
        send_keys("^j")
        time.sleep(0.4)
        send_keys(args.session.replace(" ", "{SPACE}"), with_spaces=True)
        time.sleep(0.5)
        click(cl + int(110 * s), ct + int(165 * s))
        time.sleep(0.4)
        send_keys("+^t")
        time.sleep(14.0)

        if proc.poll() is not None:
            raise RuntimeError("Mist exited during connect")

        print("==> Open SFTP panel (Ctrl+Shift+S)")
        dismiss_new_session_dialog()
        send_keys(TOGGLE_SFTP)
        time.sleep(2.5)

        print("==> Toggle monitor panel (bottom bar)")
        status_y = cb - int(18 * s)
        click(cr - int(58 * s), status_y)
        time.sleep(1.0)
        click(cr - int(58 * s), status_y)
        time.sleep(0.5)

        send_keys("{ESC}")
        coverage.mark("session.connect", "sftp.toggle", "panel.monitor", "bar.bottom")
        print("OK: GUI connect + SFTP/monitor panel smoke passed")
        code = coverage.report()
        return code
    except Exception as e:
        capture_failure(hwnd, "connect_transfer")
        print(f"FAIL: {e}", file=sys.stderr)
        return 1
    finally:
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
