#!/usr/bin/env python3
"""全量顶栏菜单截图验证：校准坐标 → 逐项点击 → 落盘 → 写报告。"""

from __future__ import annotations

import json
import subprocess
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from gui_screen import (  # noqa: E402
    enable_dpi_awareness,
    find_mist_window,
    screenshot,
    window_rect,
)
from PIL import Image, ImageChops, ImageStat  # noqa: E402
import ctypes  # noqa: E402

user32 = ctypes.windll.user32

EXE = Path(__file__).resolve().parent.parent / "target" / "debug" / "Mist.exe"
OUT = Path(r"D:\work\mist-website\.tmp-browser\mist-qa\menu-all")
ITEM_H = 26
SEP_H = 8
MENU_Y = 48
FIRST_Y = 98  # calibrated 1200x800 non-immersive (Win custom chrome)
WIN_W, WIN_H = 1200, 800
WIN_X, WIN_Y = 80, 60
# MENU_X default after calibrate(): 51,78,120,160,200,240

VK_ESCAPE = 0x1B
VK_CONTROL = 0x11
KEYEVENTF_KEYUP = 0x0002
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004


@dataclass
class Case:
    name: str
    menu: int  # index into menu_xs
    row: int
    sep: int
    expect: str  # 期望界面关键词（中文），用于人工/启发式核对
    skip_reason: str = ""


CASES: list[Case] = [
    # open menus (row=-1 means only open)
    Case("00-open-terminal", 0, -1, 0, "新建会话"),
    Case("00-open-edit", 1, -1, 0, "在终端中搜索"),
    Case("00-open-view", 2, -1, 0, "沉浸式"),
    Case("00-open-tools", 3, -1, 0, "偏好设置"),
    Case("00-open-team", 4, -1, 0, "登录与账户"),
    Case("00-open-help", 5, -1, 0, "新人上手"),
    # Terminal
    Case("t01-new-session", 0, 0, 0, "新建会话"),
    Case("t02-new-tab", 0, 1, 0, "欢迎使用"),  # 无会话时可能只是欢迎页
    Case("t03-import-ssh", 0, 2, 0, "导入"),
    Case("t04-import-foreign", 0, 3, 0, "导入"),
    Case("t05-close-tab", 0, 4, SEP_H, "欢迎使用"),
    Case("t06-disconnect", 0, 5, SEP_H * 2, "欢迎使用"),
    Case("t07-reconnect", 0, 6, SEP_H * 2, "欢迎使用"),
    # Edit
    Case("e01-copy", 1, 0, 0, "欢迎使用"),
    Case("e02-paste", 1, 1, 0, "欢迎使用"),
    Case("e03-select-all", 1, 2, 0, "欢迎使用"),
    Case("e04-find", 1, 3, SEP_H, "搜索"),
    # View (skip immersive/maximize — 会打乱坐标)
    Case("v01-sidebar", 2, 1, 0, "欢迎使用"),
    Case("v02-sftp", 2, 3, SEP_H, "SFTP"),
    Case("v03-portfwd", 2, 4, SEP_H, "转发"),
    Case("v04-fragments", 2, 5, SEP_H, "片段"),
    Case("v05-monitor", 2, 6, SEP_H, "监控"),
    Case("v06-ai", 2, 7, SEP_H, "AI"),
    # Tools
    Case("o01-prefs", 3, 0, 0, "偏好设置"),
    Case("o02-ai-settings", 3, 1, 0, "AI 设置"),
    Case("o03-frag-lib", 3, 2, SEP_H, "命令片段库"),
    Case("o04-quick-frag", 3, 3, SEP_H, "快速"),
    Case("o05-history", 3, 4, SEP_H, "历史"),
    Case("o06-batch", 3, 5, SEP_H, "批量"),
    Case("o07-creds", 3, 6, SEP_H * 2, "凭证"),
    Case("o08-logs", 3, 7, SEP_H * 3, "日志"),
    # Team
    Case("m01-signin", 4, 0, 0, "偏好设置"),
    Case("m02-members", 4, 1, 0, "团队成员"),
    Case("m03-cloud", 4, 2, SEP_H, "同步"),
    # Help
    Case("h01-onboard", 5, 0, 0, "新人上手"),
    Case("h02-quickstart", 5, 1, 0, "快速入门"),
    Case("h03-shortcuts", 5, 2, 0, "快捷键"),
    Case("h04-ask", 5, 4, SEP_H, "问"),
    Case("h05-audit", 5, 5, SEP_H * 2, "审计"),
    Case("h06-agent", 5, 6, SEP_H * 2, "探针"),
    Case("h07-about", 5, 10, SEP_H * 4, "关于"),
]


def kill_mist() -> None:
    subprocess.run(["taskkill", "/F", "/IM", "Mist.exe"], capture_output=True)


def click(x: int, y: int) -> None:
    user32.SetCursorPos(int(x), int(y))
    time.sleep(0.05)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)


def tap_esc() -> None:
    user32.keybd_event(VK_ESCAPE, 0, 0, 0)
    user32.keybd_event(VK_ESCAPE, 0, KEYEVENTF_KEYUP, 0)


def focus(hwnd: int) -> None:
    user32.ShowWindow(hwnd, 9)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.12)


def set_geom(hwnd: int) -> None:
    user32.SetWindowPos(hwnd, 0, WIN_X, WIN_Y, WIN_W, WIN_H, 0x0040)
    time.sleep(0.35)


def dismiss(hwnd: int, n: int = 8) -> None:
    focus(hwnd)
    for _ in range(n):
        tap_esc()
        time.sleep(0.07)
    time.sleep(0.12)


def grab_top(hwnd: int, path: Path, height: int = 560) -> Image.Image:
    enable_dpi_awareness()
    left, top, right, bottom = window_rect(hwnd)
    from PIL import ImageGrab

    h = min(height, max(1, bottom - top))
    img = ImageGrab.grab(bbox=(left, top, right, top + h))
    path.parent.mkdir(parents=True, exist_ok=True)
    img.save(path)
    return img


def img_diff_score(a: Image.Image, b: Image.Image) -> float:
    a2 = a.convert("L").resize((320, 180))
    b2 = b.convert("L").resize((320, 180))
    if a2.size != b2.size:
        b2 = b2.resize(a2.size)
    diff = ImageChops.difference(a2, b2)
    return float(ImageStat.Stat(diff).mean[0])


def launch() -> tuple[subprocess.Popen[bytes], int]:
    kill_mist()
    time.sleep(0.4)
    proc = subprocess.Popen([str(EXE)])
    hwnd = find_mist_window(proc, timeout=25.0, title_sub="Mist")
    focus(hwnd)
    set_geom(hwnd)
    dismiss(hwnd, 6)
    # 关掉可能自动弹出的新人上手
    dismiss(hwnd, 4)
    return proc, hwnd


def calibrate_menus(hwnd: int) -> list[int]:
    """扫描菜单 X，返回 6 个中心点。"""
    dismiss(hwnd)
    left, top, _, _ = window_rect(hwnd)
    baseline_path = OUT / "_baseline.png"
    baseline = grab_top(hwnd, baseline_path, 220)

    scores: list[tuple[int, float]] = []
    for ox in range(45, 300, 5):
        dismiss(hwnd)
        left, top, _, _ = window_rect(hwnd)
        click(left + ox, top + MENU_Y)
        time.sleep(0.38)
        img = grab_top(hwnd, OUT / f"_scan_x{ox:03d}.png", 220)
        score = img_diff_score(baseline, img)
        scores.append((ox, score))
        print(f"  scan x={ox} diff={score:.2f}", flush=True)

    # 有下拉时 diff 明显更大
    thresh = max(8.0, sorted(s for _, s in scores)[len(scores) // 2] * 2.5)
    hits = [ox for ox, s in scores if s >= thresh]
    print(f"  thresh={thresh:.2f} hits={hits}", flush=True)
    if len(hits) < 6:
        # 回退到经验值
        print("  calibration weak — using fallback Xs", flush=True)
        return [72, 110, 145, 180, 215, 255]

    # 聚成 6 簇
    clusters: list[list[int]] = []
    cur: list[int] = []
    for ox in hits:
        if not cur or ox - cur[-1] <= 18:
            cur.append(ox)
        else:
            clusters.append(cur)
            cur = [ox]
    if cur:
        clusters.append(cur)
    # 取最大的 6 簇
    clusters = sorted(clusters, key=lambda c: -len(c))[:6]
    centers = sorted(int(sum(c) / len(c)) for c in clusters)
    if len(centers) < 6:
        print(f"  only {len(centers)} clusters — pad fallback", flush=True)
        fb = [72, 110, 145, 180, 215, 255]
        while len(centers) < 6:
            centers.append(fb[len(centers)])
        centers = sorted(centers)[:6]
    print(f"  menu_xs={centers}", flush=True)
    return centers


def run_case(hwnd: int, menu_xs: list[int], case: Case, baseline: Image.Image) -> dict:
    if case.skip_reason:
        return {"name": case.name, "status": "skip", "reason": case.skip_reason}

    dismiss(hwnd)
    left, top, _, _ = window_rect(hwnd)
    mx = left + menu_xs[case.menu]
    click(mx, top + MENU_Y)
    time.sleep(0.45)

    if case.row >= 0:
        iy = top + FIRST_Y + case.row * ITEM_H + case.sep
        click(mx + 42, iy)
        time.sleep(0.75)

    path = OUT / f"{case.name}.png"
    img = grab_top(hwnd, path, 600)
    diff = img_diff_score(baseline, img)
    # 打开菜单或对话框后应相对 baseline 有变化
    changed = diff >= 3.5
    status = "ok" if changed else "fail"
    print(f"  {case.name}: diff={diff:.2f} status={status} expect={case.expect}", flush=True)
    return {
        "name": case.name,
        "status": status,
        "diff": round(diff, 2),
        "expect": case.expect,
        "path": str(path),
        "size": path.stat().st_size,
    }


def main() -> int:
    enable_dpi_awareness()
    if OUT.exists():
        for p in OUT.glob("*"):
            if p.is_file():
                p.unlink()
    OUT.mkdir(parents=True, exist_ok=True)

    if not EXE.exists():
        print(f"missing exe: {EXE}", flush=True)
        return 2

    print("==> launch", flush=True)
    proc, hwnd = launch()
    try:
        print("==> calibrate", flush=True)
        menu_xs = calibrate_menus(hwnd)
        (OUT / "coords.json").write_text(
            json.dumps({"menu_xs": menu_xs, "menu_y": MENU_Y, "first_y": FIRST_Y}, indent=2),
            encoding="utf-8",
        )

        # 重新截 baseline（关闭菜单后）
        dismiss(hwnd)
        baseline = grab_top(hwnd, OUT / "00-main.png", 600)

        # 先拍 6 个 open 菜单确认校准
        results: list[dict] = []
        print("==> cases", flush=True)
        for case in CASES:
            # 每个 case 前确保几何
            set_geom(hwnd)
            results.append(run_case(hwnd, menu_xs, case, baseline))

        # 失败项：若 open-* 失败则整体重校一次再重跑失败项
        fails = [r for r in results if r["status"] == "fail"]
        open_fails = [r for r in fails if r["name"].startswith("00-open-")]
        if open_fails:
            print("==> recalibrate after open failures", flush=True)
            menu_xs = calibrate_menus(hwnd)
            dismiss(hwnd)
            baseline = grab_top(hwnd, OUT / "00-main.png", 600)
            retry_names = {r["name"] for r in fails}
            for case in CASES:
                if case.name not in retry_names:
                    continue
                set_geom(hwnd)
                r = run_case(hwnd, menu_xs, case, baseline)
                # 替换
                for i, old in enumerate(results):
                    if old["name"] == case.name:
                        results[i] = r
                        break

        passed = sum(1 for r in results if r["status"] == "ok")
        failed = sum(1 for r in results if r["status"] == "fail")
        skipped = sum(1 for r in results if r["status"] == "skip")
        report = {
            "menu_xs": menu_xs,
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "results": results,
        }
        (OUT / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        print("\n=== SUMMARY ===", flush=True)
        print(f"passed={passed} failed={failed} skipped={skipped}", flush=True)
        for r in results:
            if r["status"] == "fail":
                print(f"  FAIL {r['name']} diff={r.get('diff')} expect={r.get('expect')}", flush=True)
        return 1 if failed else 0
    finally:
        # 保留进程便于人工复核；如需关掉可取消注释
        # kill_mist()
        pass


if __name__ == "__main__":
    sys.exit(main())
