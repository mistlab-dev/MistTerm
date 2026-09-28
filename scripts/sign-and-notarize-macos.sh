#!/usr/bin/env bash
# Import Developer ID identity (CI), codesign Mist.app, package DMG,
# notarize the DMG once, and staple.
#
# Required env (GitHub Actions secrets):
#   APPLE_CERTIFICATE_BASE64
#   APPLE_CERTIFICATE_PASSWORD
#   APPLE_CODESIGN_IDENTITY
#   APPSTORE_ISSUER_ID
#   APPSTORE_KEY_ID
#   APPSTORE_PRIVATE_KEY
# Optional:
#   APPLE_KEYCHAIN_PASSWORD  (defaults to a random value for this job)
#   NOTARY_TIMEOUT          (notarytool --wait timeout; default 20m)
#
# Usage:
#   bash scripts/sign-and-notarize-macos.sh path/to/Mist.app path/to/out.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_PATH="${1:-}"
DMG_PATH="${2:-}"
ENTITLEMENTS="$ROOT/resources/macos/Mist.entitlements"
NOTARY_TIMEOUT="${NOTARY_TIMEOUT:-20m}"

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

log() { echo "==> [$(date -u +%H:%M:%S)] $*"; }

KEYCHAIN_PASSWORD="${APPLE_KEYCHAIN_PASSWORD:-$(openssl rand -base64 32)}"
KEYCHAIN_PATH="$WORK/mistterm-signing.keychain-db"
CERT_PATH="$WORK/certificate.p12"
API_KEY_PATH="$WORK/AuthKey_${APPSTORE_KEY_ID}.p8"

log "importing Developer ID certificate into temporary keychain"
echo -n "$APPLE_CERTIFICATE_BASE64" | base64 --decode >"$CERT_PATH"
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

log "codesign $APP_PATH"
# Innermost Mach-O first, then the bundle (avoid --deep).
while IFS= read -r -d '' bin; do
  file "$bin" | grep -q 'Mach-O' || continue
  codesign --force --options runtime --timestamp \
    --entitlements "$ENTITLEMENTS" \
    --sign "$APPLE_CODESIGN_IDENTITY" \
    "$bin"
done < <(find "$APP_PATH/Contents" -type f -print0 2>/dev/null)
codesign --force --options runtime --timestamp \
  --entitlements "$ENTITLEMENTS" \
  --sign "$APPLE_CODESIGN_IDENTITY" \
  "$APP_PATH"

codesign --verify --deep --strict --verbose=2 "$APP_PATH"
log "codesign identity:"
codesign -dv --verbose=4 "$APP_PATH" 2>&1 | grep -E 'Authority|Identifier|TeamIdentifier|Signature' || true

log "writing App Store Connect API key"
# Normalize CRLF from GitHub secret paste on Windows machines.
printf '%s\n' "$APPSTORE_PRIVATE_KEY" | tr -d '\r' >"$API_KEY_PATH"
chmod 600 "$API_KEY_PATH"

log "packaging DMG $DMG_PATH"
bash "$ROOT/scripts/package-macos-dmg.sh" "$APP_PATH" "$DMG_PATH"
codesign --force --timestamp --sign "$APPLE_CODESIGN_IDENTITY" "$DMG_PATH"

log "notarytool submit DMG (wait, timeout=${NOTARY_TIMEOUT})"
# Single notarization of the DMG (covers nested Mist.app). Avoids double-wait.
xcrun notarytool submit "$DMG_PATH" \
  --key "$API_KEY_PATH" \
  --key-id "$APPSTORE_KEY_ID" \
  --issuer "$APPSTORE_ISSUER_ID" \
  --wait \
  --timeout "$NOTARY_TIMEOUT"

log "stapler staple $DMG_PATH"
xcrun stapler staple "$DMG_PATH"
xcrun stapler validate "$DMG_PATH"

log "Signed + notarized DMG: $DMG_PATH"
log "Signed Mist.app (ticket via DMG notarization / online check): $APP_PATH"
