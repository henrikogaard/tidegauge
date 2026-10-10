#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${TIDEGAUGE_VERSION:-$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT/Cargo.toml")}"
OUTPUT_ROOT="${TIDEGAUGE_OUTPUT_ROOT:-$ROOT/build/release}"
APP="$OUTPUT_ROOT/production/Tidegauge.app"
ZIP="$OUTPUT_ROOT/Tidegauge-${VERSION}.zip"
APPCAST="$OUTPUT_ROOT/appcast.xml"
APPCAST_CHECKSUM="$APPCAST.sha256"
PRODUCTION_FEED="https://github.com/henrikogaard/tidegauge/releases/latest/download/appcast.xml"
DOWNLOAD_URL_PREFIX="https://github.com/henrikogaard/tidegauge/releases/download/v${VERSION}/"

if [ -z "${TIDEGAUGE_SPARKLE_PUBLIC_KEY:-}" ]; then
    PUBLIC_KEY_FILE="$ROOT/packaging/sparkle-public-key.txt"
    if [ ! -f "$PUBLIC_KEY_FILE" ]; then
        printf 'Production public key file not found: %s\n' "$PUBLIC_KEY_FILE" >&2
        exit 1
    fi
    TIDEGAUGE_SPARKLE_PUBLIC_KEY="$(tr -d '[:space:]' < "$PUBLIC_KEY_FILE")"
fi
export TIDEGAUGE_SPARKLE_PUBLIC_KEY
: "${TIDEGAUGE_SPARKLE_PUBLIC_KEY:?Production requires a public key}"
: "${DEVELOPER_ID_APPLICATION:?Production requires DEVELOPER_ID_APPLICATION}"
: "${NOTARY_PROFILE:?Production requires NOTARY_PROFILE}"

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

NOTARY_TEMP="$(mktemp -d "$OUTPUT_ROOT/.notary.XXXXXX")"
trap 'rm -rf "$NOTARY_TEMP"' EXIT
ditto -c -k --sequesterRsrc --keepParent "$APP" "$NOTARY_TEMP/Tidegauge.zip"
if [ -n "${NOTARY_KEYCHAIN:-}" ]; then
    xcrun notarytool submit "$NOTARY_TEMP/Tidegauge.zip" \
        --keychain-profile "$NOTARY_PROFILE" \
        --keychain "$NOTARY_KEYCHAIN" \
        --wait
else
    xcrun notarytool submit "$NOTARY_TEMP/Tidegauge.zip" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait
fi
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"
rm -rf "$NOTARY_TEMP"
trap - EXIT

RELEASE_TEMP_DIR="$(mktemp -d "$OUTPUT_ROOT/.release.XXXXXX")"
trap 'rm -rf "$RELEASE_TEMP_DIR"' EXIT
APPCAST_CANDIDATE="$RELEASE_TEMP_DIR/appcast.xml"
APPCAST_CHECKSUM_CANDIDATE="$RELEASE_TEMP_DIR/appcast.xml.sha256"
APPCAST_INPUT="$RELEASE_TEMP_DIR/updates"
mkdir -p "$APPCAST_INPUT"

ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
ZIP_BASENAME="$(basename "$ZIP")"
(cd "$OUTPUT_ROOT" && shasum -a 256 "$ZIP_BASENAME" > "$ZIP_BASENAME.sha256")

cp "$ZIP" "$APPCAST_INPUT/"
if [ -n "${TIDEGAUGE_SPARKLE_PRIVATE_KEY:-}" ]; then
    printf '%s' "$TIDEGAUGE_SPARKLE_PRIVATE_KEY" | "$GENERATE_APPCAST" \
        --ed-key-file - \
        --download-url-prefix "$DOWNLOAD_URL_PREFIX" \
        -o "$APPCAST_CANDIDATE" "$APPCAST_INPUT"
else
    "$GENERATE_APPCAST" \
        --account "${TIDEGAUGE_SPARKLE_SIGNING_ACCOUNT:-no.ogard.tidegauge.sparkle}" \
        --download-url-prefix "$DOWNLOAD_URL_PREFIX" \
        -o "$APPCAST_CANDIDATE" "$APPCAST_INPUT"
fi
python3 "$ROOT/packaging/validate_appcast.py" "$APPCAST_CANDIDATE" "$ZIP"
swift "$ROOT/packaging/verify_appcast_signatures.swift" \
    "$APPCAST_CANDIDATE" "$ZIP" "$APP/Contents/Info.plist"
(cd "$RELEASE_TEMP_DIR" && shasum -a 256 appcast.xml > appcast.xml.sha256)
mv "$APPCAST_CANDIDATE" "$APPCAST"
mv "$APPCAST_CHECKSUM_CANDIDATE" "$APPCAST_CHECKSUM"

printf '%s\n' "Notarized and stapled: $APP"

printf '%s\n' "Release bundle: $APP"
printf '%s\n' "Update archive: $ZIP"
printf '%s\n' "Appcast: $APPCAST"
printf '%s\n' "Feed: $PRODUCTION_FEED"
