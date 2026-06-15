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

# Generate .icns app icon (macOS CI has iconutil + sips)
ICON_SRC="$ROOT/assets/app-icon-1024.png"
ICONSET="$ROOT/target/release/AppIcon.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
if [[ -f "$ICON_SRC" ]] && command -v iconutil &>/dev/null; then
  sips -z 16 16   "$ICON_SRC" --out "$ICONSET/icon_16x16.png"      >/dev/null 2>&1
  sips -z 32 32   "$ICON_SRC" --out "$ICONSET/icon_16x16@2x.png"   >/dev/null 2>&1
  sips -z 32 32   "$ICON_SRC" --out "$ICONSET/icon_32x32.png"      >/dev/null 2>&1
  sips -z 64 64   "$ICON_SRC" --out "$ICONSET/icon_32x32@2x.png"   >/dev/null 2>&1
  sips -z 128 128 "$ICON_SRC" --out "$ICONSET/icon_128x128.png"    >/dev/null 2>&1
  sips -z 256 256 "$ICON_SRC" --out "$ICONSET/icon_128x128@2x.png" >/dev/null 2>&1
  sips -z 256 256 "$ICON_SRC" --out "$ICONSET/icon_256x256.png"    >/dev/null 2>&1
  sips -z 512 512 "$ICON_SRC" --out "$ICONSET/icon_256x256@2x.png" >/dev/null 2>&1
  sips -z 512 512 "$ICON_SRC" --out "$ICONSET/icon_512x512.png"    >/dev/null 2>&1
  sips -z 1024 1024 "$ICON_SRC" --out "$ICONSET/icon_512x512@2x.png" >/dev/null 2>&1
  iconutil -c icns "$ICONSET" -o "$OUT/Contents/Resources/AppIcon.icns" >/dev/null 2>&1
  echo "==> AppIcon.icns generated"
else
  echo "⚠️  iconutil/sips not available, skipping icon"
fi

echo "Bundle ready: $OUT"
echo "Launch GUI: open \"$OUT\""
