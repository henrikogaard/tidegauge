#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${TIDEGAUGE_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}"
OUTPUT_ROOT="${TIDEGAUGE_OUTPUT_ROOT:-$ROOT/build/release}"
APP="$OUTPUT_ROOT/production/Tidegauge.app"
ZIP="$OUTPUT_ROOT/Tidegauge-${VERSION}.zip"
APPCAST="$OUTPUT_ROOT/appcast.xml"
PRODUCTION_FEED="https://github.com/henrikogaard/tidegauge/releases/latest/download/appcast.xml"

: "${TIDEGAUGE_SPARKLE_PUBLIC_KEY:?Production requires TIDEGAUGE_SPARKLE_PUBLIC_KEY}"
: "${DEVELOPER_ID_APPLICATION:?Production requires DEVELOPER_ID_APPLICATION}"

if [[ ! "$VERSION" =~ ^[0-9]+([.][0-9]+){0,2}$ ]]; then
    printf 'Release version must be numeric: %s\n' "$VERSION" >&2
    exit 1
fi

TIDEGAUGE_OUTPUT_ROOT="$OUTPUT_ROOT" \
    "$ROOT/packaging/package-app.sh" production

mkdir -p "$OUTPUT_ROOT"
SPARKLE_DISTRIBUTION="$("$ROOT/scripts/fetch-sparkle.sh")"
GENERATE_APPCAST="$SPARKLE_DISTRIBUTION/bin/generate_appcast"
if [ ! -x "$GENERATE_APPCAST" ]; then
    printf 'Sparkle appcast tool not found: %s\n' "$GENERATE_APPCAST" >&2
    exit 1
fi

if [ -n "${NOTARY_PROFILE:-}" ]; then
    NOTARY_TEMP="$(mktemp -d "$OUTPUT_ROOT/.notary.XXXXXX")"
    trap 'rm -rf "$NOTARY_TEMP"' EXIT
    ditto -c -k --sequesterRsrc --keepParent "$APP" "$NOTARY_TEMP/Tidegauge.zip"
    xcrun notarytool submit "$NOTARY_TEMP/Tidegauge.zip" \
        --keychain-profile "$NOTARY_PROFILE" --wait
    xcrun stapler staple "$APP"
    rm -rf "$NOTARY_TEMP"
    trap - EXIT
fi

ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
shasum -a 256 "$ZIP" > "$ZIP.sha256"

APPCAST_INPUT="$OUTPUT_ROOT/updates"
mkdir -p "$APPCAST_INPUT"
cp "$ZIP" "$APPCAST_INPUT/"
if [ -n "${TIDEGAUGE_SPARKLE_PRIVATE_KEY:-}" ]; then
    printf '%s' "$TIDEGAUGE_SPARKLE_PRIVATE_KEY" | "$GENERATE_APPCAST" \
        --ed-key-file - \
        --download-url-prefix "https://github.com/henrikogaard/tidegauge/releases/latest/download/" \
        -o "$APPCAST" "$APPCAST_INPUT"
else
    "$GENERATE_APPCAST" \
        --account "${TIDEGAUGE_SPARKLE_SIGNING_ACCOUNT:-ed25519}" \
        --download-url-prefix "https://github.com/henrikogaard/tidegauge/releases/latest/download/" \
        -o "$APPCAST" "$APPCAST_INPUT"
fi
shasum -a 256 "$APPCAST" > "$APPCAST.sha256"

if [ -n "${NOTARY_PROFILE:-}" ]; then
    printf '%s\n' "Notarized and stapled: $APP"
else
    printf '%s\n' "NOTARY_PROFILE is not set; production output is signed but not notarized." >&2
fi

printf '%s\n' "Release bundle: $APP"
printf '%s\n' "Update archive: $ZIP"
printf '%s\n' "Appcast: $APPCAST"
printf '%s\n' "Feed: $PRODUCTION_FEED"
