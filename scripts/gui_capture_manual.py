#!/usr/bin/env python3
"""为操作手册采集 MistTerm 界面截图（覆盖已验证 GUI 功能）。"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import paramiko
from gui_automation_keys import (
    SFTP_UPLOAD,
    TOGGLE_SFTP,
    dismiss_new_session_dialog,
)
from gui_e2e_local_ssh import modal_layout, paste_field
from gui_full_workflow import (
    ITEM_H,
    MENU_X,
    SEP_H,
    SSH_HOST,
    SSH_PASS,
    SSH_USER,
    click,
    dismiss_esc,
    focus_sftp_panel,
    scale_for,
    set_local_sftp_path,
    ssh_preflight,
)
from gui_screen import MANUAL_SHOT_DIR, client_rect, find_mist_window, maximize_window, screenshot
from pywinauto import Application
from pywinauto.keyboard import send_keys

# 手册演示用文件名（与 E2E 的 gui_e2e_upload.txt 区分，截图更接近真实场景）
MANUAL_DEMO_FILE = "deploy-notes.txt"


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


@dataclass
class CaptureItem:
    id: str
    title: str
    section: str
    file: str


def prepare_manual_demo_file() -> tuple[Path, str]:
    d = Path(tempfile.gettempdir()) / "mistterm_downloads"
    d.mkdir(parents=True, exist_ok=True)
    for old in d.iterdir():
        if old.is_file():
            old.unlink()
    batch = time.strftime("%Y%m%d")
    marker = f"deploy-{batch}"
    content = (
        "# deploy-notes\n"
        "host: prod-web-01\n"
        f"updated: {time.strftime('%Y-%m-%d')}\n"
        f"batch: {marker}\n"
    )
    p = d / MANUAL_DEMO_FILE
    p.write_text(content, encoding="utf-8")
    return p, marker


def manual_remote_paths() -> list[str]:
    home = f"C:/Users/{SSH_USER}"
    return [
        MANUAL_DEMO_FILE,
        f"{home}/{MANUAL_DEMO_FILE}",
        f"{home}/mistterm_sftp/{MANUAL_DEMO_FILE}",
        f"mistterm_sftp/{MANUAL_DEMO_FILE}",
    ]


def wait_for_manual_upload(marker: str, timeout: float = 60.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        c = paramiko.SSHClient()
        c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        try:
            c.connect(
                SSH_HOST,
                22,
                SSH_USER,
                SSH_PASS,
                timeout=10,
                allow_agent=False,
                look_for_keys=False,
            )
            sftp = c.open_sftp()
            for rp in manual_remote_paths():
                try:
                    with sftp.open(rp, "r") as f:
                        body = f.read().decode("utf-8", errors="replace")
                    if marker in body:
                        return
                except OSError:
                    pass
        except Exception:
            pass
        finally:
            try:
                c.close()
            except Exception:
                pass
        time.sleep(2.0)
    raise RuntimeError(f"上传后服务器未找到 {MANUAL_DEMO_FILE}")


@dataclass
class ManualCapture:
    hwnd: int
    proc: subprocess.Popen[bytes]
    app: Application
    items: list[CaptureItem] = field(default_factory=list)
    cl: int = 0
    ct: int = 0
    cr: int = 0
    cb: int = 0
    s: float = 1.0
    sftp_open: bool = False
    ai_open: bool = False
    bottom_open: set[int] = field(default_factory=set)

    def refresh(self) -> None:
        self.cl, self.ct, self.cr, self.cb = client_rect(self.hwnd)
        self.s = scale_for(self.cl, self.cr)

    def focus(self) -> None:
        try:
            self.app.window(handle=self.hwnd).set_focus()
        except Exception:
            click(self.cl + int(80 * self.s), self.ct + int(16 * self.s))
        time.sleep(0.25)

    def clear_modals(self) -> None:
        self.focus()
        dismiss_new_session_dialog(repeats=1)
        dismiss_esc(3)

    def clear_terminal(self) -> None:
        self.focus_terminal()
        send_terminal_line("cls")
        time.sleep(0.6)

    def close_sftp(self) -> None:
        if self.sftp_open:
            send_keys(TOGGLE_SFTP)
            time.sleep(0.8)
            self.sftp_open = False

    def close_ai(self) -> None:
        if self.ai_open:
            send_keys("+^a")
            time.sleep(0.8)
            self.ai_open = False

    def close_bottom_panels(self) -> None:
        for off in list(self.bottom_open):
            self.bottom_btn(off)
            time.sleep(0.45)
        self.bottom_open.clear()

    def close_all_panels(self) -> None:
        self.clear_modals()
        self.close_sftp()
        self.close_ai()
        self.close_bottom_panels()

    def open_sftp(self) -> None:
        self.close_all_panels()
        send_keys(TOGGLE_SFTP)
        time.sleep(2.5)
        self.sftp_open = True

    def open_ai(self) -> None:
        self.close_all_panels()
        send_keys("+^a")
        time.sleep(1.0)
        self.ai_open = True

    def open_bottom_panel(self, offset: int) -> None:
        self.close_all_panels()
        self.bottom_btn(offset)
        time.sleep(0.8)
        self.bottom_open.add(offset)

    def snap(self, stable_id: str, title: str, section: str) -> Path:
        self.refresh()
        path = screenshot(
            self.hwnd,
            stable_id,
            MANUAL_SHOT_DIR,
            stable_name=stable_id,
        )
        rel = f"docs/manual/screenshots/{stable_id}.png"
        self.items.append(CaptureItem(stable_id, title, section, rel))
        print(f"  [截图] {title} -> {path.name}", flush=True)
        return path

    def reconnect(self, name: str = "Local Test SSH") -> None:
        self.close_all_panels()
        self.focus()
        send_keys("^j")
        time.sleep(0.6)
        send_keys("^a")
        send_keys(name.replace(" ", "{SPACE}"), with_spaces=True)
        time.sleep(0.8)
        click(self.cl + int(110 * self.s), self.ct + int(165 * self.s))
        time.sleep(0.5)
        send_keys("+^t")
        time.sleep(16.0)

    def focus_terminal(self) -> None:
        self.focus()
        w, h = self.cr - self.cl, self.cb - self.ct
        click(self.cl + int(w * 0.42), self.ct + int(h * 0.55))
        time.sleep(0.35)

    def run_terminal_demo(self, *, full: bool = True) -> None:
        """在 Windows SSH 上执行常见管理命令，输出贴近真实使用场景。"""
        self.focus_terminal()
        send_terminal_line("whoami")
        time.sleep(0.8)
        send_terminal_line("hostname")
        time.sleep(0.8)
        if full:
            send_terminal_line(f"cd /d C:\\Users\\{SSH_USER}")
            time.sleep(0.8)
            send_terminal_line("dir")
            time.sleep(2.2)

    def fill_new_session_demo(self) -> None:
        self.close_all_panels()
        maximize_window(self.hwnd)
        time.sleep(0.4)
        self.refresh()
        self.focus()
        send_keys("^n")
        time.sleep(2.0)
        m = modal_layout(self.hwnd)
        for key, text in [
            ("name", "Production Web-01"),
            ("host", "192.168.1.10"),
            ("port", "22"),
            ("user", "deploy"),
        ]:
            click(*m[key])
            time.sleep(0.2)
            paste_field(text)
            time.sleep(0.25)

    def open_menu(self, idx: int) -> int:
        self.close_all_panels()
        self.focus()
        x = self.cl + int(MENU_X[idx] * self.s)
        click(x, self.ct + int(16 * self.s))
        time.sleep(0.35)
        return x

    def pick_menu(self, menu_x: int, row: int, extra: int = 0) -> None:
        y = self.ct + int(16 * self.s) + int(((row + 1) * ITEM_H + extra) * self.s)
        click(menu_x, y, 0.35)

    def bottom_btn(self, offset: int) -> None:
        click(self.cr - int(offset * self.s), self.cb - int(18 * self.s), 0.35)


def cleanup_remote_test_artifacts() -> None:
    """清理 E2E 遗留文件，避免 SFTP/终端截图出现测试文件名。"""
    c = paramiko.SSHClient()
    c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    c.connect(SSH_HOST, 22, SSH_USER, SSH_PASS, timeout=10, allow_agent=False, look_for_keys=False)
    sftp = c.open_sftp()
    home = f"C:/Users/{SSH_USER}"
    for rp in [
        "gui_e2e_upload.txt",
        f"{home}/gui_e2e_upload.txt",
        f"{home}/mistterm_sftp/gui_e2e_upload.txt",
        "mistterm_sftp/gui_e2e_upload.txt",
        MANUAL_DEMO_FILE,
        f"{home}/{MANUAL_DEMO_FILE}",
        f"{home}/mistterm_sftp/{MANUAL_DEMO_FILE}",
        "mistterm_sftp/deploy-notes.txt",
    ]:
        try:
            sftp.remove(rp)
        except OSError:
            pass
    c.close()


def run_capture(exe: Path, timeout: float) -> list[CaptureItem]:
    ssh_preflight()
    cleanup_remote_test_artifacts()
    local_file, marker = prepare_manual_demo_file()
    print(f"==> 演示文件: {local_file}", flush=True)

    env = os.environ.copy()
    env["MISTTERM_GUI_AUTOMATION"] = "1"
    env["MISTTERM_E2E_FILE"] = MANUAL_DEMO_FILE

    proc = subprocess.Popen([str(exe)], env=env)
    hwnd = find_mist_window(proc, timeout=timeout)
    app = Application(backend="uia").connect(process=proc.pid)
    cap = ManualCapture(hwnd=hwnd, proc=proc, app=app)
    cap.refresh()
    time.sleep(1.0)

    print("==> 连接本地测试会话", flush=True)
    cap.reconnect()
    cap.close_all_panels()
    cap.run_terminal_demo(full=False)
    cap.snap("01-main-connected", "主界面（已连接 SSH）", "快速开始")

    print("==> 新建会话对话框", flush=True)
    cap.fill_new_session_demo()
    cap.snap("02-new-session-dialog", "新建会话对话框", "SSH 连接管理")
    cap.close_all_panels()

    print("==> 终端命令", flush=True)
    cap.close_all_panels()
    cap.clear_terminal()
    cap.run_terminal_demo(full=True)
    cap.snap("03-terminal-session", "终端会话与命令输出", "终端操作")

    print("==> SFTP 面板与上传", flush=True)
    cap.open_sftp()
    set_local_sftp_path(cap)
    cap.snap("04-sftp-panel", "SFTP 双栏文件浏览器", "文件传输")
    focus_sftp_panel(cap)
    send_keys(SFTP_UPLOAD)
    wait_for_manual_upload(marker, timeout=60.0)
    time.sleep(1.0)
    cap.snap("05-sftp-upload-done", "SFTP 上传完成", "文件传输")
    cap.close_sftp()

    print("==> AI 助手", flush=True)
    cap.open_ai()
    cap.snap("06-ai-panel", "AI 助手面板", "AI 助手")
    cap.close_ai()

    print("==> Tools > AI 设置", flush=True)
    x = cap.open_menu(3)
    cap.snap("07-menu-tools", "工具菜单", "界面导航")
    cap.pick_menu(x, 0)
    time.sleep(0.9)
    cap.snap("08-ai-settings-dialog", "AI 设置对话框", "AI 助手")
    cap.close_all_panels()

    print("==> 底栏面板", flush=True)
    for stable_id, title, section, off in [
        ("09-panel-monitor", "主机监控面板", "主机监控", 58),
        ("10-panel-port-forward", "端口转发面板", "端口转发", 92),
        ("11-panel-snippets", "命令片段面板", "命令片段", 160),
    ]:
        cap.open_bottom_panel(off)
        cap.snap(stable_id, title, section)
        cap.close_bottom_panels()

    print("==> View 菜单", flush=True)
    cap.open_menu(2)
    cap.snap("12-menu-view", "视图菜单（面板切换）", "界面导航")
    cap.close_all_panels()

    print("==> 偏好与关于", flush=True)
    cap.close_all_panels()
    send_keys("^,")
    time.sleep(0.9)
    cap.snap("13-preferences", "偏好设置", "主题与外观")
    cap.close_all_panels()
    send_keys("^h")
    time.sleep(0.9)
    cap.snap("14-about", "关于 Mist", "界面导航")
    cap.close_all_panels()

    print("==> 终端查找与其他工具", flush=True)
    cap.close_all_panels()
    cap.clear_terminal()
    cap.run_terminal_demo(full=True)
    send_keys("{F3}")
    time.sleep(0.7)
    send_keys("Desktop{ENTER}")
    time.sleep(0.5)
    cap.snap("15-terminal-find", "终端内查找 (F3)", "终端操作")
    cap.close_all_panels()

    x = cap.open_menu(3)
    cap.pick_menu(x, 1)
    time.sleep(0.9)
    cap.snap("16-fragment-library", "命令片段库", "命令片段")
    cap.close_all_panels()

    x = cap.open_menu(3)
    cap.pick_menu(x, 3, SEP_H)
    time.sleep(0.9)
    cap.snap("17-command-history", "命令历史", "终端操作")
    cap.close_all_panels()

    x = cap.open_menu(0)
    cap.pick_menu(x, 2)
    time.sleep(0.9)
    cap.snap("18-import-ssh-config", "导入 SSH Config", "SSH 连接管理")
    cap.close_all_panels()

    x = cap.open_menu(4)
    cap.pick_menu(x, 0)
    time.sleep(0.9)
    cap.snap("19-help-quick-start", "快速入门帮助", "界面导航")
    cap.close_all_panels()

    cap.reconnect()
    cap.close_all_panels()
    cap.snap("20-sidebar-sessions", "侧栏会话列表", "SSH 连接管理")

    manifest = MANUAL_SHOT_DIR / "manifest.json"
    manifest.write_text(
        json.dumps([item.__dict__ for item in cap.items], ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(f"\n==> 共 {len(cap.items)} 张截图 -> {MANUAL_SHOT_DIR}", flush=True)

    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()

    return cap.items


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe", nargs="?", default="target/debug/Mist.exe")
    parser.add_argument("--timeout", type=float, default=120.0)
    args = parser.parse_args()
    exe = Path(args.exe)
    if not exe.is_file():
        print(f"找不到 {exe}", file=sys.stderr)
        return 2
    MANUAL_SHOT_DIR.mkdir(parents=True, exist_ok=True)
    try:
        run_capture(exe, args.timeout)
        return 0
    except Exception as e:
        print(f"FATAL: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
