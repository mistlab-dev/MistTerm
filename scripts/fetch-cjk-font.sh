#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/assets/fonts/NotoSansSC-Regular.otf"
URL="https://cdn.jsdelivr.net/gh/googlefonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf"
mkdir -p "$(dirname "$OUT")"
curl -fsSL -o "$OUT" "$URL"
ls -lh "$OUT"
echo "CJK font ready: $OUT"
