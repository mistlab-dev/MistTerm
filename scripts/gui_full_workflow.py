#!/usr/bin/env python3
"""MistTerm 全套 GUI 流程：新建连接 → 终端 → SFTP → AI 与其它面板（无 cargo test）。"""

from __future__ import annotations

import argparse
import ctypes
import os
import subprocess
import sys
import tempfile
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pywinauto import Application
from gui_automation_keys import (
    SFTP_DOWNLOAD,
    SFTP_UPLOAD,
    TOGGLE_SFTP,
    dismiss_new_session_dialog,
)
from pywinauto.keyboard import send_keys

from gui_common import (
    automation_env,
    capture_failure,
    remote_has_marker,
    send_terminal_line,
    ssh_preflight,
)
from gui_coverage import CoverageTracker
from gui_screen import (
    SHOT_DIR,
    client_rect,
    find_mist_window,
    modal_sample_pixels,
    new_session_modal_seems_open,
    screenshot,
    sftp_dock_pixel,
    sftp_dock_seems_open,
)
SSH_USER = "mistterm_test"
SSH_PASS = "mistterm123"
SSH_HOST = "127.0.0.1"
REMOTE_FILE = "gui_e2e_upload.txt"
MENU_X = [16, 82, 132, 178, 228]
ITEM_H = 28
SEP_H = 8

user32 = ctypes.windll.user32


@dataclass
class Report:
    passed: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  [OK] {name}", flush=True)

    def fail(self, name: str, err: str) -> None:
        self.failed.append((name, err))
        print(f"  [FAIL] {name} — {err}", flush=True)

    def summary(self) -> int:
        print("\n=== 全套 GUI 流程汇总 ===", flush=True)
        print(f"  通过: {len(self.passed)}", flush=True)
        print(f"  失败: {len(self.failed)}", flush=True)
        if self.failed:
            print("\n失败项:", flush=True)
            for name, err in self.failed:
                print(f"  - {name}: {err}", flush=True)
        return 1 if self.failed else 0


def scale_for(cl: int, cr: int) -> float:
    return max(0.85, min(1.35, (cr - cl) / 1200.0))


def click(x: int, y: int, delay: float = 0.12) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)
    time.sleep(delay)


def right_click(x: int, y: int) -> None:
    user32.SetCursorPos(int(x), int(y))
    user32.mouse_event(0x0008, 0, 0, 0, 0)
    user32.mouse_event(0x0010, 0, 0, 0, 0)
    time.sleep(0.25)


def dismiss_esc(times: int = 2) -> None:
    for _ in range(times):
        send_keys("{ESC}")
        time.sleep(0.15)


class MistGui:
    def __init__(self, proc: subprocess.Popen[bytes], hwnd: int, report: Report):
        self.proc = proc
        self.hwnd = hwnd
        self.report = report
        self.shot_dir = SHOT_DIR
        self.app = Application(backend="uia").connect(process=proc.pid)
        cl, ct, cr, cb = client_rect(hwnd)
        self.cl, self.ct, self.cr, self.cb = cl, ct, cr, cb
        self.s = scale_for(cl, cr)
        self.menu_y = ct + int(16 * self.s)
        self.status_y = cb - int(18 * self.s)
        self.session_name = f"GUI E2E {uuid.uuid4().hex[:6]}"
        self._modal_baseline: list[tuple[int, int, int]] | None = None
        self._sftp_baseline: tuple[int, int, int] | None = None

    def modal_open(self) -> bool:
        return new_session_modal_seems_open(
            self.cl, self.ct, self.cr, self.cb, self.s, self._modal_baseline
        )

    def capture_modal_baseline(self) -> None:
        self.refresh_rect()
        self._modal_baseline = modal_sample_pixels(
            self.cl, self.ct, self.cr, self.cb, self.s
        )

    def snap(self, label: str) -> Path:
        return screenshot(self.hwnd, label, self.shot_dir)

    def refresh_rect(self) -> None:
        cl, ct, cr, cb = client_rect(self.hwnd)
        self.cl, self.ct, self.cr, self.cb = cl, ct, cr, cb
        self.s = scale_for(cl, cr)
        self.menu_y = ct + int(16 * self.s)
        self.status_y = cb - int(18 * self.s)

    def alive(self) -> bool:
        return self.proc.poll() is None

    def focus(self) -> None:
        if not self.alive():
            raise RuntimeError("Mist 进程已退出")
        try:
            self.app.window(handle=self.hwnd).set_focus()
        except Exception:
            click(self.cl + int(80 * self.s), self.menu_y)
        time.sleep(0.2)

    def step(self, name: str, fn, *, stop: bool = False) -> bool:
        try:
            fn()
            if not self.alive():
                raise RuntimeError("进程已退出")
            self.report.ok(name)
            return True
        except Exception as e:
            self.snap(f"FAIL-{name}")
            dismiss_esc(2)
            self.report.fail(name, str(e))
            if stop:
                raise RuntimeError(f"关键步骤失败: {name}: {e}") from e
            return False

    def shortcut(self, keys: str, wait: float = 0.4) -> None:
        self.focus()
        send_keys(keys)
        time.sleep(wait)

    def bottom_btn(self, offset: int) -> None:
        click(self.cr - int(offset * self.s), self.status_y, 0.35)

    def open_menu(self, idx: int) -> int:
        x = self.cl + MENU_X[idx]
        click(x, self.menu_y, 0.3)
        return x

    def pick_menu(self, menu_x: int, row: int, extra: int = 0) -> None:
        y = self.menu_y + int(((row + 1) * ITEM_H + extra) * self.s)
        click(menu_x, y, 0.35)

    def modal_layout(self) -> dict[str, tuple[int, int]]:
        """相对客户区比例（按实测截图校准）。"""
        w, h = self.cr - self.cl, self.cb - self.ct

        def rel(rx: float, ry: float) -> tuple[int, int]:
            return self.cl + int(w * rx), self.ct + int(h * ry)

        return {
            "name": rel(0.46, 0.375),
            "host": rel(0.46, 0.44),
            "port": rel(0.625, 0.44),
            "user": rel(0.46, 0.50),
            "pass": rel(0.57, 0.50),
            "agent_cb": rel(0.42, 0.57),
            "save": rel(0.60, 0.775),
            "cancel": rel(0.52, 0.775),
        }

    def force_clear_modals(self) -> None:
        """关闭新建会话等阻塞弹窗。"""
        self.focus()
        dismiss_new_session_dialog(repeats=2)
        dismiss_esc(1)

    def dismiss_blocking_modals(self) -> None:
        self.force_clear_modals()

    def capture_sftp_baseline(self) -> None:
        self.refresh_rect()
        self._sftp_baseline = sftp_dock_pixel(self.cl, self.ct, self.cr, self.cb, self.s)

    def sftp_open(self) -> bool:
        return sftp_dock_seems_open(
            self.cl, self.ct, self.cr, self.cb, self.s, self._sftp_baseline
        )

    def paste_field(self, text: str) -> None:
        send_keys(text, with_spaces=True, pause=0.03)

    def save_button_enabled(self) -> bool:
        from gui_screen import accent_color_detected, save_button_pixel

        r, g, b = save_button_pixel(self.cl, self.ct, self.cr, self.cb, self.s)
        return accent_color_detected(r, g, b)

    def fill_new_session(self) -> None:
        """打开新建会话对话框并截图，随后关闭以免遮挡后续步骤。"""
        self.focus()
        time.sleep(0.5)
        send_keys("^n")
        time.sleep(1.5)
        self.snap("new-session-opened")
        self.force_clear_modals()

    def require_modal_closed(self, ctx: str) -> None:
        self.force_clear_modals()

    def reconnect_session(self, name: str) -> None:
        self.force_clear_modals()
        self.focus()
        send_keys("^j")
        time.sleep(0.6)
        send_keys("^a")
        send_keys(name.replace(" ", "{SPACE}"), with_spaces=True)
        time.sleep(0.8)
        click(self.cl + int(110 * self.s), self.ct + int(165 * self.s))
        time.sleep(0.5)
        send_keys("+^t")
        time.sleep(18.0)


def ssh_established_count() -> int:
    out = subprocess.check_output(["netstat", "-an"], text=True, errors="replace")
    return sum(
        1
        for line in out.splitlines()
        if ":22" in line and "ESTABLISHED" in line and "127.0.0.1" in line
    )


def prepare_local_file() -> tuple[Path, str]:
    d = Path(tempfile.gettempdir()) / "mistterm_downloads"
    d.mkdir(parents=True, exist_ok=True)
    marker = f"gui-full-{int(time.time())}"
    p = d / REMOTE_FILE
    p.write_text(f"MistTerm GUI upload {marker}\n", encoding="utf-8")
    return p, marker


def set_local_sftp_path(gui: MistGui) -> None:
    path = Path(tempfile.gettempdir()) / "mistterm_downloads"
    dock_left = gui.cr - int(360 * gui.s)
    click(dock_left + int(150 * gui.s), gui.ct + int(155 * gui.s), 0.2)
    send_keys("^a")
    send_keys(str(path), with_spaces=True, pause=0.02)
    send_keys("{ENTER}")
    time.sleep(1.2)


def focus_sftp_panel(gui: MistGui) -> None:
    gui.focus()
    click(gui.cr - int(150 * gui.s), gui.ct + int(240 * gui.s))
    time.sleep(0.35)


def wait_for_remote_marker(marker: str, timeout: float = 45.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if remote_has_marker(marker):
            return
        time.sleep(2.0)
    raise RuntimeError("上传后服务器未找到带标记的文件")


def run_workflow(gui: MistGui, marker: str, local_file: Path, cov: CoverageTracker) -> None:
    if not gui.step("1. 新建会话对话框 (Ctrl+N) 打开并关闭", gui.fill_new_session, stop=True):
        return
    cov.mark("session.new_dialog")

    def ensure_ssh() -> None:
        gui.require_modal_closed("连接前")
        gui.reconnect_session("Local Test SSH")

    if not gui.step("1b. 连接本地测试会话 (Ctrl+J / Ctrl+T)", ensure_ssh, stop=True):
        return
    cov.mark("session.connect", "tab.new")

    def terminal_cmds() -> None:
        gui.require_modal_closed("终端输入前")
        gui.focus()
        click(gui.cl + int(450 * gui.s), gui.ct + int(380 * gui.s))
        time.sleep(0.3)
        send_terminal_line("whoami")
        time.sleep(0.8)
        send_terminal_line("echo MISTTERM_GUI_FULL_OK")
        time.sleep(0.8)

    if gui.step("2. 终端命令 whoami / echo", terminal_cmds):
        cov.mark("terminal.commands")

    def open_sftp() -> None:
        gui.force_clear_modals()
        dismiss_esc(2)
        gui.focus()
        send_keys(TOGGLE_SFTP)
        time.sleep(3.5)
        gui.snap("sftp-panel-opened")

    if not gui.step("3. 打开 SFTP (Ctrl+Shift+S)", open_sftp, stop=True):
        return
    cov.mark("sftp.toggle")

    def refresh_local() -> None:
        set_local_sftp_path(gui)
        dock_left = gui.cr - int(360 * gui.s)
        click(dock_left + int(200 * gui.s), gui.ct + int(175 * gui.s))
        time.sleep(1.5)

    gui.step("4. SFTP 本机路径与刷新", refresh_local)

    def upload_file() -> None:
        gui.force_clear_modals()
        dismiss_esc(2)
        focus_sftp_panel(gui)
        gui.focus()
        send_keys(SFTP_UPLOAD)
        wait_for_remote_marker(marker)

    if not gui.step("5. SFTP 上传 (Ctrl+Shift+F9)", upload_file, stop=True):
        return
    cov.mark("sftp.upload")

    def download_file() -> None:
        wait_for_remote_marker(marker)
        time.sleep(4.0)
        local_file.unlink(missing_ok=True)
        deadline = time.time() + 90.0
        while time.time() < deadline:
            gui.force_clear_modals()
            gui.focus()
            send_keys(TOGGLE_SFTP)
            time.sleep(1.5)
            focus_sftp_panel(gui)
            send_keys(SFTP_DOWNLOAD)
            time.sleep(6.0)
            if local_file.exists() and marker in local_file.read_text(encoding="utf-8"):
                return
        gui.snap("download-failed")
        raise RuntimeError("下载后本地未找到带标记的文件")

    if not gui.step("6. SFTP 下载 (Ctrl+Shift+F10)", download_file, stop=True):
        return
    cov.mark("sftp.download")

    def ai_panel() -> None:
        dismiss_esc()
        gui.shortcut("+^a", 0.8)
        click(gui.cr - int(170 * gui.s), gui.cb - int(72 * gui.s))
        time.sleep(0.3)
        send_keys("Explain the ls command briefly{ENTER}")
        time.sleep(1.0)

    def ai_settings() -> None:
        dismiss_esc()
        x = gui.open_menu(3)
        gui.pick_menu(x, 0)
        time.sleep(0.8)
        dismiss_esc(2)

    def other_panels() -> None:
        dismiss_esc()
        for off, _ in [(58, "Monitor"), (92, "Port Forward"), (160, "Snippets")]:
            gui.bottom_btn(off)
            time.sleep(0.5)
        dismiss_esc()

    def view_panels() -> None:
        dismiss_esc()
        x = gui.open_menu(2)
        for row, extra in [(4, SEP_H), (5, SEP_H), (6, SEP_H)]:
            gui.pick_menu(x, row, extra)
            time.sleep(0.45)
            dismiss_esc()

    def prefs_about() -> None:
        gui.shortcut("^,", 0.7)
        dismiss_esc()
        gui.shortcut("^h", 0.7)
        dismiss_esc()

    if gui.step("7. AI 助手面板输入并发送", ai_panel):
        cov.mark("panel.ai")

    if gui.step("8. Tools > AI 设置对话框", ai_settings):
        cov.mark("panel.ai_settings")

    if gui.step("9. 其它底栏面板 (Monitor/转发/片段)", other_panels):
        cov.mark("panel.monitor", "panel.port_forward", "panel.snippets")

    if gui.step("10. View 菜单切换片段/监控/AI", view_panels):
        cov.mark("panel.snippets", "panel.monitor", "panel.ai")

    if gui.step("11. 偏好设置与关于", prefs_about):
        cov.mark("dialog.preferences", "dialog.about")

    def terminal_find() -> None:
        dismiss_esc()
        gui.shortcut("{F3}")
        send_keys("GUI{ENTER}")
        time.sleep(0.4)
        dismiss_esc(2)

    if gui.step("12. 终端查找 F3", terminal_find):
        cov.mark("terminal.find")

    def tools_extra() -> None:
        dismiss_esc()
        x = gui.open_menu(3)
        gui.pick_menu(x, 1)
        time.sleep(0.7)
        dismiss_esc(2)
        x = gui.open_menu(3)
        gui.pick_menu(x, 3, SEP_H)
        time.sleep(0.7)
        dismiss_esc(2)

    if gui.step("13. 片段库与命令历史", tools_extra):
        cov.mark("dialog.fragment_lib", "terminal.history")

    def import_ssh() -> None:
        dismiss_esc()
        x = gui.open_menu(0)
        gui.pick_menu(x, 2)
        time.sleep(0.7)
        dismiss_esc(2)

    if gui.step("14. 导入 SSH Config", import_ssh):
        cov.mark("session.import_ssh")

    def help_quick() -> None:
        dismiss_esc()
        x = gui.open_menu(4)
        gui.pick_menu(x, 0)
        time.sleep(0.7)
        dismiss_esc(2)

    if gui.step("15. 帮助快速入门", help_quick):
        cov.mark("dialog.help")

    def edit_session() -> None:
        gui.shortcut("^e")
        time.sleep(0.7)
        dismiss_esc(2)

    if gui.step("16. 编辑会话 Ctrl+E", edit_session):
        cov.mark("session.edit")

    def split_and_pane() -> None:
        dismiss_esc()
        gui.shortcut("+^d")
        time.sleep(0.5)
        gui.shortcut("+^{LEFT}")
        time.sleep(0.4)
        dismiss_esc()

    if gui.step("17. 分屏与窗格切换", split_and_pane):
        cov.mark("terminal.split_h", "terminal.pane_focus")

    def split_vertical() -> None:
        dismiss_esc()
        gui.shortcut("+^u")
        time.sleep(0.4)
        dismiss_esc()

    if gui.step("18. 上下分屏 Ctrl+Shift+U", split_vertical):
        cov.mark("terminal.split_v")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("exe")
    parser.add_argument("--timeout", type=float, default=180.0)
    parser.add_argument("--keep-open", action="store_true")
    args = parser.parse_args()

    print("=== MistTerm 全套 GUI 流程（无单元测试）===\n")
    report = Report()
    coverage = CoverageTracker("workflow")

    proc: subprocess.Popen[bytes] | None = None
    hwnd: int | None = None
    try:
        print("==> SSH 预检与准备本机文件")
        ssh_preflight()
        local_file, marker = prepare_local_file()
        print(f"    本机文件: {local_file} (marker={marker})")

        print(f"==> 启动 {args.exe} (MISTTERM_GUI_AUTOMATION=1)")
        proc = subprocess.Popen([args.exe], env=automation_env())
        deadline = time.time() + args.timeout
        hwnd = None
        while time.time() < deadline:
            try:
                hwnd = find_mist_window(proc, timeout=min(5.0, deadline - time.time()))
                break
            except RuntimeError as e:
                if proc.poll() is not None:
                    raise RuntimeError(f"Mist 进程已退出 (code={proc.returncode})") from e
                if time.time() >= deadline:
                    raise
                time.sleep(0.25)
        if not hwnd:
            raise RuntimeError("未找到 Mist 窗口")

        print(f"    hwnd={hwnd} pid={proc.pid}")
        print(f"    截图目录: {SHOT_DIR}")
        time.sleep(1.0)
        gui = MistGui(proc, hwnd, report)
        run_workflow(gui, marker, local_file, coverage)

        code = max(report.summary(), coverage.report())
        if code == 0:
            print("\n=== 全套 GUI 流程通过 ===")
        if args.keep_open and proc.poll() is None:
            proc.wait()
        return code
    except Exception as e:
        capture_failure(hwnd, "full_workflow")
        print(f"\nFATAL: {e}", file=sys.stderr)
        return 2
    finally:
        if proc is not None and proc.poll() is None and not args.keep_open:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
