#!/usr/bin/env python3
"""试用清单相关 UI 点测（1920 全屏校准版）。

用法:
  python scripts/gui_trial_ui_click.py target/debug/Mist.exe
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import dismiss_new_session_dialog
from gui_common import (
    click,
    client_rect,
    clipboard_set,
    focus_terminal_area,
    scale_for,
    send_terminal_line,
    stop_existing_mist_processes,
)
from gui_screen import find_mist_window, screenshot, window_rect
from pywinauto import Application
from pywinauto.keyboard import send_keys


@dataclass
class Report:
    passed: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)
    notes: list[str] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  [OK] {name}", flush=True)

    def fail(self, name: str, err: str) -> None:
        self.failed.append((name, err))
        print(f"  [FAIL] {name} — {err}", flush=True)

    def note(self, msg: str) -> None:
        self.notes.append(msg)
        print(f"  [NOTE] {msg}", flush=True)


def focus_win(app: Application, hwnd: int) -> None:
    try:
        app.window(handle=hwnd).set_focus()
    except Exception:
        pass
    time.sleep(0.15)


def shot(hwnd: int, out: Path, name: str) -> Path:
    path = screenshot(hwnd, name, shot_dir=out, stable_name=name, maximize=False)
    print(f"  [SHOT] {path}", flush=True)
    return path


def paste_text(text: str) -> None:
    clipboard_set(text)
    time.sleep(0.08)
    send_keys("^v")
    time.sleep(0.15)


def esc(times: int = 2) -> None:
    for _ in range(times):
        send_keys("{ESC}")
        time.sleep(0.2)


def rail_xy(hwnd: int, index: int) -> tuple[int, int]:
    """0连接 1片段 2文件 3转发 4监控 5助手；截图实测顶约 +52，步距约 52。"""
    left, top, _, _ = window_rect(hwnd)
    return left + 30, top + 52 + index * 52


def open_local_shell(hwnd: int) -> None:
    cl, ct, cr, cb = client_rect(hwnd)
    # 欢迎页中间「本机终端」按钮
    click((cl + cr) // 2, ct + int((cb - ct) * 0.52), pause=0.5)
    time.sleep(0.6)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("exe", nargs="?", default=str(Path("target/debug/Mist.exe")))
    ap.add_argument("--out", default=str(Path("target/gui_trial_ui")))
    ap.add_argument("--keep-open", action="store_true")
    ap.add_argument("--timeout", type=float, default=30.0)
    args = ap.parse_args()

    exe = Path(args.exe)
    if not exe.is_file():
        print(f"FATAL: missing {exe}", flush=True)
        return 2

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    report = Report()

    print("==> Launch", flush=True)
    stop_existing_mist_processes(title_sub="Mist")
    time.sleep(0.5)
    env = os.environ.copy()
    env["MISTTERM_GUI_AUTOMATION"] = "1"
    env.pop("MISTTERM_AUTO_CONNECT", None)
    proc = subprocess.Popen([str(exe)], env=env)
    try:
        hwnd = find_mist_window(proc, timeout=args.timeout, title_sub="Mist")
        app = Application(backend="uia").connect(process=proc.pid)
        focus_win(app, hwnd)
        dismiss_new_session_dialog(repeats=2)
        esc(1)
        time.sleep(0.4)
        shot(hwnd, out, "00_launch")
        report.ok("launch")

        cl, ct, cr, cb = client_rect(hwnd)
        s = scale_for(cl, cr)

        # 1) 连接行：点 Linux Test SSH 名称（非最左侧色条）
        print("==> Connection row name click", flush=True)
        try:
            click(*rail_xy(hwnd, 0), pause=0.4)  # 确保连接面板开
            time.sleep(0.35)
            # 侧栏名称约 x=170–220，Linux Test SSH 约 y=270（相对窗）
            left, top, _, _ = window_rect(hwnd)
            click(left + 190, top + 275, pause=0.7)
            shot(hwnd, out, "01_conn_row")
            report.ok("conn_row_click")
            report.note("01：名称区点击后是否选中/开始连接")
        except Exception as e:
            report.fail("conn_row", str(e))

        # 2) Ctrl+N → 名称框粘贴中文
        print("==> New session name field", flush=True)
        try:
            esc(2)
            focus_win(app, hwnd)
            send_keys("^n")
            time.sleep(0.7)
            shot(hwnd, out, "02_new_open")
            # 弹窗名称框：窗口中心偏上
            click((cl + cr) // 2, ct + int(145 * s), pause=0.35)
            send_keys("^a")
            time.sleep(0.05)
            paste_text("UI点测连接")
            time.sleep(0.25)
            shot(hwnd, out, "03_new_name")
            esc(2)
            report.ok("new_session_name")
            report.note("03：名称框应显示「UI点测连接」")
        except Exception as e:
            report.fail("new_session_name", str(e))
            esc(2)

        # 3) 本机终端 + Ctrl+F
        print("==> Local shell + Ctrl+F", flush=True)
        try:
            esc(2)
            open_local_shell(hwnd)
            time.sleep(0.5)
            focus_terminal_area(hwnd)
            send_terminal_line("echo TRIAL_FIND_MARK")
            time.sleep(0.35)
            send_keys("^f")
            time.sleep(0.45)
            shot(hwnd, out, "04_ctrl_f")
            esc(1)
            report.ok("ctrl_f")
            report.note("04：应有查找条；终端无 ^F")
        except Exception as e:
            report.fail("ctrl_f", str(e))
            esc(1)

        # 4) 主题：视图菜单 → 主题（更保守的坐标）
        print("==> Theme switch", flush=True)
        try:
            esc(2)
            focus_terminal_area(hwnd)
            send_terminal_line("echo THEME_中文测试#")
            time.sleep(0.4)
            # 顶栏「视图」约 client x=115–130
            click(cl + 122, ct + 16, pause=0.4)
            # 下拉里「主题」大约第 8–10 项；先点偏下
            click(cl + 160, ct + 250, pause=0.45)
            # 子菜单第一项旁的「晨曦」
            click(cl + 340, ct + 250, pause=0.7)
            shot(hwnd, out, "05_theme_a")
            # 再切回暗夜
            click(cl + 122, ct + 16, pause=0.4)
            click(cl + 160, ct + 250, pause=0.45)
            click(cl + 340, ct + 220, pause=0.7)
            shot(hwnd, out, "06_theme_b")
            report.ok("theme_switch")
            report.note("05/06：终端是否空白/残影；若误开其它弹窗则坐标仍偏")
        except Exception as e:
            report.fail("theme_switch", str(e))
            esc(3)

        # 5) 活动栏互斥：依次片段→文件→监控→助手
        print("==> Activity rail exclusive", flush=True)
        try:
            esc(3)
            for idx, tag in ((1, "snips"), (2, "files"), (4, "mon"), (5, "ai")):
                click(*rail_xy(hwnd, idx), pause=0.55)
                shot(hwnd, out, f"07_rail_{tag}")
            report.ok("rail_cycle")
            report.note("07_*：同一时刻只应一个主图标蓝高亮")
        except Exception as e:
            report.fail("rail_cycle", str(e))

        # 6) AI 发送反馈
        print("==> AI send feedback", flush=True)
        try:
            esc(2)
            click(*rail_xy(hwnd, 5), pause=0.55)
            time.sleep(0.4)
            # 右 dock 输入区
            click(cr - int(180 * s), cb - int(120 * s), pause=0.3)
            paste_text("查所有服务器磁盘")
            time.sleep(0.2)
            # 发送按钮
            click(cr - int(55 * s), cb - int(75 * s), pause=0.55)
            time.sleep(0.5)
            shot(hwnd, out, "08_ai_send")
            report.ok("ai_send")
            report.note("08：应有 status/Toast/计划卡，不能静默")
        except Exception as e:
            report.fail("ai_send", str(e))

    finally:
        print("\n=== Trial UI click summary ===", flush=True)
        print(f"  passed: {len(report.passed)}  failed: {len(report.failed)}", flush=True)
        for n, e in report.failed:
            print(f"  - FAIL {n}: {e}", flush=True)
        for n in report.notes:
            print(f"  - NOTE {n}", flush=True)
        print(f"  shots → {out}", flush=True)
        if not args.keep_open:
            try:
                proc.terminate()
            except Exception:
                pass
            time.sleep(0.25)
            try:
                proc.kill()
            except Exception:
                pass

    return 1 if report.failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
