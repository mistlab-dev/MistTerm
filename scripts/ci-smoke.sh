#!/usr/bin/env bash
# Low-memory local check mirroring GitHub Actions "test" job (tag builds).
#
# Usage:
#   ./scripts/ci-smoke.sh              # compile + lib tests + zmodem test
#   ./scripts/ci-smoke.sh --compile-only
#   ./scripts/ci-smoke.sh --windows-ci   # vendored-openssl (Windows CI parity)

set -euo pipefail
cd "$(dirname "$0")/.."

export CARGO_BUILD_JOBS=1
export CARGO_INCREMENTAL=0

COMPILE_ONLY=0
FEATURES=()
for arg in "$@"; do
  case "$arg" in
    --compile-only) COMPILE_ONLY=1 ;;
    --windows-ci) FEATURES=(--features vendored-openssl) ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

step() {
  echo ""
  echo "== $1 =="
  shift
  cargo "$@"
}

step "1/3 compile lib + integration tests" test --lib --tests --no-run "${FEATURES[@]}"
step "2/3 compile examples" test --examples --no-run "${FEATURES[@]}"
step "3/3 build bins" build --bins "${FEATURES[@]}"

if [ "$COMPILE_ONLY" -eq 1 ]; then
  echo ""
  echo "OK: compile-only CI smoke passed"
  exit 0
fi

step "4/4 run lib unit tests" test --lib -j 1 "${FEATURES[@]}"
step "5/5 zmodem integration test" test --test zmodem_integration_test -j 1 "${FEATURES[@]}"

echo ""
echo "OK: CI smoke passed (see docs/tech/QA.md for manual UI checks)"
