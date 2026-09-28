#!/usr/bin/env bash
# Import Developer ID identity (CI), codesign Mist.app, package DMG,
# notarize the DMG once (polled with heartbeats), and staple.
#
# Required env (GitHub Actions secrets):
#   APPLE_CERTIFICATE_BASE64
#   APPLE_CERTIFICATE_PASSWORD
#   APPLE_CODESIGN_IDENTITY
#   APPSTORE_ISSUER_ID
#   APPSTORE_KEY_ID
#   APPSTORE_PRIVATE_KEY
# Optional:
#   APPLE_KEYCHAIN_PASSWORD
#   NOTARY_TIMEOUT_SECS   (default 1200 = 20m)
#   NOTARY_POLL_SECS      (default 20)
#
# Usage:
#   bash scripts/sign-and-notarize-macos.sh path/to/Mist.app path/to/out.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_PATH="${1:-}"
DMG_PATH="${2:-}"
ENTITLEMENTS="$ROOT/resources/macos/Mist.entitlements"
NOTARY_TIMEOUT_SECS="${NOTARY_TIMEOUT_SECS:-1200}"
NOTARY_POLL_SECS="${NOTARY_POLL_SECS:-20}"

[[ -n "$APP_PATH" ]] || { echo "usage: $0 Mist.app out.dmg" >&2; exit 1; }
[[ -d "$APP_PATH" ]] || { echo "missing app bundle: $APP_PATH" >&2; exit 1; }
[[ -n "$DMG_PATH" ]] || { echo "usage: $0 Mist.app out.dmg (DMG path required)" >&2; exit 1; }
[[ -f "$ENTITLEMENTS" ]] || { echo "missing entitlements: $ENTITLEMENTS" >&2; exit 1; }

: "${APPLE_CERTIFICATE_BASE64:?}"
: "${APPLE_CERTIFICATE_PASSWORD:?}"
: "${APPLE_CODESIGN_IDENTITY:?}"
: "${APPSTORE_ISSUER_ID:?}"
: "${APPSTORE_KEY_ID:?}"
: "${APPSTORE_PRIVATE_KEY:?}"

WORK="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/mistterm-sign-$$"
mkdir -p "$WORK"
cleanup() {
  rm -rf "$WORK" || true
  if [[ -n "${KEYCHAIN_PATH:-}" ]] && [[ -f "$KEYCHAIN_PATH" ]]; then
    security delete-keychain "$KEYCHAIN_PATH" 2>/dev/null || true
  fi
}
trap cleanup EXIT

SCRIPT_START=$SECONDS
log() { echo "==> [$(date -u +%H:%M:%S) +$((SECONDS - SCRIPT_START))s] $*"; }
group_start() { echo "::group::$*"; log "BEGIN $*"; }
group_end() { log "END $*"; echo "::endgroup::"; }

json_field() {
  # $1=json $2=field — prefer python3, fall back to plutil-ish grep
  local json="$1" field="$2"
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json,sys; print(json.load(sys.stdin).get(sys.argv[1],"") or "")' "$field" <<<"$json"
  elif command -v jq >/dev/null 2>&1; then
    jq -r --arg f "$field" '.[$f] // empty' <<<"$json"
  else
    echo "$json" | sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p" | head -1
  fi
}

group_start "0. Preflight (secrets present, no values)"
log "identity=${APPLE_CODESIGN_IDENTITY}"
log "team_id=${APPLE_TEAM_ID:-<unset>}"
log "appstore_key_id=${APPSTORE_KEY_ID}"
log "appstore_issuer_id_len=${#APPSTORE_ISSUER_ID}"
log "certificate_b64_len=${#APPLE_CERTIFICATE_BASE64}"
log "private_key_pem_len=${#APPSTORE_PRIVATE_KEY}"
log "app_path=$APP_PATH"
log "dmg_path=$DMG_PATH"
log "notary_timeout_secs=$NOTARY_TIMEOUT_SECS poll_secs=$NOTARY_POLL_SECS"
du -sh "$APP_PATH" || true
group_end "0. Preflight"

KEYCHAIN_PASSWORD="${APPLE_KEYCHAIN_PASSWORD:-$(openssl rand -base64 32)}"
KEYCHAIN_PATH="$WORK/mistterm-signing.keychain-db"
CERT_PATH="$WORK/certificate.p12"
API_KEY_PATH="$WORK/AuthKey_${APPSTORE_KEY_ID}.p8"

group_start "1. Import Developer ID into temporary keychain"
echo -n "$APPLE_CERTIFICATE_BASE64" | base64 --decode >"$CERT_PATH"
log "decoded p12 bytes=$(wc -c <"$CERT_PATH" | tr -d ' ')"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security import "$CERT_PATH" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -A \
  -t cert \
  -f pkcs12 \
  -k "$KEYCHAIN_PATH"
security list-keychains -d user -s "$KEYCHAIN_PATH" $(security list-keychains -d user | sed -e s/\"//g)
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "$KEYCHAIN_PASSWORD" \
  "$KEYCHAIN_PATH"
log "identities in keychain:"
security find-identity -v -p codesigning "$KEYCHAIN_PATH" || true
group_end "1. Import Developer ID"

group_start "2. Codesign Mist.app"
signed=0
while IFS= read -r -d '' bin; do
  file "$bin" | grep -q 'Mach-O' || continue
  codesign --force --options runtime --timestamp \
    --entitlements "$ENTITLEMENTS" \
    --sign "$APPLE_CODESIGN_IDENTITY" \
    "$bin"
  signed=$((signed + 1))
  if (( signed % 5 == 0 )); then
    log "signed $signed Mach-O so far (latest: $bin)"
  fi
done < <(find "$APP_PATH/Contents" -type f -print0 2>/dev/null)
log "signed $signed nested Mach-O file(s); signing bundle…"
codesign --force --options runtime --timestamp \
  --entitlements "$ENTITLEMENTS" \
  --sign "$APPLE_CODESIGN_IDENTITY" \
  "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
codesign -dv --verbose=4 "$APP_PATH" 2>&1 | grep -E 'Authority|Identifier|TeamIdentifier|Signature|Runtime' || true
group_end "2. Codesign Mist.app"

group_start "3. Write App Store Connect API key"
printf '%s\n' "$APPSTORE_PRIVATE_KEY" | tr -d '\r' >"$API_KEY_PATH"
chmod 600 "$API_KEY_PATH"
log "api key file bytes=$(wc -c <"$API_KEY_PATH" | tr -d ' ')"
# Show PEM header only (safe).
head -n 1 "$API_KEY_PATH" || true
group_end "3. Write API key"

group_start "4. Package + codesign DMG"
bash "$ROOT/scripts/package-macos-dmg.sh" "$APP_PATH" "$DMG_PATH"
codesign --force --timestamp --sign "$APPLE_CODESIGN_IDENTITY" "$DMG_PATH"
ls -lh "$DMG_PATH"
group_end "4. Package + codesign DMG"

group_start "5. Notarytool submit + poll"
log "submitting DMG (async; will poll every ${NOTARY_POLL_SECS}s)"
submit_json="$(
  xcrun notarytool submit "$DMG_PATH" \
    --key "$API_KEY_PATH" \
    --key-id "$APPSTORE_KEY_ID" \
    --issuer "$APPSTORE_ISSUER_ID" \
    --output-format json
)"
log "submit response: $submit_json"
submission_id="$(json_field "$submit_json" id)"
[[ -n "$submission_id" ]] || {
  log "ERROR: could not parse submission id from notarytool output"
  exit 1
}
log "submission_id=$submission_id"

deadline=$((SECONDS + NOTARY_TIMEOUT_SECS))
status=""
while (( SECONDS < deadline )); do
  info_json="$(
    xcrun notarytool info "$submission_id" \
      --key "$API_KEY_PATH" \
      --key-id "$APPSTORE_KEY_ID" \
      --issuer "$APPSTORE_ISSUER_ID" \
      --output-format json
  )"
  status="$(json_field "$info_json" status)"
  log "notary status=${status:-unknown} (elapsed=$((SECONDS - SCRIPT_START))s, left=$((deadline - SECONDS))s)"
  case "$status" in
    Accepted)
      break
      ;;
    Invalid|Rejected)
      log "ERROR: notarization $status — fetching log"
      xcrun notarytool log "$submission_id" \
        --key "$API_KEY_PATH" \
        --key-id "$APPSTORE_KEY_ID" \
        --issuer "$APPSTORE_ISSUER_ID" || true
      exit 1
      ;;
    In\ Progress|"In Progress"|Accepted) ;;
    *)
      # Unknown / empty — keep polling
      ;;
  esac
  if [[ "$status" == "Accepted" ]]; then
    break
  fi
  sleep "$NOTARY_POLL_SECS"
done

if [[ "$status" != "Accepted" ]]; then
  log "ERROR: notarization timed out after ${NOTARY_TIMEOUT_SECS}s (last status=${status:-none})"
  xcrun notarytool info "$submission_id" \
    --key "$API_KEY_PATH" \
    --key-id "$APPSTORE_KEY_ID" \
    --issuer "$APPSTORE_ISSUER_ID" || true
  xcrun notarytool log "$submission_id" \
    --key "$API_KEY_PATH" \
    --key-id "$APPSTORE_KEY_ID" \
    --issuer "$APPSTORE_ISSUER_ID" || true
  exit 1
fi
group_end "5. Notarytool submit + poll"

group_start "6. Staple DMG"
xcrun stapler staple "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"
spctl --assess --type open --context context:primary-signature -v "$DMG_PATH" || true
group_end "6. Staple DMG"

log "DONE Signed + notarized DMG: $DMG_PATH (total ${SECONDS}s)"
log "Signed Mist.app (covered by DMG notarization): $APP_PATH"
