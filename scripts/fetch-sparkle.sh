#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="2.9.6"
ARCHIVE_NAME="Sparkle-${VERSION}.tar.xz"
URL="https://github.com/sparkle-project/Sparkle/releases/download/${VERSION}/${ARCHIVE_NAME}"
SHA256="52bf9e88cdd972fc0c81501377a880e90d47031bd8ca5462488f843e2609e192"
CACHE_ROOT="${TIDEGAUGE_SPARKLE_CACHE:-$ROOT/.cache/sparkle}"
ARCHIVE="$CACHE_ROOT/$ARCHIVE_NAME"
DISTRIBUTIONS_ROOT="$CACHE_ROOT/distributions"
DISTRIBUTION="$DISTRIBUTIONS_ROOT/$VERSION"

mkdir -p "$CACHE_ROOT" "$DISTRIBUTIONS_ROOT"

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

TEMP_DIR="$(mktemp -d "$DISTRIBUTIONS_ROOT/.${VERSION}.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT
tar -xJf "$ARCHIVE" -C "$TEMP_DIR"

if [ ! -d "$TEMP_DIR/Sparkle.framework" ] ||
    [ ! -x "$TEMP_DIR/bin/generate_appcast" ] ||
    [ ! -f "$TEMP_DIR/LICENSE" ]; then
    printf '%s\n' "Pinned Sparkle distribution is incomplete: $ARCHIVE" >&2
    exit 1
fi

rm -rf "$DISTRIBUTION"
mv "$TEMP_DIR" "$DISTRIBUTION"
trap - EXIT

printf '%s\n' "$DISTRIBUTION"
