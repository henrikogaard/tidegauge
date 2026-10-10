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
    local preserve_entitlements="${2:-no}"
    local sign_args=(--force)
    if [ "$IDENTITY" != "-" ]; then
        sign_args+=(--options runtime --timestamp)
    fi
    if [ "$preserve_entitlements" = "yes" ]; then
        sign_args+=(--preserve-metadata=entitlements)
    fi
    codesign "${sign_args[@]}" --sign "$IDENTITY" "$path"
}

FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
VERSIONED="$FRAMEWORK/Versions/B"
INSTALLER_XPC="$VERSIONED/XPCServices/Installer.xpc"
DOWNLOADER_XPC="$VERSIONED/XPCServices/Downloader.xpc"
AUTOUPDATE="$VERSIONED/Autoupdate"
UPDATER_APP="$VERSIONED/Updater.app"

for required in "$FRAMEWORK" "$INSTALLER_XPC" "$DOWNLOADER_XPC" "$AUTOUPDATE" "$UPDATER_APP"; do
    if [ ! -e "$required" ]; then
        printf 'Sparkle signing component not found: %s\n' "$required" >&2
        exit 1
    fi
done

sign "$INSTALLER_XPC"
# Sparkle 2.6+ requires the Downloader XPC entitlements to survive re-signing.
sign "$DOWNLOADER_XPC" yes
sign "$AUTOUPDATE"
sign "$UPDATER_APP"
sign "$FRAMEWORK"
sign "$APP"
