#!/bin/bash
# Builds widgets/TidegaugeWidgets.appex unsigned and prints its path.
# Usage: build-widgets.sh <work-dir> <version> <build-version> <arch>
# Needs Xcode (xcodebuild); package-app.sh signs the result with the app.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="${1:?work directory is required}"
VERSION="${2:?version is required}"
BUILD_VERSION="${3:?build version is required}"
ARCH="${4:?architecture is required}"

mkdir -p "$WORK"
LOG="$WORK/xcodebuild.log"
if ! xcodebuild \
    -project "$ROOT/widgets/TidegaugeWidgets.xcodeproj" \
    -scheme TidegaugeWidgets \
    -configuration Release \
    -derivedDataPath "$WORK/DerivedData" \
    CODE_SIGNING_ALLOWED=NO \
    MARKETING_VERSION="$VERSION" \
    CURRENT_PROJECT_VERSION="$BUILD_VERSION" \
    ARCHS="$ARCH" \
    ONLY_ACTIVE_ARCH=NO \
    build >"$LOG" 2>&1; then
    tail -40 "$LOG" >&2
    printf 'Widget build failed; full log: %s\n' "$LOG" >&2
    exit 1
fi

APPEX="$WORK/DerivedData/Build/Products/Release/TidegaugeWidgets.appex"
if [ ! -d "$APPEX" ]; then
    printf 'Widget extension was not produced: %s\n' "$APPEX" >&2
    exit 1
fi
printf '%s\n' "$APPEX"
