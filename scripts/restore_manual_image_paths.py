#!/usr/bin/env python3
"""将操作手册中的 base64 图片还原为相对路径引用。"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MD = ROOT / "MistTerm_操作手册.md"
MANIFEST = ROOT / "docs" / "manual" / "screenshots" / "manifest.json"
BASE64_IMG = re.compile(r"!\[([^\]]*)\]\(data:image/png;base64,[^)]+\)")


def load_manifest() -> dict[str, str]:
    items = json.loads(MANIFEST.read_text(encoding="utf-8"))
    by_title: dict[str, str] = {}
    by_id: dict[str, str] = {}
    for item in items:
        by_title[item["title"]] = item["file"]
        by_id[item["id"]] = item["file"]
        num = item["id"].split("-", 1)[0]
        by_id[num] = item["file"]
    # 手册正文里使用的 alt 与 manifest 标题略有差异
    aliases = {
        "终端会话": "03-terminal-session",
        "终端内查找": "15-terminal-find",
        "AI 设置": "08-ai-settings-dialog",
        "视图菜单": "12-menu-view",
        "关于 Mist": "14-about",
        "SFTP 双栏浏览器": "04-sftp-panel",
        "命令片段底栏面板": "11-panel-snippets",
    }
    for alt, fid in aliases.items():
        by_title[alt] = by_id.get(fid, f"docs/manual/screenshots/{fid}.png")
    return {**by_title, **{k: v for k, v in by_id.items()}}


def restore_paths(text: str, lookup: dict[str, str]) -> tuple[str, int]:
    count = 0

    def repl(m: re.Match[str]) -> str:
        nonlocal count
        alt = m.group(1)
        path = lookup.get(alt)
        if not path:
            print(f"  [警告] 未映射 alt: {alt!r}", file=sys.stderr)
            return m.group(0)
        count += 1
        return f"![{alt}]({path})"

    return BASE64_IMG.sub(repl, text), count


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, default=DEFAULT_MD)
    args = parser.parse_args()
    if not args.input.is_file():
        print(f"找不到 {args.input}", file=sys.stderr)
        return 2
    lookup = load_manifest()
    text = args.input.read_text(encoding="utf-8")
    restored, n = restore_paths(text, lookup)
    args.input.write_text(restored, encoding="utf-8")
    size_kb = args.input.stat().st_size / 1024
    print(f"已还原 {n} 张图片为相对路径 -> {args.input} ({size_kb:.0f} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
