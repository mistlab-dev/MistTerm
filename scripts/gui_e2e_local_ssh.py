#!/usr/bin/env python3
"""MistTerm 纯 GUI E2E：新建连接 → 终端 → SFTP 上传/下载（无 cargo test）。"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import (
    SFTP_DOWNLOAD,
    SFTP_UPLOAD,
    TOGGLE_SFTP,
    dismiss_new_session_dialog,
)
from gui_common import (
    LOCAL_TEST_SESSION,
    REMOTE_FILE,
    SSH_HOST,
    SSH_PASS,
    SSH_USER,
    automation_env,
    capture_failure,
    click,
    client_rect,
    paste_field,
    remote_has_marker,
    scale_for,
    send_terminal_line,
    ssh_preflight,
)
from gui_coverage import CoverageTracker
from gui_screen import find_mist_window
from pywinauto import Application
from pywinauto.keyboard import send_keys

SESSION_NAME = "GUI E2E Local"


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


def prepare_local_file() -> tuple[Path, str]:
    d = Path(tempfile.gettempdir()) / "mistterm_downloads"
    d.mkdir(parents=True, exist_ok=True)
    marker = f"gui-e2e-{int(time.time())}"
    p = d / REMOTE_FILE
    p.write_text(f"MistTerm GUI E2E upload {marker}\n", encoding="utf-8")
    return p, marker


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


def focus_mist(hwnd: int, pid: int) -> None:
    app = Application(backend="uia").connect(process=pid)
    try:
        app.window(handle=hwnd).set_focus()
    except Exception:
        pass


def gui_connect_existing(hwnd: int, pid: int, name: str) -> None:
    print(f"==> [GUI] 连接已有会话：{name}")
    focus_mist(hwnd, pid)
    send_keys("^j")
    time.sleep(0.5)
    send_keys(name.replace(" ", "{SPACE}"), with_spaces=True)
    time.sleep(0.6)
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cl + int(110 * s), ct + int(160 * s))
    time.sleep(0.4)
    send_keys("+^t")
    time.sleep(12.0)


def gui_terminal_smoke(hwnd: int) -> None:
    print("==> [GUI] 终端：whoami / hostname / echo")
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    click(cl + int(450 * s), ct + int(400 * s))
    time.sleep(0.4)
    send_terminal_line("whoami")
    time.sleep(0.8)
    send_terminal_line("hostname")
    time.sleep(0.8)
    send_terminal_line("echo MISTTERM_GUI_OK")
    time.sleep(0.8)


def gui_open_sftp(hwnd: int, pid: int) -> None:
    print("==> [GUI] 打开 SFTP (Ctrl+Shift+S)")
    focus_mist(hwnd, pid)
    dismiss_new_session_dialog()
    send_keys(TOGGLE_SFTP)
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
    dismiss_new_session_dialog()
    click(cr - int(150 * s), ct + int(240 * s))
    time.sleep(0.3)
    send_keys(SFTP_UPLOAD)
    deadline = time.time() + 60.0
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
    deadline = time.time() + 90.0
    while time.time() < deadline:
        dismiss_new_session_dialog()
        send_keys(TOGGLE_SFTP)
        time.sleep(1.5)
        click(cr - int(150 * s), ct + int(240 * s))
        time.sleep(0.3)
        send_keys(SFTP_DOWNLOAD)
        time.sleep(6.0)
        if local_file.exists() and marker in local_file.read_text(encoding="utf-8"):
            print(f"  [OK] 已下载到 {local_file}")
            return
    raise RuntimeError("未确认下载文件")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--timeout", type=float, default=35.0)
    parser.add_argument("--keep-open", action="store_true")
    parser.add_argument("--skip-new-session", action="store_true")
    args = parser.parse_args()

    print("=== MistTerm GUI E2E（无单元测试）===\n")
    coverage = CoverageTracker("e2e")
    ssh_preflight()
    local_file, marker = prepare_local_file()

    proc: subprocess.Popen[bytes] | None = None
    hwnd: int | None = None
    try:
        proc = subprocess.Popen([args.exe], env=automation_env())
        hwnd = find_mist_window(proc, timeout=args.timeout)

        app = Application(backend="uia").connect(process=proc.pid)
        time.sleep(1.2)

        if not args.skip_new_session:
            gui_new_session(hwnd, app)
            coverage.mark("session.new_dialog")
        else:
            gui_connect_existing(hwnd, proc.pid, LOCAL_TEST_SESSION)
        coverage.mark("session.connect", "tab.new")

        if proc.poll() is not None:
            raise RuntimeError("Mist 进程已退出")

        gui_terminal_smoke(hwnd)
        coverage.mark("terminal.commands")
        gui_open_sftp(hwnd, proc.pid)
        coverage.mark("sftp.toggle")
        gui_set_local_path(hwnd)
        gui_sftp_upload(hwnd, marker)
        coverage.mark("sftp.upload")
        gui_sftp_download(hwnd, marker, local_file)
        coverage.mark("sftp.download")

        print("\n=== GUI E2E 通过 ===")
        code = coverage.report()
        if args.keep_open:
            proc.wait()
        return code
    except Exception as e:
        capture_failure(hwnd, "gui_e2e")
        print(f"\n=== GUI E2E 失败: {e} ===", file=sys.stderr)
        return 1
    finally:
        if proc is not None and proc.poll() is None and not args.keep_open:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
