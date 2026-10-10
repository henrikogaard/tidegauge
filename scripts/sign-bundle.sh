#!/bin/bash
set -euo pipefail

APP="${1:?app bundle path is required}"
IDENTITY="${2:?code-signing identity is required}"

if [ ! -d "$APP" ]; then
    printf 'App bundle not found: %s\n' "$APP" >&2
    exit 1
fi

sign() {
    local path="$1"
    if [ "$IDENTITY" = "-" ]; then
        codesign --force --sign "$IDENTITY" "$path"
    else
        codesign --force --options runtime --timestamp --sign "$IDENTITY" "$path"
    fi
}

for framework in "$APP"/Contents/Frameworks/*.framework; do
    [ -d "$framework" ] || continue
    while IFS= read -r -d '' nested; do
        sign "$nested"
    done < <(find "$framework" -depth -type d \( -name '*.app' -o -name '*.xpc' \) -print0)
    sign "$framework"
done

sign "$APP"
