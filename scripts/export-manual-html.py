#!/usr/bin/env python3
"""将 docs/manual/MistTerm_操作手册.md 导出为完整 HTML（无第三方依赖）。"""

from __future__ import annotations

import html
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MD = ROOT / "docs" / "manual" / "MistTerm_操作手册.md"
OUT = ROOT / "docs" / "manual" / "MistTerm_操作手册.html"

STYLE = """
body { font-family: "Segoe UI", "Microsoft YaHei", sans-serif; max-width: 920px; margin: 2rem auto; padding: 0 1.5rem; line-height: 1.65; color: #222; }
h1 { border-bottom: 2px solid #ddd; padding-bottom: 0.3em; }
h1,h2,h3 { color: #111; margin-top: 1.6em; }
img { max-width: 100%; border: 1px solid #ddd; border-radius: 6px; box-shadow: 0 2px 8px rgba(0,0,0,.08); }
figure { margin: 1.2em 0; text-align: center; }
figcaption { font-size: 0.9em; color: #555; margin-top: 0.4em; }
p.caption { text-align: center; color: #555; font-size: 0.9em; margin: 0.2em 0 1.2em; }
code { background: #f4f4f4; padding: 0.12em 0.35em; border-radius: 4px; font-size: 0.92em; }
pre { background: #f4f4f4; padding: 1em; overflow-x: auto; border-radius: 6px; line-height: 1.45; white-space: pre; }
pre code { background: none; padding: 0; }
table { border-collapse: collapse; width: 100%; margin: 1em 0; }
th, td { border: 1px solid #ccc; padding: 0.5em 0.75em; text-align: left; vertical-align: top; }
th { background: #f0f0f0; font-weight: 600; }
blockquote { border-left: 4px solid #ccc; margin: 1em 0; padding: 0.5em 1em; color: #444; background: #fafafa; }
ul, ol { padding-left: 1.5em; margin: 0.6em 0; }
li { margin: 0.3em 0; }
a { color: #0969da; }
hr { border: none; border-top: 1px solid #ddd; margin: 2em 0; }
strong { font-weight: 600; }
"""


def inline_md(text: str) -> str:
    text = html.escape(text)
    text = re.sub(r"`([^`]+)`", r"<code>\1</code>", text)
    text = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", text)
    text = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r'<a href="\2">\1</a>', text)
    return text


def is_table_row(line: str) -> bool:
    s = line.strip()
    return s.startswith("|") and s.endswith("|") and "|" in s[1:-1]


def is_table_sep(line: str) -> bool:
    s = line.strip()
    if not is_table_row(line):
        return False
    cells = [c.strip() for c in s.strip("|").split("|")]
    return all(re.fullmatch(r":?-{3,}:?", c or "-") for c in cells)


def parse_table(lines: list[str], i: int) -> tuple[str, int]:
    rows: list[list[str]] = []
    while i < len(lines) and is_table_row(lines[i]):
        if is_table_sep(lines[i]):
            i += 1
            continue
        cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
        rows.append(cells)
        i += 1
    if not rows:
        return "", i
    header = rows[0]
    body = rows[1:] if len(rows) > 1 else []
    out = ["<table>", "<thead><tr>"]
    for c in header:
        out.append(f"<th>{inline_md(c)}</th>")
    out.append("</tr></thead>")
    if body:
        out.append("<tbody>")
        for row in body:
            out.append("<tr>")
            for c in row:
                out.append(f"<td>{inline_md(c)}</td>")
            out.append("</tr>")
        out.append("</tbody>")
    out.append("</table>")
    return "".join(out), i


def convert_md(text: str) -> str:
    lines = text.replace("\r\n", "\n").replace("\r", "\n").split("\n")
    out: list[str] = []
    i = 0
    in_code = False
    code_buf: list[str] = []

    while i < len(lines):
        line = lines[i]
        stripped = line.strip()

        if stripped.startswith("```"):
            if in_code:
                out.append("<pre><code>" + html.escape("\n".join(code_buf)) + "</code></pre>")
                code_buf.clear()
                in_code = False
            else:
                in_code = True
            i += 1
            continue

        if in_code:
            code_buf.append(line)
            i += 1
            continue

        if is_table_row(line):
            table_html, i = parse_table(lines, i)
            out.append(table_html)
            continue

        if stripped == "---":
            out.append("<hr>")
            i += 1
            continue

        if stripped.startswith("# "):
            out.append(f"<h1>{inline_md(stripped[2:])}</h1>")
            i += 1
            continue
        if stripped.startswith("## "):
            out.append(f"<h2>{inline_md(stripped[3:])}</h2>")
            i += 1
            continue
        if stripped.startswith("### "):
            out.append(f"<h3>{inline_md(stripped[4:])}</h3>")
            i += 1
            continue

        img = re.match(r"!\[([^\]]*)\]\(([^)]+)\)", stripped)
        if img:
            alt, src = img.group(1), img.group(2)
            out.append(
                f'<figure><img src="{html.escape(src)}" alt="{html.escape(alt)}">'
                f"<figcaption>{html.escape(alt)}</figcaption></figure>"
            )
            i += 1
            continue

        if stripped.startswith("> "):
            out.append(f"<blockquote><p>{inline_md(stripped[2:])}</p></blockquote>")
            i += 1
            continue

        if re.match(r"^[-*]\s+", stripped):
            out.append("<ul>")
            while i < len(lines) and re.match(r"^[-*]\s+", lines[i].strip()):
                item = re.sub(r"^[-*]\s+", "", lines[i].strip())
                out.append(f"<li>{inline_md(item)}</li>")
                i += 1
            out.append("</ul>")
            continue

        if re.match(r"^\d+\.\s+", stripped):
            out.append("<ol>")
            while i < len(lines) and re.match(r"^\d+\.\s+", lines[i].strip()):
                item = re.sub(r"^\d+\.\s+", "", lines[i].strip())
                out.append(f"<li>{inline_md(item)}</li>")
                i += 1
            out.append("</ol>")
            continue

        cap = re.fullmatch(r"\*(图 [^*]+)\*", stripped)
        if cap:
            out.append(f'<p class="caption"><em>{html.escape(cap.group(1))}</em></p>')
            i += 1
            continue

        if not stripped:
            i += 1
            continue

        out.append(f"<p>{inline_md(stripped)}</p>")
        i += 1

    return "\n".join(out)


def main() -> int:
    if not MD.is_file():
        print(f"找不到 {MD}", file=sys.stderr)
        return 2
    text = MD.read_text(encoding="utf-8-sig")
    page = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MistTerm 操作手册</title>
<style>{STYLE}</style>
</head>
<body>
{convert_md(text)}
</body>
</html>
"""
    OUT.write_text(page, encoding="utf-8")
    print(f"已生成 {OUT} ({OUT.stat().st_size // 1024} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
