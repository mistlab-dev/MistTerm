#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== MistTerm: cargo test =="
cargo test -q
echo "== MistTerm: cargo build --release =="
cargo build --release -q
echo "OK: 构建与测试通过。完整界面步骤见 docs/SMOKE.md"
