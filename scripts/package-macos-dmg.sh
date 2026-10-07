#!/usr/bin/env bash
# 将 Mist.app 打成可分发 .dmg（含拖入 Applications 的快捷入口）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_PATH="${1:-$ROOT/target/release/Mist.app}"
DMG_PATH="${2:-$ROOT/dist/Mist-macos-universal.dmg}"

[[ -d "$APP_PATH" ]] || { echo "missing app bundle: $APP_PATH" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp -R "$APP_PATH" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

mkdir -p "$(dirname "$DMG_PATH")"

# Give hdiutil an explicit image size. Its own estimate for -srcfolder is sometimes too
# small, and the copy into the temporary volume then fails with "No space left on device"
# (this broke the v1.2.0 tag build). hdiutil is also known to fail now and then on CI
# runners, so retry a couple of times.
STAGE_MB="$(du -sm "$STAGE" | cut -f1)"
SIZE_MB=$(( STAGE_MB * 2 + 64 ))
echo "DMG staging: ${STAGE_MB} MB, image size ${SIZE_MB} MB"
df -h "$(dirname "$DMG_PATH")" "${TMPDIR:-/tmp}" || true

for attempt in 1 2 3; do
  rm -f "$DMG_PATH"
  if hdiutil create \
    -volname "Mist" \
    -srcfolder "$STAGE" \
    -fs HFS+ \
    -size "${SIZE_MB}m" \
    -ov \
    -format UDZO \
    "$DMG_PATH"; then
    break
  fi
  if [[ "$attempt" -eq 3 ]]; then
    echo "hdiutil create failed 3 times" >&2
    df -h "$(dirname "$DMG_PATH")" "${TMPDIR:-/tmp}" || true
    exit 1
  fi
  echo "hdiutil create failed (attempt ${attempt}); retrying in 15 s"
  sleep 15
done

echo "DMG ready: $DMG_PATH"
