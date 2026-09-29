#!/usr/bin/env python3
"""静态检查：egui 上下文锁重入。

egui 的 `Context::input/input_mut/memory/...` 会在闭包执行期间持有上下文 RwLock；
闭包内再调用 `ctx.*`（如 `ctx.wants_keyboard_input()`）会在同一把不可重入的锁上自锁，
UI 线程永久卡死（v1.1.6–v1.1.20 多标签切换冻死即此原因）。

用法：
    python3 scripts/check-egui-reentrancy.py [src_dir]   # 发现问题时退出码 1
    python3 scripts/check-egui-reentrancy.py --self-test

确属安全的行可在行尾加 `// egui-reentrancy: ok` 放行。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

LOCKING_METHODS = (
    "input",
    "input_mut",
    "memory",
    "memory_mut",
    "data",
    "data_mut",
    "options",
    "options_mut",
    "fonts",
)
CTX_NAME = r"\b(?:[A-Za-z_][A-Za-z0-9_]*_)?ctx\b"
RECEIVER = r"(?:\bui\.ctx\(\)|" + CTX_NAME + r"|\bui\b)"
OUTER_RE = re.compile(RECEIVER + r"\.(?:" + "|".join(LOCKING_METHODS) + r")\(\s*(?:move\s*)?\|")
INNER_RE = re.compile(r"(?:\bui\.ctx\(\)|" + CTX_NAME + r")\.[A-Za-z_]+\s*\(")
ALLOW_MARK = "egui-reentrancy: ok"


def blank_strings_and_comments(src: str) -> str:
    """把字符串 / 字符字面量 / 注释替换为空格（保留换行与长度），便于括号配对。"""
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            j = src.find("*/", i + 2)
            j = n if j < 0 else j + 2
            blank(i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            hashes = re.match(r"r(#*)\"", src[i:]).group(1)
            end = '"' + hashes
            j = src.find(end, i + 2 + len(hashes))
            j = n if j < 0 else j + len(end)
            blank(i, j)
            i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'":
            m = re.match(r"'(?:\\.[^']*|[^'\\])'", src[i:])
            if m:
                blank(i, i + m.end())
                i += m.end()
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def find_violations(src: str) -> list[tuple[int, str]]:
    clean = blank_strings_and_comments(src)
    lines = src.splitlines()
    hits: list[tuple[int, str]] = []
    for m in OUTER_RE.finditer(clean):
        open_idx = clean.rindex("(", m.start(), m.end())
        depth, j = 0, open_idx
        while j < len(clean):
            if clean[j] == "(":
                depth += 1
            elif clean[j] == ")":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        body_start = m.end()
        for im in INNER_RE.finditer(clean, body_start, j):
            line_no = clean.count("\n", 0, im.start()) + 1
            text = lines[line_no - 1] if line_no - 1 < len(lines) else ""
            if ALLOW_MARK in text:
                continue
            hits.append((line_no, text.strip()))
    return hits


def scan(root: Path) -> int:
    total = 0
    for path in sorted(root.rglob("*.rs")):
        for line_no, text in find_violations(path.read_text(encoding="utf-8")):
            total += 1
            print(f"{path}:{line_no}: egui ctx re-entered inside a locking closure: {text}")
    if total:
        print(
            f"\n{total} egui lock re-entrancy issue(s). Read the needed ctx state into a local "
            f"before the closure (see keyboard_shortcuts::consume_preferences_shortcut).",
            file=sys.stderr,
        )
        return 1
    print("egui re-entrancy check: OK")
    return 0


def self_test() -> int:
    bad = """
        let x = ctx.input_mut(|i| {
            if ctx.wants_keyboard_input() { return false; }
            true
        });
        ui.input(|i| ui.ctx().request_repaint());
        ctx.memory_mut(move |m| { egui_ctx.request_repaint(); });
    """
    good = """
        let wants = ctx.wants_keyboard_input();
        let x = ctx.input_mut(|i| { i.consume_key(m, k) && wants });
        ctx.input(|i| { let s = "ctx.foo()"; /* ctx.bar() */ s.len() });
        ctx.input(|i| i.modifiers.command); ctx.request_repaint();
        ctx.input(|i| { ctx.request_repaint(); }); // egui-reentrancy: ok
        let c = '('; let d = ')';
    """
    bad_hits = find_violations(bad)
    good_hits = find_violations(good)
    ok = len(bad_hits) == 3 and not good_hits
    print(f"self-test bad={bad_hits} good={good_hits}")
    print("self-test:", "OK" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--self-test":
        sys.exit(self_test())
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent / "src"
    sys.exit(scan(root))
