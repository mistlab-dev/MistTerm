#!/usr/bin/env python3
"""MistTerm GUI 测试截图与窗口采样辅助。"""

from __future__ import annotations

import ctypes
import time
from pathlib import Path

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

SHOT_DIR = Path(__file__).resolve().parent.parent / "target" / "gui-screenshots"


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


def get_pixel(x: int, y: int) -> tuple[int, int, int]:
    hdc = user32.GetDC(0)
    try:
        color = gdi32.GetPixel(hdc, int(x), int(y))
        if color == 0xFFFFFFFF:
            return (0, 0, 0)
        return color & 0xFF, (color >> 8) & 0xFF, (color >> 16) & 0xFF
    finally:
        user32.ReleaseDC(0, hdc)


def screenshot(hwnd: int, label: str, shot_dir: Path | None = None) -> Path:
    out_dir = shot_dir or SHOT_DIR
    out_dir.mkdir(parents=True, exist_ok=True)
    safe = "".join(c if c.isalnum() or c in "-_" else "_" for c in label)
    path = out_dir / f"{safe}_{int(time.time() * 1000)}.png"
    cl, ct, cr, cb = client_rect(hwnd)

    try:
        from pywinauto import Application

        img = Application(backend="uia").connect(handle=hwnd).window(handle=hwnd).capture_as_image()
        img.save(str(path))
    except Exception:
        try:
            from PIL import ImageGrab

            img = ImageGrab.grab(bbox=(cl, ct, cr, cb))
            img.save(path)
        except Exception as e:
            path.write_text(f"screenshot failed: {e}", encoding="utf-8")
            return path

    print(f"    [截图] {path}", flush=True)
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
