#!/usr/bin/env python3
"""Quick debug: connect existing session + SFTP GUI upload."""

import ctypes
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import paramiko
from pywinauto import Application, Desktop
from pywinauto.keyboard import send_keys

user32 = ctypes.windll.user32
SSH_USER, SSH_PASS, SSH_HOST = "mistterm_test", "mistterm123", "127.0.0.1"
REMOTE_FILE = "gui_e2e_upload.txt"
EXISTING = "Local Test SSH"


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class RECT(ctypes.Structure):
    _fields_ = [("l", ctypes.c_long), ("t", ctypes.c_long), ("r", ctypes.c_long), ("b", ctypes.c_long)]


def client_rect(hwnd):
    rect = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y, pt.x + rect.r, pt.y + rect.b


def click(x, y):
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(2, 0, 0, 0, 0)
    user32.mouse_event(4, 0, 0, 0, 0)
    time.sleep(0.15)


def right_click(x, y):
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(8, 0, 0, 0, 0)
    user32.mouse_event(16, 0, 0, 0, 0)
    time.sleep(0.2)


def main():
    exe = sys.argv[1]
    d = Path(tempfile.gettempdir()) / "mistterm_downloads"
    d.mkdir(parents=True, exist_ok=True)
    (d / REMOTE_FILE).write_text("payload\n", encoding="utf-8")

    proc = subprocess.Popen([exe])
    try:
        deadline = time.time() + 25
        hwnd = None
        while time.time() < deadline:
            for w in Desktop(backend="uia").windows():
                if "Mist" in w.window_text():
                    hwnd = int(w.handle)
                    break
            if hwnd:
                break
            time.sleep(0.2)
        app = Application(backend="uia").connect(process=proc.pid)
        app.window(handle=hwnd).set_focus()
        time.sleep(1)

        # 选中已有会话并连接
        send_keys("^j")
        time.sleep(0.4)
        send_keys(EXISTING.replace(" ", "{SPACE}"), with_spaces=True)
        time.sleep(0.6)
        cl, ct, cr, cb = client_rect(hwnd)
        s = (cr - cl) / 1200.0
        click(cl + int(110 * s), ct + int(155 * s))
        time.sleep(0.4)
        send_keys("+^t")
        time.sleep(10)

        click(cr - int(126 * s), cb - int(18 * s))
        time.sleep(3)

        lx = cr - int(240 * s)
        ly = ct + int(300 * s)
        click(lx, ly)
        right_click(lx, ly)
        click(lx + int(55 * s), ly + int(32 * s))
        time.sleep(6)

        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        c.connect(SSH_HOST, 22, SSH_USER, SSH_PASS, allow_agent=False, look_for_keys=False)
        sftp = c.open_sftp()
        for p in [REMOTE_FILE, f"C:/Users/{SSH_USER}/{REMOTE_FILE}"]:
            try:
                print("FOUND", p, sftp.stat(p).st_size)
                c.close()
                return 0
            except OSError:
                pass
        print("NOT FOUND")
        return 1
    finally:
        proc.terminate()


if __name__ == "__main__":
    sys.exit(main())
