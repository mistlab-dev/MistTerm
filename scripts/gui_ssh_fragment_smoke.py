#!/usr/bin/env python3
"""SSH 连通后点测：等终端真有输出，再测命令片段 / 片段库 / 监控。

默认连 Local Test SSH（本机 mistterm_test@127.0.0.1）。
用法:
  python scripts/gui_ssh_fragment_smoke.py target/debug/Mist.exe
"""

from __future__ import annotations

import argparse
import os
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_automation_keys import (
    FRAGMENT_LIBRARY_CLOSE,
    OPEN_FRAGMENT_LIBRARY_NEW,
    TOGGLE_FRAGMENT_PANEL,
    dismiss_new_session_dialog,
)
from gui_common import (
    LOCAL_TEST_SESSION,
    click,
    client_rect,
    focus_terminal_area,
    launch_mist_gui,
    remote_exec,
    scale_for,
    send_terminal_line,
    use_local_openssh_test_config,
)
from gui_screen import maximize_window, screenshot
from PIL import Image
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
    time.sleep(0.2)


def shot(hwnd: int, out: Path, name: str) -> Path:
    p = screenshot(hwnd, name, shot_dir=out, stable_name=name, maximize=False)
    print(f"  [SHOT] {p}", flush=True)
    return Path(p)


def terminal_bright_count(path: Path, *, thresh: int = 60) -> int:
    """统计终端主体亮像素。提示符贴顶且偏左，裁切过右/过下会假黑。"""
    im = Image.open(path).convert("RGB")
    w, h = im.size
    crop = im.crop((int(w * 0.12), int(h * 0.08), int(w * 0.95), int(h * 0.55)))
    return sum(1 for r, g, b in crop.getdata() if max(r, g, b) >= thresh)


def right_dock_metrics(path: Path) -> tuple[int, int]:
    """右侧 dock：亮像素 + 面板灰底像素（区别于终端纯黑 ~18）。"""
    im = Image.open(path).convert("RGB")
    w, h = im.size
    right = im.crop((int(w * 0.70), int(h * 0.10), int(w * 0.995), int(h * 0.92)))
    bright = 0
    panelish = 0
    for r, g, b in right.getdata():
        m = max(r, g, b)
        if m >= 70:
            bright += 1
        if 28 <= m <= 65 and abs(r - g) < 10 and abs(g - b) < 10:
            panelish += 1
    return bright, panelish


def right_header_looks_like_fragment(path: Path) -> bool:
    """命令片段：有面板灰底，且列表区有「组合」等蓝标签。"""
    _bright, panelish = right_dock_metrics(path)
    if panelish < 5000:
        return False
    im = Image.open(path).convert("RGB")
    w, h = im.size
    zone = im.crop((int(w * 0.72), 200, w - 12, int(h * 0.55)))
    blue = sum(1 for r, g, b in zone.getdata() if b > r + 25 and b > 100)
    return blue >= 25


def right_header_looks_like_monitor(path: Path) -> bool:
    """监控：有面板灰底，但列表区无明显蓝标签（与片段区分）。"""
    _bright, panelish = right_dock_metrics(path)
    if panelish < 5000:
        return False
    return not right_header_looks_like_fragment(path)


def rail_selected_count(path: Path) -> int:
    im = Image.open(path).convert("RGB")
    h = im.size[1]
    rail = im.crop((8, 48, 62, min(h - 60, 560)))
    bands = 6
    bh = max(1, rail.height // bands)
    hits = 0
    for i in range(bands):
        band = rail.crop((0, i * bh, rail.width, (i + 1) * bh))
        px = list(band.getdata())
        blue = sum(1 for r, g, b in px if b > r + 25 and b > g + 15 and b > 70)
        if blue > max(12, len(px) * 0.04):
            hits += 1
    return hits


def rail_click(hwnd: int, index: int) -> None:
    """活动栏：btn=54、spacing=6、顶 pad=8，相对 client。"""
    cl, ct, _, _ = client_rect(hwnd)
    menu_h = 30
    btn_h = 54
    gap = 6
    pad = 8
    x = cl + 28
    y = ct + menu_h + pad + btn_h // 2 + index * (btn_h + gap)
    click(x, y, pause=0.55)


def open_fragment_via_view_menu(hwnd: int) -> None:
    cl, ct, _, _ = client_rect(hwnd)
    menu_y = ct + 16
    view_x = cl + 120
    click(view_x, menu_y, pause=0.45)
    # 视图菜单项（含一条 separator ≈6px）：
    # 0活动栏 1侧栏 2最大化 | 3SFTP 4转发 5片段 6监控 7助手
    # 片段 ≈ 菜单下 22 + 5*28 + 6 ≈ 168
    click(view_x + 36, menu_y + 168, pause=0.8)


def click_ssh_tab(hwnd: int) -> None:
    """点第一个会话标签（Local Test SSH / conhost），避免落在本机 PowerShell。"""
    cl, ct, _, _ = client_rect(hwnd)
    click(cl + 220, ct + 58, pause=0.45)


def ensure_fragment_panel(hwnd: int, out: Path) -> Path:
    """热键 → rail → 视图菜单，直到右侧确是「命令片段」而非监控。"""
    from gui_automation_keys import OPEN_FRAGMENT_PANEL

    attempts = [
        ("hotkey_caf", lambda: send_keys(OPEN_FRAGMENT_PANEL)),
        ("rail", lambda: rail_click(hwnd, 1)),
        ("view_menu", lambda: open_fragment_via_view_menu(hwnd)),
    ]
    last = out / "03_fragment_panel.png"
    for name, action in attempts:
        # 先关掉可能误开的监控，再开片段
        if name != "hotkey_f11":
            pass
        action()
        time.sleep(1.0)
        last = shot(hwnd, out, "03_fragment_panel")
        bright, panelish = right_dock_metrics(last)
        is_frag = right_header_looks_like_fragment(last)
        is_mon = right_header_looks_like_monitor(last)
        print(
            f"  [VIS] fragment via {name}: bright={bright} panelish={panelish} "
            f"frag={is_frag} mon={is_mon}",
            flush=True,
        )
        if bright >= 800 and panelish >= 1500 and is_frag and not is_mon:
            return last
        if is_mon:
            print("  [VIS] 右侧是系统监控，继续尝试下一种打开方式", flush=True)
    raise RuntimeError(f"命令片段侧栏未打开（或误开成监控）→ {last}")


def wait_shell_ready(hwnd: int, out: Path, *, max_wait: float = 14.0) -> Path:
    focus_terminal_area(hwnd)
    time.sleep(0.3)
    deadline = time.time() + max_wait
    attempt = 0
    last = out / "01b_wait_shell.png"
    while time.time() < deadline:
        attempt += 1
        focus_terminal_area(hwnd)
        send_keys("{ENTER}")
        time.sleep(0.25)
        if attempt in (1, 3, 5):
            send_terminal_line(f"echo SHELL_READY_{attempt}")
        time.sleep(1.1)
        last = shot(hwnd, out, f"01b_wait_shell_{attempt}")
        bright = terminal_bright_count(last)
        print(f"  [WAIT] shell attempt={attempt} bright={bright}", flush=True)
        if bright >= 120:
            ready = out / "01b_shell_ready.png"
            Image.open(last).save(ready)
            print(f"  [SHOT] {ready}", flush=True)
            return ready
    raise RuntimeError(f"等待 {max_wait:.0f}s 后终端仍无可见输出 → {last}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("exe", nargs="?", default="target/debug/Mist.exe")
    ap.add_argument("--out", default="target/gui_ssh_fragment")
    ap.add_argument("--keep-open", action="store_true")
    ap.add_argument("--timeout", type=float, default=40.0)
    ap.add_argument("--shell-wait", type=float, default=14.0)
    args = ap.parse_args()

    for k in (
        "MISTTERM_TEST_SSH_HOST",
        "MISTTERM_TEST_SSH_PASSWORD",
        "MISTTERM_TEST_SSH_USER",
        "MISTTERM_TEST_SSH_SESSION",
    ):
        os.environ.pop(k, None)
    os.environ["MISTTERM_GUI_LOCAL_SSH"] = "1"
    use_local_openssh_test_config()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob("*.png"):
        try:
            old.unlink()
        except OSError:
            pass
    report = Report()

    print(
        f"==> Launch + auto-connect {LOCAL_TEST_SESSION} (mistterm_test@127.0.0.1)",
        flush=True,
    )
    try:
        proc, hwnd = launch_mist_gui(
            args.exe,
            session_name=LOCAL_TEST_SESSION,
            auto_connect=True,
            window_timeout=args.timeout,
            connect_wait=22.0,
            title_sub="Mist",
            extra_env={"MISTTERM_GUI_LOCAL_SSH": "1"},
        )
        app = Application(backend="uia").connect(process=proc.pid)
        focus_win(app, hwnd)
        maximize_window(hwnd)
        time.sleep(0.7)
        dismiss_new_session_dialog(repeats=2)
        time.sleep(0.4)
        shot(hwnd, out, "01_connected")
        report.ok("ssh_auto_connect")
    except Exception as e:
        print(f"FATAL: {e}", flush=True)
        return 1

    try:
        cl, ct, cr, cb = client_rect(hwnd)
        s = scale_for(cl, cr)

        print("==> Focus SSH tab (not local PowerShell)", flush=True)
        try:
            focus_win(app, hwnd)
            click_ssh_tab(hwnd)
            time.sleep(0.4)
            shot(hwnd, out, "01a_ssh_tab")
        except Exception as e:
            report.note(f"click_ssh_tab: {e}")

        print("==> Wait for shell prompt/output", flush=True)
        try:
            ready = wait_shell_ready(hwnd, out, max_wait=args.shell_wait)
            bright = terminal_bright_count(ready)
            report.ok(f"shell_ready_visible(bright={bright})")
        except Exception as e:
            report.fail("shell_ready_visible", str(e))

        print("==> Terminal echo/hostname", flush=True)
        try:
            focus_win(app, hwnd)
            click_ssh_tab(hwnd)
            focus_terminal_area(hwnd)
            marker = f"FRAG_GUI_{int(time.time())}"
            send_terminal_line(f"echo {marker}")
            time.sleep(1.2)
            send_terminal_line("hostname")
            time.sleep(1.0)
            send_terminal_line("echo AFTER_HOST")
            time.sleep(1.4)
            p = shot(hwnd, out, "02_terminal_cmds")
            bright = terminal_bright_count(p)
            if bright < 80:
                raise RuntimeError(f"敲命令后终端仍近全黑 bright={bright}")
            code, out_s, err = remote_exec("echo ok")
            if code != 0 or out_s.strip() != "ok":
                raise RuntimeError(f"remote echo failed: {err or out_s}")
            report.ok(f"terminal_echo_hostname(bright={bright})")
        except Exception as e:
            report.fail("terminal_cmds", str(e))

        print("==> Open fragment panel", flush=True)
        try:
            focus_win(app, hwnd)
            click_ssh_tab(hwnd)
            p = ensure_fragment_panel(hwnd, out)
            rail_n = rail_selected_count(p)
            report.note(f"fragment rail_selected≈{rail_n}")
            if rail_n > 1:
                report.fail("rail_exclusive", f"活动栏疑似多选中 ≈{rail_n}")
            else:
                report.ok("rail_exclusive_fragment")
            if not right_header_looks_like_fragment(p):
                report.fail("fragment_panel_open", "截图右侧不是命令片段（可能是监控假绿）")
            else:
                report.ok("fragment_panel_open")
            # 点列表区域尝试插入/执行
            click(cr - int(150 * s), ct + int(260 * s), pause=0.35)
            click(cr - int(150 * s), ct + int(260 * s), pause=0.12)
            click(cr - int(150 * s), ct + int(260 * s), pause=0.55)
            click(cr - int(90 * s), ct + int(330 * s), pause=0.45)
            time.sleep(0.7)
            focus_terminal_area(hwnd)
            time.sleep(0.5)
            shot(hwnd, out, "04_fragment_after_click")
            report.ok("fragment_list_click_attempted")
        except Exception as e:
            report.fail("fragment_panel", str(e))

        print("==> Fragment library (Ctrl+Shift+F7)", flush=True)
        try:
            focus_win(app, hwnd)
            send_keys(OPEN_FRAGMENT_LIBRARY_NEW)
            time.sleep(1.2)
            p = shot(hwnd, out, "05_fragment_library")
            im = Image.open(p).convert("RGB")
            w, h = im.size
            mid = im.crop((int(w * 0.22), int(h * 0.12), int(w * 0.78), int(h * 0.88)))
            mbright = sum(1 for r, g, b in mid.getdata() if max(r, g, b) >= 90)
            if mbright < 500:
                raise RuntimeError(f"未见片段库新建表单 mid_bright={mbright}")
            report.ok(f"fragment_library_form(mid_bright={mbright})")
            send_keys(FRAGMENT_LIBRARY_CLOSE)
            time.sleep(0.45)
            send_keys("{ESC}")
            time.sleep(0.25)
        except Exception as e:
            report.fail("fragment_library", str(e))

        print("==> Ctrl+K (blur terminal first for palette)", flush=True)
        try:
            focus_win(app, hwnd)
            # 点顶栏菜单失焦终端，避免 Ctrl+K 进 PTY
            click(cl + 40, ct + 16, pause=0.35)
            send_keys("{ESC}")
            time.sleep(0.25)
            send_keys("^k")
            time.sleep(0.9)
            p = shot(hwnd, out, "06_ctrl_k")
            im = Image.open(p).convert("RGB")
            w, h = im.size
            mid = im.crop((int(w * 0.28), int(h * 0.18), int(w * 0.72), int(h * 0.55)))
            mbright = sum(1 for r, g, b in mid.getdata() if max(r, g, b) >= 90)
            report.note(f"ctrl_k mid_bright={mbright}")
            if mbright < 200:
                report.fail("ctrl_k", "失焦后 Ctrl+K 未见命令面板（或仍落入终端）")
            else:
                report.ok("ctrl_k_command_palette")
            send_keys("{ESC}")
            time.sleep(0.3)
        except Exception as e:
            report.fail("ctrl_k", str(e))

        print("==> Monitor panel", flush=True)
        try:
            focus_win(app, hwnd)
            click_ssh_tab(hwnd)
            # 视图菜单打开监控（避免活动栏像素偏移）
            cl2, ct2, _, _ = client_rect(hwnd)
            click(cl2 + 120, ct2 + 16, pause=0.4)
            click(cl2 + 160, ct2 + 16 + 7 * 28 + 18, pause=0.7)
            time.sleep(2.5)
            p = shot(hwnd, out, "07_monitor")
            bright, panelish = right_dock_metrics(p)
            is_mon = right_header_looks_like_monitor(p) and not right_header_looks_like_fragment(p)
            report.note(f"monitor bright={bright} panelish={panelish} is_mon={is_mon}")
            if bright < 800 or panelish < 1000 or not is_mon:
                raise RuntimeError("监控面板未打开或内容不对")
            rail_n = rail_selected_count(p)
            if rail_n > 1:
                report.fail("rail_exclusive_monitor", f"活动栏疑似多选中 ≈{rail_n}")
            else:
                report.ok("rail_exclusive_monitor")
            report.ok("monitor_panel_open")
        except Exception as e:
            report.fail("monitor", str(e))

        print("==> Terminal still alive", flush=True)
        try:
            focus_win(app, hwnd)
            click_ssh_tab(hwnd)
            # 关右 dock：再点一次监控菜单项或 Esc
            send_keys("{ESC}")
            time.sleep(0.35)
            focus_terminal_area(hwnd)
            send_terminal_line("echo STILL_ALIVE")
            time.sleep(1.4)
            p = shot(hwnd, out, "08_still_alive")
            bright = terminal_bright_count(p)
            if bright < 80:
                raise RuntimeError(f"仍全黑 bright={bright}")
            report.ok(f"session_still_alive(bright={bright})")
        except Exception as e:
            report.fail("still_alive", str(e))

    finally:
        print("\n=== SSH + fragment smoke ===", flush=True)
        print("  host: mistterm_test@127.0.0.1 (Local Test SSH)", flush=True)
        print(f"  passed: {len(report.passed)}  failed: {len(report.failed)}", flush=True)
        for n in report.passed:
            print(f"  + {n}", flush=True)
        for n, e in report.failed:
            print(f"  - FAIL {n}: {e}", flush=True)
        for n in report.notes:
            print(f"  · {n}", flush=True)
        print(f"  shots → {out.resolve()}", flush=True)
        if not args.keep_open:
            try:
                proc.terminate()
            except Exception:
                pass
            time.sleep(0.3)
            try:
                proc.kill()
            except Exception:
                pass
        else:
            print("  --keep-open: Mist 进程保留", flush=True)

    return 1 if report.failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
