#!/usr/bin/env python3
"""MistTerm GUI 测试截图与窗口采样辅助。"""

from __future__ import annotations

import ctypes
import subprocess
import time
from pathlib import Path

from pywinauto import Desktop

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
try:
    shcore = ctypes.windll.shcore
except Exception:
    shcore = None

SHOT_DIR = Path(__file__).resolve().parent.parent / "target" / "gui-screenshots"
MANUAL_SHOT_DIR = Path(__file__).resolve().parent.parent / "docs" / "manual" / "screenshots"

PW_RENDERFULLCONTENT = 0x00000002
SRCCOPY = 0x00CC0020
BI_RGB = 0
DIB_RGB_COLORS = 0
SW_MAXIMIZE = 3


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


def find_mist_window(
    proc: subprocess.Popen[bytes],
    *,
    timeout: float = 60.0,
    title_sub: str = "Mist",
) -> int:
    """等待 Mist 主窗口出现；按进程 PID 匹配，避免误选其它窗口。"""
    deadline = time.time() + timeout
    last_titles: list[str] = []
    while time.time() < deadline:
        code = proc.poll()
        if code is not None:
            raise RuntimeError(f"Mist 进程已退出 (code={code})")
        for w in Desktop(backend="uia").windows():
            title = w.window_text()
            if title_sub not in title:
                continue
            try:
                if int(w.process_id()) == proc.pid:
                    return int(w.handle)
            except Exception:
                return int(w.handle)
            last_titles.append(title)
        try:
            from pywinauto import findwindows

            wins = findwindows.find_windows(process=proc.pid, title_re=f".*{title_sub}.*")
            if wins:
                return int(wins[0])
        except Exception:
            pass
        time.sleep(0.25)
    hint = f" 最近可见标题: {last_titles[-5:]}" if last_titles else ""
    raise RuntimeError(
        f"未找到 Mist 窗口 (pid={proc.pid}, 超时 {timeout}s){hint}"
    )


def get_pixel(x: int, y: int) -> tuple[int, int, int]:
    hdc = user32.GetDC(0)
    try:
        color = gdi32.GetPixel(hdc, int(x), int(y))
        if color == 0xFFFFFFFF:
            return (0, 0, 0)
        return color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF
    finally:
        user32.ReleaseDC(0, hdc)


def enable_dpi_awareness() -> None:
    """高 DPI 屏上避免截图发糊。"""
    if shcore is not None:
        try:
            shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_AWARE_V2
            return
        except Exception:
            pass
    try:
        user32.SetProcessDPIAware()
    except Exception:
        pass


def window_rect(hwnd: int) -> tuple[int, int, int, int]:
    rect = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return rect.l, rect.t, rect.r, rect.b


def maximize_window(hwnd: int) -> None:
    user32.ShowWindow(hwnd, SW_MAXIMIZE)
    time.sleep(0.6)


def _capture_print_window(hwnd: int) -> "Image.Image":
    from PIL import Image

    left, top, right, bottom = window_rect(hwnd)
    w, h = max(1, right - left), max(1, bottom - top)
    hwnd_dc = user32.GetWindowDC(hwnd)
    mfc_dc = gdi32.CreateCompatibleDC(hwnd_dc)
    save_bitmap = gdi32.CreateCompatibleBitmap(hwnd_dc, w, h)
    gdi32.SelectObject(mfc_dc, save_bitmap)
    ok = user32.PrintWindow(hwnd, mfc_dc, PW_RENDERFULLCONTENT)
    if not ok:
        user32.PrintWindow(hwnd, mfc_dc, 0)

    class BITMAPINFOHEADER(ctypes.Structure):
        _fields_ = [
            ("biSize", ctypes.c_uint32),
            ("biWidth", ctypes.c_int32),
            ("biHeight", ctypes.c_int32),
            ("biPlanes", ctypes.c_uint16),
            ("biBitCount", ctypes.c_uint16),
            ("biCompression", ctypes.c_uint32),
            ("biSizeImage", ctypes.c_uint32),
            ("biXPelsPerMeter", ctypes.c_int32),
            ("biYPelsPerMeter", ctypes.c_int32),
            ("biClrUsed", ctypes.c_uint32),
            ("biClrImportant", ctypes.c_uint32),
        ]

    bmi = BITMAPINFOHEADER()
    bmi.biSize = ctypes.sizeof(BITMAPINFOHEADER)
    bmi.biWidth = w
    bmi.biHeight = -h
    bmi.biPlanes = 1
    bmi.biBitCount = 32
    bmi.biCompression = BI_RGB
    buf = ctypes.create_string_buffer(w * h * 4)
    gdi32.GetDIBits(mfc_dc, save_bitmap, 0, h, buf, ctypes.byref(bmi), DIB_RGB_COLORS)
    gdi32.DeleteObject(save_bitmap)
    gdi32.DeleteDC(mfc_dc)
    user32.ReleaseDC(hwnd, hwnd_dc)
    return Image.frombuffer("RGBA", (w, h), buf, "raw", "BGRA", 0, 1).convert("RGB")


def screenshot(
    hwnd: int,
    label: str,
    shot_dir: Path | None = None,
    *,
    stable_name: str | None = None,
    maximize: bool = True,
) -> Path:
    enable_dpi_awareness()
    out_dir = shot_dir or SHOT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in label)
    if stable_name:
        fname = "".join(c if c.isalnum() or c in "-_" else "_" for c in stable_name)
        path = out_dir / f"{fname}.png"
    else:
        path = out_dir / f"{safe}_{int(time.time() * 1000)}.png"

    if maximize:
        maximize_window(hwnd)
        time.sleep(0.25)

    try:
        img = _capture_print_window(hwnd)
        img.save(str(path), format="PNG", optimize=False)
    except Exception:
        cl, ct, cr, cb = client_rect(hwnd)
        try:
            from PIL import ImageGrab

            img = ImageGrab.grab(bbox=(cl, ct, cr, cb))
            img.save(path, format="PNG", optimize=False)
        except Exception as e:
            path.write_text(f"screenshot failed: {e}", encoding="utf-8")
            return path

    print(f"    [截图] {path} ({img.size[0]}x{img.size[1]})", flush=True)
    return path


def modal_sample_pixels(cl: int, ct: int, cr: int, cb: int, scale: float) -> list[tuple[int, int, int]]:
    mx = (cl + cr) // 2
    my = (ct + cb) // 2
    top = my - int(195 * scale)
    pts = [
        (mx, top + int(36 * scale)),
        (mx, top + int(120 * scale)),
        (mx, top + int(260 * scale)),
    ]
    return [get_pixel(x, y) for x, y in pts]


def pixel_signature(pixels: list[tuple[int, int, int]]) -> int:
    return sum(r + g + b for r, g, b in pixels)


def pixels_differ(
    a: list[tuple[int, int, int]], b: list[tuple[int, int, int]], threshold: int = 40
) -> bool:
    if len(a) != len(b):
        return True
    return sum(abs(x - y) for p, q in zip(a, b) for x, y in zip(p, q)) > threshold


def modal_header_pixel(cl: int, ct: int, cr: int, cb: int, scale: float) -> tuple[int, int, int]:
    mx = (cl + cr) // 2
    my = (ct + cb) // 2
    top = my - int(195 * scale)
    return get_pixel(mx, top + int(36 * scale))


def accent_color_detected(r: int, g: int, b: int) -> bool:
    """Mist 主按钮橙色：R 高、G 中、B 低。"""
    return r > 110 and g > 45 and b < 95 and r > g > b


def disabled_button_detected(r: int, g: int, b: int) -> bool:
    """禁用「Save & connect」灰按钮仍在弹窗内。"""
    return 35 < r < 130 and 35 < g < 130 and 35 < b < 130 and max(r, g, b) - min(r, g, b) < 35


def save_button_pixel(cl: int, ct: int, cr: int, cb: int, scale: float) -> tuple[int, int, int]:
    w, h = cr - cl, cb - ct
    x = cl + int(w * 0.60)
    y = ct + int(h * 0.775)
    return get_pixel(x, y)


def new_session_modal_seems_open(
    cl: int,
    ct: int,
    cr: int,
    cb: int,
    scale: float,
    baseline: list[tuple[int, int, int]] | None,
) -> bool:
    w, h = cr - cl, cb - ct
    hr, hg, hb = get_pixel(cl + int(w * 0.55), ct + int(h * 0.285))
    if hr + hg + hb > 105:
        return True
    r, g, b = save_button_pixel(cl, ct, cr, cb, scale)
    if accent_color_detected(r, g, b) or disabled_button_detected(r, g, b):
        return True
    if baseline is not None:
        cur = modal_sample_pixels(cl, ct, cr, cb, scale)
        return pixels_differ(cur, baseline, threshold=55)
    return False


def sftp_dock_pixel(cl: int, ct: int, cr: int, cb: int, scale: float) -> tuple[int, int, int]:
    return get_pixel(cr - int(180 * scale), ct + int(220 * scale))


def sftp_dock_seems_open(
    cl: int,
    ct: int,
    cr: int,
    cb: int,
    scale: float,
    baseline: tuple[int, int, int] | None,
) -> bool:
    cur = sftp_dock_pixel(cl, ct, cr, cb, scale)
    if baseline is None:
        return cur[0] + cur[1] + cur[2] > 120
    return sum(abs(a - b) for a, b in zip(cur, baseline)) > 35
