#!/usr/bin/env python3
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import dismiss_new_session_dialog
from gui_common import click, client_rect, stop_existing_mist_processes
from gui_screen import screenshot, window_rect
from pywinauto import Application, Desktop
from pywinauto.keyboard import send_keys


def main() -> int:
    import subprocess

    stop_existing_mist_processes(title_sub="Mist")
    time.sleep(0.5)
    exe = Path(__file__).resolve().parents[1] / "target" / "debug" / "Mist.exe"
    out = Path(__file__).resolve().parents[1] / "target" / "gui_trial_ui2"
    out.mkdir(parents=True, exist_ok=True)
    proc = subprocess.Popen([str(exe)])
    hwnd = None
    for _ in range(50):
        time.sleep(0.2)
        for w in Desktop(backend="uia").windows():
            try:
                if w.process_id() == proc.pid and "Mist" in (w.window_text() or ""):
                    hwnd = int(w.handle)
                    break
            except Exception:
                pass
        if hwnd:
            break
    print("hwnd", hwnd, flush=True)
    if not hwnd:
        return 1
    app = Application(backend="uia").connect(process=proc.pid)
    try:
        app.window(handle=hwnd).set_focus()
    except Exception:
        pass
    dismiss_new_session_dialog(repeats=2)
    send_keys("{ESC}")
    time.sleep(0.2)

    send_keys("^n")
    time.sleep(0.75)
    cl, ct, cr, cb = client_rect(hwnd)
    click((cl + cr) // 2, ct + 145, pause=0.4)
    send_keys("^a")
    time.sleep(0.05)
    send_keys("UI_CLICK_TEST", with_spaces=False, pause=0.02)
    time.sleep(0.35)
    screenshot(hwnd, "03b_name", shot_dir=out, stable_name="03b_name", maximize=False)
    send_keys("{ESC}")
    time.sleep(0.35)

    left, top, _, _ = window_rect(hwnd)
    click(left + 30, top + 52 + 5 * 52, pause=0.6)
    click(cr - 180, cb - 120, pause=0.35)
    send_keys("disk check", with_spaces=True, pause=0.02)
    time.sleep(0.2)
    click(cr - 55, cb - 75, pause=0.65)
    time.sleep(0.4)
    screenshot(hwnd, "08b_ai", shot_dir=out, stable_name="08b_ai", maximize=False)
    print("done", flush=True)
    try:
        proc.terminate()
    except Exception:
        pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
