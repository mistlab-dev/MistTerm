#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OTF="$ROOT/assets/fonts/NotoSansSC-Regular.otf"
TTF="$ROOT/assets/fonts/NotoSansSC-Regular.ttf"
OTF_URL="https://cdn.jsdelivr.net/gh/googlefonts/noto-cjk@main/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf"
# Fontsource 子集 TTF（genpdf 需 TrueType；约 2MB）
TTF_URL="https://cdn.jsdelivr.net/fontsource/fonts/noto-sans-sc@5.0.0/chinese-simplified-400-normal.ttf"
mkdir -p "$(dirname "$OTF")"
if [[ ! -f "$OTF" ]]; then
  curl -fsSL -o "$OTF" "$OTF_URL"
fi
curl -fsSL -o "$TTF" "$TTF_URL"
ls -lh "$OTF" "$TTF"
echo "CJK fonts ready (OTF for UI, TTF for PDF)"
