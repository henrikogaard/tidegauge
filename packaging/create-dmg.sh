#!/bin/bash
set -euo pipefail

if [ "$#" -ne 2 ]; then
    printf 'Usage: %s APP DESTINATION\n' "$0" >&2
    exit 2
fi

APP="$1"
DESTINATION="$2"
if [ ! -d "$APP" ] || [ ! -f "$APP/Contents/Info.plist" ]; then
    printf 'Application bundle is missing or invalid: %s\n' "$APP" >&2
    exit 1
fi
if [ -e "$DESTINATION" ] || [ -L "$DESTINATION" ]; then
    printf 'Refusing to overwrite existing disk image: %s\n' "$DESTINATION" >&2
    exit 1
fi
DESTINATION_PARENT="$(dirname "$DESTINATION")"
if [ ! -d "$DESTINATION_PARENT" ]; then
    printf 'Destination directory does not exist: %s\n' "$DESTINATION_PARENT" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$(cd "$APP" && pwd -P)"
STAGING="$(mktemp -d "${TMPDIR:-/tmp}/tidegauge-stage.XXXXXX")"
CANDIDATE_DIR=""
cleanup() {
    rm -rf "$STAGING"
    if [ -n "$CANDIDATE_DIR" ]; then
        rm -rf "$CANDIDATE_DIR"
    fi
}
trap cleanup EXIT
CANDIDATE_DIR="$(mktemp -d "$DESTINATION_PARENT/.tidegauge-dmg.XXXXXX")"
CANDIDATE="$CANDIDATE_DIR/Tidegauge.dmg"

ditto "$APP" "$STAGING/Tidegauge.app"
ln -s /Applications "$STAGING/Applications"
cp "$ROOT/packaging/dmg-layout.DS_Store" "$STAGING/.DS_Store"
hdiutil create -quiet -format UDZO -volname Tidegauge -srcfolder "$STAGING" "$CANDIDATE"

if [ -n "${DEVELOPER_ID_APPLICATION:-}" ]; then
    codesign --force --sign "$DEVELOPER_ID_APPLICATION" --timestamp "$CANDIDATE"
    codesign --verify --verbose=2 "$CANDIDATE"
fi

mv -n "$CANDIDATE" "$DESTINATION"
if [ -e "$CANDIDATE" ]; then
    printf 'Refusing to overwrite existing disk image: %s\n' "$DESTINATION" >&2
    exit 1
fi
printf '%s\n' "$DESTINATION"
