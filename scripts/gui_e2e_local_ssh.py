#!/usr/bin/env python3
"""MistTerm 纯 GUI E2E：新建连接 → 终端 → SFTP 上传/下载（无 cargo test）。"""

from __future__ import annotations

import argparse
import ctypes
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import paramiko
from pywinauto import Application, Desktop
from pywinauto.keyboard import send_keys

user32 = ctypes.windll.user32
SESSION_NAME = "GUI E2E Local"
SSH_USER = "mistterm_test"
SSH_PASS = "mistterm123"
SSH_HOST = "127.0.0.1"
REMOTE_FILE = "gui_e2e_upload.txt"


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


class RECT(ctypes.Structure):
    _fields_ = [("l", ctypes.c_long), ("t", ctypes.c_long), ("r", ctypes.c_long), ("b", ctypes.c_long)]


def client_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y, pt.x + rect.r, pt.y + rect.b


def click(x: int, y: int) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(0.12)


def right_click(x: int, y: int) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0008, 0, 0, 0, 0)
    user32.mouse_event(0x0010, 0, 0, 0, 0)
    time.sleep(0.2)


def scale_for(cl: int, cr: int) -> float:
    return max(0.85, min(1.35, (cr - cl) / 1200.0))


def paste_field(text: str) -> None:
    send_keys("^a")
    time.sleep(0.05)
    send_keys(text, with_spaces=True, pause=0.02)


def modal_layout(hwnd: int) -> dict[str, tuple[int, int]]:
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    mx, my = (cl + cr) // 2, (ct + cb) // 2
    top = my - int(195 * s)
    return {
        "name": (mx - int(60 * s), top + int(72 * s)),
        "host": (mx - int(60 * s), top + int(118 * s)),
        "port": (mx + int(95 * s), top + int(118 * s)),
        "user": (mx - int(60 * s), top + int(164 * s)),
        "pass": (mx + int(50 * s), top + int(164 * s)),
        "agent_cb": (mx - int(120 * s), top + int(218 * s)),
        "save": (mx + int(40 * s), top + int(338 * s)),
    }


def ssh_preflight() -> None:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(SSH_HOST, 22, SSH_USER, SSH_PASS, timeout=10, allow_agent=False, look_for_keys=False)
    c.close()


def prepare_local_file() -> tuple[Path, str]:
    d = Path(tempfile.gettempdir()) / "mistterm_downloads"
    d.mkdir(parents=True, exist_ok=True)
    marker = f"gui-e2e-{int(time.time())}"
    p = d / REMOTE_FILE
    p.write_text(f"MistTerm GUI E2E upload {marker}\n", encoding="utf-8")
    return p, marker


def remote_has_marker(marker: str) -> bool:
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(SSH_HOST, 22, SSH_USER, SSH_PASS, timeout=10, allow_agent=False, look_for_keys=False)
    sftp = c.open_sftp()
    for rp in [REMOTE_FILE, f"C:/Users/{SSH_USER}/{REMOTE_FILE}"]:
        try:
            with sftp.open(rp, "r") as f:
                if marker in f.read().decode("utf-8", errors="replace"):
                    c.close()
                    return True
        except OSError:
            pass
    c.close()
    return False


def gui_new_session(hwnd: int, app: Application) -> None:
    print("==> [GUI] 新建会话：Ctrl+N → 填表 → 保存并连接")
    app.window(handle=hwnd).set_focus()
    send_keys("^n")
    time.sleep(1.0)
    m = modal_layout(hwnd)
    for key, text in [
        ("name", SESSION_NAME),
        ("host", SSH_HOST),
        ("port", "22"),
        ("user", SSH_USER),
        ("pass", SSH_PASS),
    ]:
        click(*m[key])
        paste_field(text)
        time.sleep(0.1)
    click(*m["agent_cb"])
    time.sleep(0.15)
    click(*m["save"])
    time.sleep(12.0)
    print("  [OK] 已提交新建连接")


def gui_connect_existing(hwnd: int, name: str) -> None:
    print(f"==> [GUI] 连接已有会话：{name}")
    app = Application(backend="uia").connect(process=int(hwnd))
    try:
        app.window(handle=hwnd).set_focus()
    except Exception:
        pass
    send_keys("^j")
    time.sleep(0.5)
    send_keys(name.replace(" ", "{SPACE}"), with_spaces=True)
    time.sleep(0.6)
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cl + int(110 * s), ct + int(160 * s))
    time.sleep(0.4)
    send_keys("^t")
    time.sleep(12.0)


def gui_terminal_smoke(hwnd: int) -> None:
    print("==> [GUI] 终端输入 whoami / echo")
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cl + int(450 * s), ct + int(400 * s))
    time.sleep(0.4)
    send_keys("whoami{ENTER}")
    time.sleep(1.0)
    send_keys("echo MISTTERM_GUI_OK{ENTER}")
    time.sleep(1.0)


def gui_open_sftp(hwnd: int) -> None:
    print("==> [GUI] 底栏打开 SFTP")
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cr - int(126 * s), cb - int(18 * s))
    time.sleep(3.0)


def gui_set_local_path(hwnd: int) -> None:
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    dock_left = cr - int(360 * s)
    click(dock_left + int(150 * s), ct + int(155 * s))
    path = Path(tempfile.gettempdir()) / "mistterm_downloads"
    send_keys("^a")
    send_keys(str(path), with_spaces=True, pause=0.02)
    send_keys("{ENTER}")
    time.sleep(1.5)


def gui_sftp_upload(hwnd: int, marker: str) -> None:
    print("==> [GUI] SFTP 上传 Ctrl+Shift+F9")
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cr - int(150 * s), ct + int(240 * s))
    time.sleep(0.3)
    send_keys("+^{F9}")
    deadline = time.time() + 45.0
    while time.time() < deadline:
        if remote_has_marker(marker):
            print("  [OK] 上传成功")
            return
        time.sleep(2.0)
    raise RuntimeError("未在服务器上找到上传文件")


def gui_sftp_download(hwnd: int, marker: str, local_file: Path) -> None:
    print("==> [GUI] SFTP 下载 Ctrl+Shift+F10")
    local_file.unlink(missing_ok=True)
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    time.sleep(2.0)
    click(cr - int(150 * s), ct + int(240 * s))
    time.sleep(0.3)
    send_keys("+^{F10}")
    deadline = time.time() + 45.0
    while time.time() < deadline:
        if local_file.exists() and marker in local_file.read_text(encoding="utf-8"):
            print(f"  [OK] 已下载到 {local_file}")
            return
        time.sleep(2.0)
    raise RuntimeError("未确认下载文件")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--timeout", type=float, default=35.0)
    parser.add_argument("--keep-open", action="store_true")
    parser.add_argument("--skip-new-session", action="store_true")
    args = parser.parse_args()

    print("=== MistTerm GUI E2E（无单元测试）===\n")
    ssh_preflight()
    local_file, marker = prepare_local_file()

    env = os.environ.copy()
    env["MISTTERM_GUI_AUTOMATION"] = "1"
    env["MISTTERM_E2E_FILE"] = REMOTE_FILE

    proc = subprocess.Popen([args.exe], env=env)
    try:
        hwnd = None
        deadline = time.time() + args.timeout
        while time.time() < deadline:
            for w in Desktop(backend="uia").windows():
                if "Mist" in w.window_text():
                    hwnd = int(w.handle)
                    break
            if hwnd:
                break
            time.sleep(0.25)
        if not hwnd:
            raise RuntimeError("未找到 Mist 窗口")

        app = Application(backend="uia").connect(process=proc.pid)
        time.sleep(1.2)

        if not args.skip_new_session:
            gui_new_session(hwnd, app)
        else:
            gui_connect_existing(hwnd, "Local Test SSH")

        if proc.poll() is not None:
            raise RuntimeError("Mist 进程已退出")

        gui_terminal_smoke(hwnd)
        gui_open_sftp(hwnd)
        gui_set_local_path(hwnd)
        gui_sftp_upload(hwnd, marker)
        gui_sftp_download(hwnd, marker, local_file)

        print("\n=== GUI E2E 通过 ===")
        if args.keep_open:
            proc.wait()
        return 0
    except Exception as e:
        print(f"\n=== GUI E2E 失败: {e} ===", file=sys.stderr)
        return 1
    finally:
        if proc.poll() is None and not args.keep_open:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
