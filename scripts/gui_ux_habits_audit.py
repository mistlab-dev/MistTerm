#!/usr/bin/env python3
"""GUI UX 习惯走查：模拟真人鼠标键盘，截图并记录可疑交互。

不替代完整 e2e；侧重「像不像人用」：焦点、选区、AI 输入、面板切换、回车发送等。
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import dismiss_new_session_dialog
from gui_common import (
    LOCAL_TEST_SESSION,
    click,
    client_rect,
    clipboard_get,
    clipboard_set,
    connect_local_session,
    drag_select,
    focus_terminal_area,
    launch_mist_gui,
    reload_ssh_test_config,
    scale_for,
    send_terminal_line,
    ssh_preflight,
    use_local_openssh_test_config,
)
from gui_screen import screenshot
from pywinauto import Application
from pywinauto.keyboard import send_keys


@dataclass
class Finding:
    severity: str  # high / medium / low / note
    area: str
    detail: str


@dataclass
class Audit:
    findings: list[Finding] = field(default_factory=list)
    shots: list[str] = field(default_factory=list)
    ok_steps: list[str] = field(default_factory=list)
    fail_steps: list[tuple[str, str]] = field(default_factory=list)

    def note(self, area: str, detail: str, severity: str = "note") -> None:
        self.findings.append(Finding(severity, area, detail))
        print(f"  [{severity.upper()}] {area}: {detail}", flush=True)

    def step_ok(self, name: str) -> None:
        self.ok_steps.append(name)
        print(f"  [OK] {name}", flush=True)

    def step_fail(self, name: str, err: str) -> None:
        self.fail_steps.append((name, err))
        print(f"  [FAIL] {name} — {err}", flush=True)


def shot(hwnd: int, out_dir: Path, name: str, audit: Audit) -> Path:
    path = screenshot(
        hwnd,
        name,
        shot_dir=out_dir,
        stable_name=name,
        maximize=False,
    )
    audit.shots.append(str(path))
    print(f"  [SHOT] {path}", flush=True)
    return path


def focus_win(app: Application, hwnd: int) -> None:
    try:
        app.window(handle=hwnd).set_focus()
    except Exception:
        pass
    time.sleep(0.2)


def is_hung(hwnd: int) -> bool:
    """连续两次判定未响应才算挂起，避免 SendKeys 瞬间误报。"""
    import ctypes

    try:
        user32 = ctypes.windll.user32
        if not bool(user32.IsHungAppWindow(hwnd)):
            return False
        time.sleep(0.45)
        return bool(user32.IsHungAppWindow(hwnd))
    except Exception:
        return False


def open_ai_panel(hwnd: int, app: Application) -> None:
    """点活动栏 AI 图标（截图采样第 6 簇约 y=320；y=274 是系统监控）。"""
    from gui_screen import window_rect

    focus_win(app, hwnd)
    left, top, _, _ = window_rect(hwnd)
    click(left + 28, top + 320, pause=0.4)
    time.sleep(0.65)


def click_ai_draft(hwnd: int) -> None:
    cl, ct, cr, cb = client_rect(hwnd)
    s = scale_for(cl, cr)
    # 右 dock 底部输入区
    x = cr - int(160 * s)
    y = cb - int(110 * s)
    click(x, y, pause=0.25)


def paste_unicode(text: str) -> None:
    """尽量粘贴 Unicode；剪贴板争用或 SendKeys 卡死则跳过(不阻塞走查)。"""
    try:
        clipboard_set(text)
        time.sleep(0.1)
        # SendKeys 偶发卡死：限时跑
        import concurrent.futures

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            fut = pool.submit(send_keys, "^v")
            fut.result(timeout=3.0)
        time.sleep(0.15)
    except Exception as e:
        print(f"  [WARN] paste_unicode skipped: {e}", flush=True)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("exe", nargs="?", default=str(Path("target/debug/Mist.exe")))
    ap.add_argument("--out", default=str(Path("target/gui_ux_audit")))
    ap.add_argument("--keep-open", action="store_true")
    args = ap.parse_args()

    use_local_openssh_test_config()
    reload_ssh_test_config()
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    audit = Audit()

    print("==> SSH preflight", flush=True)
    ssh_preflight()

    print("==> Launch Mist GUI (auto-connect Local Test SSH)", flush=True)
    try:
        proc, hwnd = launch_mist_gui(
            args.exe,
            session_name=LOCAL_TEST_SESSION,
            auto_connect=True,
            title_sub="Mist",
            extra_env={"MISTTERM_GUI_AUTOMATION": "1", "MISTTERM_GUI_LOCAL_SSH": "1"},
        )
        app = Application(backend="uia").connect(process=proc.pid)
        focus_win(app, hwnd)
        dismiss_new_session_dialog(repeats=1)
        time.sleep(0.3)
        shot(hwnd, out_dir, "01_connected", audit)
        audit.step_ok("launch_and_auto_connect")
    except Exception as e:
        audit.step_fail("launch_and_auto_connect", str(e))
        print(f"FATAL: {e}", flush=True)
        return 1

    try:
        # 已由 launch_mist_gui 自动连接；再点一次侧栏确认可复连/聚焦
        print("==> Re-focus session via sidebar", flush=True)
        try:
            connect_local_session(
                hwnd, proc.pid, name=LOCAL_TEST_SESSION, verify=False, wait=2.0
            )
            time.sleep(0.6)
            shot(hwnd, out_dir, "02_sidebar_refocus", audit)
            audit.step_ok("sidebar_refocus")
        except Exception as e:
            audit.step_fail("sidebar_refocus", str(e))
            shot(hwnd, out_dir, "02_sidebar_fail", audit)

        # 2) 终端输入：像人一样敲命令
        print("==> Terminal typing", flush=True)
        try:
            focus_terminal_area(hwnd)
            send_terminal_line("echo UX_AUDIT_MARK")
            time.sleep(0.6)
            send_terminal_line("dir")
            time.sleep(0.8)
            shot(hwnd, out_dir, "03_after_commands", audit)
            audit.step_ok("terminal_echo_and_dir")
        except Exception as e:
            audit.step_fail("terminal_echo_and_dir", str(e))

        # 3) 拖选文本（人类常从行中拖一段）
        print("==> Drag select in terminal", flush=True)
        try:
            cl, ct, cr, cb = client_rect(hwnd)
            # 终端中部偏左，拖一小段
            x1 = cl + int((cr - cl) * 0.28)
            y1 = ct + int((cb - ct) * 0.55)
            x2 = cl + int((cr - cl) * 0.55)
            y2 = y1
            drag_select(x1, y1, x2, y2, pause=0.2)
            time.sleep(0.25)
            if is_hung(hwnd):
                raise RuntimeError("Mist hung after drag_select")
            shot(hwnd, out_dir, "04_drag_select", audit)
            if is_hung(hwnd):
                raise RuntimeError("Mist hung after 04 shot (PrintWindow?)")
            # 点一下清选区，避免灰条残留干扰后续
            focus_terminal_area(hwnd)
            time.sleep(0.15)
            audit.step_ok("drag_select_only")
            audit.note(
                "terminal_selection",
                "仅拖选截图；复制快捷键跳过（防剪贴板死锁）。请人工确认选区高亮",
                "note",
            )
        except Exception as e:
            audit.step_fail("drag_select", str(e))
            if is_hung(hwnd):
                audit.note("hang", "Mist 未响应，中止后续步骤", "high")
                raise SystemExit(2)

        # 4) 点活动栏打开 AI
        print("==> AI panel via rail click", flush=True)
        try:
            # 先点终端清掉拖选灰条
            focus_terminal_area(hwnd)
            time.sleep(0.2)
            open_ai_panel(hwnd, app)
            time.sleep(0.5)
            if is_hung(hwnd):
                raise RuntimeError("Mist hung after open AI")
            shot(hwnd, out_dir, "05_ai_opened", audit)
            click_ai_draft(hwnd)
            time.sleep(0.3)
            try:
                import concurrent.futures

                with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                    fut = pool.submit(
                        lambda: send_keys("hello", with_spaces=True, pause=0.02)
                    )
                    fut.result(timeout=4.0)
            except Exception as te:
                print(f"  [WARN] type skipped: {te}", flush=True)
            time.sleep(0.2)
            shot(hwnd, out_dir, "06_ai_draft_type", audit)
            focus_terminal_area(hwnd)
            time.sleep(0.25)
            send_terminal_line("echo NO_CTRL_LEAK")
            time.sleep(0.8)
            shot(hwnd, out_dir, "07_no_ctrl_leak", audit)
            if is_hung(hwnd):
                audit.note("hang", "Mist 在 NO_CTRL_LEAK 后短暂未响应(已继续)", "medium")
            audit.step_ok("ai_open_and_no_ctrl_leak")
            audit.note(
                "ctrl_leak",
                "请目视 05 是否有右侧 AI dock；07 应有 NO_CTRL_LEAK 且无会话名当命令",
                "note",
            )
        except Exception as e:
            audit.step_fail("ai_panel_input", str(e))
            try:
                shot(hwnd, out_dir, "05_ai_fail", audit)
            except Exception:
                pass
            if is_hung(hwnd):
                audit.note("hang", "Mist 未响应，中止后续步骤", "high")
                raise SystemExit(2)

        # 5) AI Enter 发送
        print("==> AI Enter send habit", flush=True)
        try:
            open_ai_panel(hwnd, app)
            time.sleep(0.5)
            click_ai_draft(hwnd)
            time.sleep(0.3)
            send_keys("ping test", with_spaces=True, pause=0.02)
            time.sleep(0.15)
            send_keys("{ENTER}")
            time.sleep(0.8)
            shot(hwnd, out_dir, "08_ai_after_enter", audit)
            audit.step_ok("ai_enter_pressed")
        except Exception as e:
            audit.step_fail("ai_enter", str(e))

        # 6) 终端焦点被 AI 抢走后，再点终端应能立刻输入
        print("==> Return focus to terminal", flush=True)
        try:
            focus_terminal_area(hwnd)
            time.sleep(0.25)
            send_terminal_line("echo BACK_TO_TERM")
            time.sleep(0.6)
            shot(hwnd, out_dir, "09_back_to_terminal", audit)
            audit.step_ok("refocus_terminal_after_ai")
        except Exception as e:
            audit.step_fail("refocus_terminal_after_ai", str(e))

        # 7) Escape 关闭层叠
        print("==> Escape dismiss", flush=True)
        try:
            send_keys("{ESC}")
            time.sleep(0.2)
            send_keys("{ESC}")
            time.sleep(0.2)
            shot(hwnd, out_dir, "10_after_esc", audit)
            audit.step_ok("escape_dismiss")
        except Exception as e:
            audit.step_fail("escape_dismiss", str(e))

        audit.note(
            "focus_model",
            "打开 AI 时应自动聚焦草稿；Ctrl+Shift 应用快捷键不应再漏进 PTY",
            "note",
        )
        audit.note(
            "copy_shortcut",
            "Win 终端复制仍是 Ctrl+Shift+C（对标 Windows Terminal）",
            "note",
        )

    finally:
        if not args.keep_open and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except Exception:
                proc.kill()

    # 报告
    report_path = out_dir / "REPORT.md"
    lines = [
        "# MistTerm GUI UX 习惯走查",
        "",
        f"- 会话: `{LOCAL_TEST_SESSION}`",
        f"- 截图目录: `{out_dir}`",
        f"- 通过步骤: {len(audit.ok_steps)}",
        f"- 失败步骤: {len(audit.fail_steps)}",
        "",
        "## 步骤结果",
        "",
    ]
    for s in audit.ok_steps:
        lines.append(f"- OK: {s}")
    for name, err in audit.fail_steps:
        lines.append(f"- FAIL: {name} — {err}")
    lines += ["", "## 发现", ""]
    for f in audit.findings:
        lines.append(f"- **[{f.severity}]** `{f.area}`: {f.detail}")
    lines += ["", "## 截图", ""]
    for p in audit.shots:
        lines.append(f"- `{p}`")
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"\n==> Wrote {report_path}", flush=True)
    print("\n=== Summary ===", flush=True)
    print(f"  ok={len(audit.ok_steps)} fail={len(audit.fail_steps)} findings={len(audit.findings)}", flush=True)
    return 1 if audit.fail_steps else 0


if __name__ == "__main__":
    raise SystemExit(main())
