#!/usr/bin/env bash
# 构建 Mist.app 并用 open 启动（macOS 直接出界面，不占用 Terminal 窗口）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
bash "$ROOT/scripts/bundle-macos.sh"
open "$ROOT/target/release/Mist.app"
