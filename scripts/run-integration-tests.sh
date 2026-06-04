#!/usr/bin/env bash
# 运行需本地 sshd 的集成测试（无 sshd 时相关用例自动 skip）
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export RUST_TEST_THREADS=1
for t in sftp_integration_test monitor_integration_test zmodem_integration_test; do
  echo "==> cargo test --test $t -- --nocapture"
  cargo test --test "$t" --target-dir target/ci-test -- --nocapture
done
echo "Integration tests finished."
