#!/usr/bin/env python3
"""MistTerm Windows GUI 测试脚本的共享辅助。"""

from __future__ import annotations

import ctypes
import os
import time
from pathlib import Path

import paramiko
from pywinauto.keyboard import send_keys

from gui_screen import client_rect, screenshot

user32 = ctypes.windll.user32

SSH_HOST = "127.0.0.1"
SSH_USER = "mistterm_test"
SSH_PASS = "mistterm123"
SSH_PORT = 22
REMOTE_FILE = "gui_e2e_upload.txt"
LOCAL_TEST_SESSION = "Local Test SSH"


def scale_for(cl: int, cr: int) -> float:
    return max(0.85, min(1.35, (cr - cl) / 1200.0))


def click(x: int, y: int, pause: float = 0.12) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(pause)


def paste_field(text: str) -> None:
    send_keys("^a")
    time.sleep(0.05)
    send_keys(text, with_spaces=True, pause=0.02)


def send_terminal_line(text: str) -> None:
    """向终端发送一行命令（转义 pywinauto 特殊字符）。"""
    escaped = (
        text.replace("{", "{{")
        .replace("}", "}}")
        .replace("+", "{+}")
        .replace("^", "{^}")
        .replace("%", "{%}")
        .replace("~", "{~}")
    )
    send_keys(escaped + "{ENTER}", with_spaces=True, pause=0.02)


def ssh_preflight() -> None:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(
        SSH_HOST,
        SSH_PORT,
        SSH_USER,
        SSH_PASS,
        timeout=10,
        allow_agent=False,
        look_for_keys=False,
    )
    c.close()


def remote_paths(filename: str = REMOTE_FILE) -> list[str]:
    home = f"C:/Users/{SSH_USER}"
    return [
        filename,
        f"{home}/{filename}",
        f"{home}/mistterm_sftp/{filename}",
        f"mistterm_sftp/{filename}",
    ]


def remote_has_marker(marker: str, filename: str = REMOTE_FILE) -> bool:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        c.connect(
            SSH_HOST,
            SSH_PORT,
            SSH_USER,
            SSH_PASS,
            timeout=10,
            allow_agent=False,
            look_for_keys=False,
        )
        sftp = c.open_sftp()
        for rp in remote_paths(filename):
            try:
                with sftp.open(rp, "r") as f:
                    if marker in f.read().decode("utf-8", errors="replace"):
                        return True
            except OSError:
                pass
    finally:
        c.close()
    return False


def automation_env(*, e2e_file: str = REMOTE_FILE) -> dict[str, str]:
    env = os.environ.copy()
    env["MISTTERM_GUI_AUTOMATION"] = "1"
    env["MISTTERM_E2E_FILE"] = e2e_file
    return env


def capture_failure(hwnd: int | None, label: str) -> Path | None:
    if hwnd is None:
        return None
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in label)[:48]
    path = screenshot(hwnd, f"fail-{safe}", stable_name=f"fail_{safe}_{int(time.time() * 1000)}")
    print(f"    [失败截图] {path}", flush=True)
    return path


def connect_local_session(
    hwnd: int,
    pid: int,
    name: str = LOCAL_TEST_SESSION,
    *,
    wait: float = 14.0,
) -> None:
    """侧栏搜索并 Ctrl+T 连接本地测试会话。"""
    from gui_automation_keys import dismiss_new_session_dialog
    from pywinauto import Application

    try:
        Application(backend="uia").connect(process=pid).window(handle=hwnd).set_focus()
    except Exception:
        pass
    dismiss_new_session_dialog()
    send_keys("^j")
    time.sleep(0.5)
    send_keys("^a")
    send_keys(name.replace(" ", "{SPACE}"), with_spaces=True)
    time.sleep(0.6)
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cl + int(110 * s), ct + int(165 * s))
    time.sleep(0.4)
    send_keys("^t")
    time.sleep(wait)
