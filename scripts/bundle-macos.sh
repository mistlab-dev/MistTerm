#!/usr/bin/env bash
# 打包 Mist.app，可用 open 直接启动 GUI（无需先开 Terminal.app）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
BIN_NAME="Mist"
APP_NAME="Mist.app"
OUT="$ROOT/target/release/$APP_NAME"

if [[ -f "$ROOT/scripts/fetch-cjk-font.sh" ]] && [[ ! -f "$ROOT/assets/fonts/NotoSansSC-Regular.otf" ]]; then
  bash "$ROOT/scripts/fetch-cjk-font.sh" || true
fi

OUT="${MIST_APP_OUT:-$OUT}"

if [[ -n "${MIST_BINARY:-}" ]]; then
  SRC="$MIST_BINARY"
  [[ -x "$SRC" ]] || { echo "missing $SRC" >&2; exit 1; }
else
  echo "==> cargo build --release --bin $BIN_NAME"
  cargo build --release --bin "$BIN_NAME"
  SRC="$ROOT/target/release/$BIN_NAME"
  [[ -x "$SRC" ]] || { echo "missing $SRC" >&2; exit 1; }
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp "$SRC" "$OUT/Contents/MacOS/$BIN_NAME"
cp "$ROOT/Info.plist" "$OUT/Contents/Info.plist"
chmod +x "$OUT/Contents/MacOS/$BIN_NAME"

echo "Bundle ready: $OUT"
echo "Launch GUI: open \"$OUT\""
