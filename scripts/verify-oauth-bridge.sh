#!/usr/bin/env bash
# 运维验收：桌面 OAuth 桥接页是否可访问（CLIENT-TEAM-TODO §1.2）
set -euo pipefail
URL="${1:-https://mistlab.dev/oauth/desktop-callback.html}"
code=$(curl -sS -o /dev/null -w '%{http_code}' "$URL")
if [[ "$code" == "200" ]]; then
  echo "OK: $URL → HTTP $code"
  exit 0
fi
echo "FAIL: $URL → HTTP $code (expected 200)" >&2
exit 1
