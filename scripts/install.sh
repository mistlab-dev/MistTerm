#!/usr/bin/env bash
# Install Mist (release) on macOS or Linux.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN_NAME="Mist"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "==> Building $BIN_NAME (release)..."
if [[ -f "$ROOT/scripts/fetch-cjk-font.sh" ]] && [[ ! -f "$ROOT/assets/fonts/NotoSansSC-Regular.otf" ]]; then
  echo "==> Optional: fetching embedded CJK font for Chinese UI..."
  bash "$ROOT/scripts/fetch-cjk-font.sh" || true
fi

cargo build --release --bin "$BIN_NAME"

SRC="$ROOT/target/release/$BIN_NAME"
if [[ ! -x "$SRC" ]]; then
  echo "Build failed: $SRC not found" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
cp "$SRC" "$INSTALL_DIR/$BIN_NAME"
chmod +x "$INSTALL_DIR/$BIN_NAME"

if [[ "$(uname -s)" == "Darwin" ]]; then
  echo "==> Bundling Mist.app (GUI launch without Terminal)..."
  bash "$ROOT/scripts/bundle-macos.sh"
  APP_INSTALL="${APP_INSTALL_DIR:-$HOME/Applications}"
  mkdir -p "$APP_INSTALL"
  rm -rf "$APP_INSTALL/Mist.app"
  cp -R "$ROOT/target/release/Mist.app" "$APP_INSTALL/Mist.app"
  echo ""
  echo "GUI app: $APP_INSTALL/Mist.app"
  echo "Launch:  open -a Mist   (Spotlight / Dock — no Terminal window)"
fi

echo ""
echo "Installed: $INSTALL_DIR/$BIN_NAME"
echo "Ensure $INSTALL_DIR is on your PATH, then run: $BIN_NAME"
echo ""
echo "System dependencies (if build failed earlier):"
echo "  macOS:  xcode-select --install && brew install libssh2 pkg-config"
echo "  Debian: sudo apt install build-essential libssh2-1-dev pkg-config libssl-dev"
echo "See docs/en/INSTALL.md (or docs/zh/INSTALL.md) for more detail."
