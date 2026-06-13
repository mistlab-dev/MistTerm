#!/usr/bin/env bash
# Quick smoke: CI checks + release binary build.
set -euo pipefail
cd "$(dirname "$0")/.."
"$(dirname "$0")/ci-smoke.sh"
echo ""
echo "== release build (Mist) =="
cargo build --release -q --bin Mist
echo "OK: 构建与测试通过。详见 docs/tech/TESTING.md"
