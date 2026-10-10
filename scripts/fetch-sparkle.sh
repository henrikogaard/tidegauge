#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="2.9.6"
ARCHIVE_NAME="Sparkle-${VERSION}.tar.xz"
URL="https://github.com/sparkle-project/Sparkle/releases/download/${VERSION}/${ARCHIVE_NAME}"
SHA256="52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192"
CACHE_ROOT="${TIDEGAUGE_SPARKLE_CACHE:-$ROOT/.cache/sparkle}"
ARCHIVE="$CACHE_ROOT/$ARCHIVE_NAME"

mkdir -p "$CACHE_ROOT"

download_archive() {
    local temporary
    temporary="$(mktemp "$CACHE_ROOT/.sparkle-download.XXXXXX")"
    trap 'rm -f "$temporary"' EXIT
    curl --fail --location --proto '=https' --tlsv1.2 --silent --show-error \
        "$URL" --output "$temporary"
    printf '%s  %s\n' "$SHA256" "$temporary" | shasum -a 256 -c - >/dev/null
    mv "$temporary" "$ARCHIVE"
    trap - EXIT
}

if [ -f "$ARCHIVE" ]; then
    if ! printf '%s  %s\n' "$SHA256" "$ARCHIVE" | shasum -a 256 -c - >/dev/null; then
        download_archive
    fi
else
    download_archive
fi

if [ -d "$CACHE_ROOT/Sparkle.framework" ]; then
    FRAMEWORK="$CACHE_ROOT/Sparkle.framework"
else
    FRAMEWORK="$(find "$CACHE_ROOT" -type d -path '*/Sparkle.framework' -print -quit 2>/dev/null || true)"
fi
if [ -z "$FRAMEWORK" ]; then
    tar -xJf "$ARCHIVE" -C "$CACHE_ROOT"
    if [ -d "$CACHE_ROOT/Sparkle.framework" ]; then
        FRAMEWORK="$CACHE_ROOT/Sparkle.framework"
    else
        FRAMEWORK="$(find "$CACHE_ROOT" -type d -path '*/Sparkle.framework' -print -quit 2>/dev/null || true)"
    fi
fi

if [ -z "$FRAMEWORK" ]; then
    printf '%s\n' "Sparkle.framework was not found in $ARCHIVE" >&2
    exit 1
fi

printf '%s\n' "$FRAMEWORK"
