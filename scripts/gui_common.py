#!/usr/bin/env python3
"""MistTerm Windows GUI 测试脚本的共享辅助。"""

from __future__ import annotations

import base64
import ctypes
import os
import subprocess
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


def clipboard_get() -> str:
    """读取系统剪贴板文本（PowerShell，避免与 GUI 进程争用 OpenClipboard）。"""
    proc = subprocess.run(
        ["powershell", "-NoProfile", "-Command", "Get-Clipboard -Raw"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=10,
    )
    if proc.returncode != 0:
        return ""
    return proc.stdout or ""


def clipboard_set(text: str) -> None:
    """写入系统剪贴板文本（PowerShell Base64，避免转义问题）。"""
    payload = base64.b64encode(text.encode("utf-16-le")).decode("ascii")
    ps = (
        f"$b=[Convert]::FromBase64String('{payload}'); "
        "$t=[System.Text.Encoding]::Unicode.GetString($b); "
        "Set-Clipboard -Value $t"
    )
    proc = subprocess.run(
        ["powershell", "-NoProfile", "-Command", ps],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=10,
    )
    if proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "").strip()
        raise RuntimeError(f"Set-Clipboard failed: {err}")


def drag_select(x1: int, y1: int, x2: int, y2: int, pause: float = 0.15) -> None:
    user32.SetCursorPos(int(x1), int(y1))
    time.sleep(0.05)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.SetCursorPos(int(x2), int(y2))
    time.sleep(pause)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.12)


def focus_terminal_area(hwnd: int, *, y_ratio: float = 0.58) -> None:
    """点击终端主体区域以获取键盘焦点。"""
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    x = cl + int((cr - cl) * 0.42)
    y = ct + int((cb - ct) * y_ratio)
    click(x, y, pause=0.28)


def remote_temp_path(name: str) -> str:
    """Windows 测试账号下的 Temp 路径（正斜杠，供 SFTP/脚本统一使用）。"""
    return f"C:/Users/{SSH_USER}/AppData/Local/Temp/{name}"


def remote_exec(command: str, *, timeout: float = 15.0) -> tuple[int, str, str]:
    """经独立 SSH 会话执行 cmd 命令，返回 (exit_code, stdout, stderr)。"""
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        c.connect(
            SSH_HOST,
            SSH_PORT,
            SSH_USER,
            SSH_PASS,
            timeout=timeout,
            allow_agent=False,
            look_for_keys=False,
        )
        _stdin, stdout, stderr = c.exec_command(command, timeout=timeout)
        out = stdout.read().decode("utf-8", errors="replace")
        err = stderr.read().decode("utf-8", errors="replace")
        code = stdout.channel.recv_exit_status()
        return code, out, err
    finally:
        c.close()


def remote_assert_file(path: str, marker: str, *, what: str) -> None:
    """断言远端文本文件包含 marker；失败时附带 type 输出便于排查。"""
    if remote_text_file_contains(path, marker):
        return
    win_path = path.replace("/", "\\")
    code, out, err = remote_exec(f'type "{win_path}" 2>&1')
    detail = (out or err or "(file missing)").strip()[:240]
    raise RuntimeError(f"{what}: expected {marker!r} in {path}, got: {detail!r} (type exit {code})")


def remote_text_file_contains(path: str, marker: str) -> bool:
    """经 SSH 读取远端文本文件是否包含 marker。"""
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
        try:
            with sftp.open(path.replace("\\", "/"), "r") as f:
                body = f.read().decode("utf-8", errors="replace")
                return marker in body
        except OSError:
            return False
    finally:
        c.close()


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
    """确认本地 sshd 可登录且 exec 输出正确。"""
    code, out, err = remote_exec("echo ok")
    got = out.strip()
    if code != 0 or got != "ok":
        msg = err.strip() or got or f"exit {code}"
        raise RuntimeError(
            f"SSH preflight failed for {SSH_USER}@{SSH_HOST}:{SSH_PORT}: {msg}. "
            f"Run: .\\scripts\\ensure-windows-test-sshd.ps1"
        )
    print(f"  [SSH] {SSH_USER}@{SSH_HOST} exec echo ok -> {got!r}", flush=True)


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
    """侧栏搜索并 Ctrl+Shift+T 连接本地测试会话。"""
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
    send_keys("+^t")
    time.sleep(wait)
