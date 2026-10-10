#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-preview}"
VERSION="${TIDEGAUGE_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}"
BUILD_VERSION="${TIDEGAUGE_BUILD_VERSION:-$VERSION}"
BINARY="${TIDEGAUGE_BINARY:-$ROOT/target/release/tidegauge}"
OUTPUT_ROOT="${TIDEGAUGE_OUTPUT_ROOT:-$ROOT/build}"
APP="$OUTPUT_ROOT/$MODE/Tidegauge.app"
PRODUCTION_FEED="https://github.com/henrikogaard/tidegauge/releases/latest/download/appcast.xml"

case "$MODE" in
    preview|qa|production) ;;
    *)
        printf 'Usage: %s [preview|qa|production]\n' "$0" >&2
        exit 2
        ;;
esac

if [ ! -x "$BINARY" ]; then
    printf 'Release binary not found or not executable: %s\n' "$BINARY" >&2
    exit 1
fi

if [[ ! "$BUILD_VERSION" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
    printf 'CFBundleVersion must contain one to three numeric components: %s\n' "$BUILD_VERSION" >&2
    exit 1
fi
if [[ ! "$VERSION" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
    printf 'CFBundleShortVersionString must be numeric: %s\n' "$VERSION" >&2
    exit 1
fi

require_value() {
    local name="$1"
    if [ -z "${!name:-}" ]; then
        printf 'Required release setting is missing: %s\n' "$name" >&2
        exit 1
    fi
}

validate_public_key() {
    local name="$1"
    require_value "$name"
    if ! printf '%s' "${!name}" | python3 -c \
        'import base64, sys; value = base64.b64decode(sys.stdin.read(), validate=True); raise SystemExit(0 if len(value) == 32 else 1)'; then
        printf '%s must be a base64-encoded 32-byte Ed25519 key\n' "$name" >&2
        exit 1
    fi
}

validate_https_url() {
    local name="$1"
    require_value "$name"
    case "${!name}" in
        https://*) ;;
        *)
            printf '%s must use https://\n' "$name" >&2
            exit 1
            ;;
    esac
}

case "$MODE" in
    production)
        validate_public_key TIDEGAUGE_SPARKLE_PUBLIC_KEY
        if [ -z "${DEVELOPER_ID_APPLICATION:-}" ]; then
            printf '%s\n' "Production packaging requires DEVELOPER_ID_APPLICATION." >&2
            exit 1
        fi
        if ! security find-identity -v -p codesigning | grep -Fq "\"$DEVELOPER_ID_APPLICATION\""; then
            printf 'Signing identity is not available: %s\n' "$DEVELOPER_ID_APPLICATION" >&2
            exit 1
        fi
        FEED_URL="$PRODUCTION_FEED"
        PUBLIC_KEY="$TIDEGAUGE_SPARKLE_PUBLIC_KEY"
        SIGNING_IDENTITY="$DEVELOPER_ID_APPLICATION"
        ;;
    qa)
        validate_public_key TIDEGAUGE_QA_PUBLIC_KEY
        validate_https_url TIDEGAUGE_QA_FEED_URL
        if [ "$TIDEGAUGE_QA_FEED_URL" = "$PRODUCTION_FEED" ]; then
            printf '%s\n' "QA feed must be separate from the production feed." >&2
            exit 1
        fi
        FEED_URL="$TIDEGAUGE_QA_FEED_URL"
        PUBLIC_KEY="$TIDEGAUGE_QA_PUBLIC_KEY"
        SIGNING_IDENTITY="-"
        ;;
    preview)
        FEED_URL=""
        PUBLIC_KEY=""
        SIGNING_IDENTITY="-"
        ;;
esac

SPARKLE_FRAMEWORK="$("$ROOT/scripts/fetch-sparkle.sh")"
SPARKLE_CACHE="${TIDEGAUGE_SPARKLE_CACHE:-$ROOT/.cache/sparkle}"
LICENSE_FILE=""
for candidate in "$SPARKLE_CACHE/LICENSE" "$SPARKLE_CACHE/License.txt" \
    "$SPARKLE_FRAMEWORK/LICENSE" "$SPARKLE_FRAMEWORK/Resources/LICENSE"; do
    if [ -f "$candidate" ]; then
        LICENSE_FILE="$candidate"
        break
    fi
done
if [ -z "$LICENSE_FILE" ]; then
    printf '%s\n' "Sparkle license file was not found in the pinned distribution." >&2
    exit 1
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
install -m 755 "$BINARY" "$APP/Contents/MacOS/tidegauge"
ditto "$SPARKLE_FRAMEWORK" "$APP/Contents/Frameworks/Sparkle.framework"
cp "$ROOT/assets/tidegauge.icns" "$APP/Contents/Resources/tidegauge.icns"
cp "$LICENSE_FILE" "$APP/Contents/Resources/Sparkle-LICENSE.txt"

PLIST="$APP/Contents/Info.plist"
plutil -create xml1 "$PLIST"
plutil -insert CFBundleDevelopmentRegion -string en "$PLIST"
plutil -insert CFBundleDisplayName -string Tidegauge "$PLIST"
plutil -insert CFBundleExecutable -string tidegauge "$PLIST"
plutil -insert CFBundleIdentifier -string no.ogard.tidegauge "$PLIST"
plutil -insert CFBundleIconFile -string tidegauge.icns "$PLIST"
plutil -insert CFBundleInfoDictionaryVersion -string 6.0 "$PLIST"
plutil -insert CFBundleName -string Tidegauge "$PLIST"
plutil -insert CFBundlePackageType -string APPL "$PLIST"
plutil -insert CFBundleShortVersionString -string "$VERSION" "$PLIST"
plutil -insert CFBundleVersion -string "$BUILD_VERSION" "$PLIST"
plutil -insert LSMinimumSystemVersion -string 11.0 "$PLIST"
plutil -insert LSUIElement -bool YES "$PLIST"
plutil -insert TidegaugeUpdaterMode -string "$MODE" "$PLIST"

if [ "$MODE" = "production" ] || [ "$MODE" = "qa" ]; then
    plutil -insert SUEnableAutomaticChecks -bool YES "$PLIST"
    plutil -insert SUFeedURL -string "$FEED_URL" "$PLIST"
    plutil -insert SUPublicEDKey -string "$PUBLIC_KEY" "$PLIST"
else
    plutil -insert SUEnableAutomaticChecks -bool NO "$PLIST"
fi

"$ROOT/scripts/sign-bundle.sh" "$APP" "$SIGNING_IDENTITY"
codesign --verify --deep --strict "$APP"
printf '%s\n' "$APP"
