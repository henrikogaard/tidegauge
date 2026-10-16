#!/bin/bash
# Retry a command that talks to Apple's notarization service. Stapling and
# validating fetch the ticket over the network, and a dropped connection
# should not fail a release whose notarization already succeeded. A real
# problem (no ticket) fails every attempt, so it still stops the release.
#
# Usage: retry_network <description> <command> [args...]
# TIDEGAUGE_RETRY_ATTEMPTS (default 5) and TIDEGAUGE_RETRY_DELAY seconds
# (default 15, doubling) tune it.

retry_network() {
    local description="$1"
    shift
    local attempts="${TIDEGAUGE_RETRY_ATTEMPTS:-5}"
    local delay="${TIDEGAUGE_RETRY_DELAY:-15}"
    local attempt=1
    local status=0
    while true; do
        if "$@"; then
            return 0
        else
            status=$?
        fi
        if [ "$attempt" -ge "$attempts" ]; then
            printf '%s failed after %s attempts (exit %s).\n' "$description" "$attempts" "$status" >&2
            return "$status"
        fi
        printf '%s failed (exit %s); retrying in %ss (attempt %s of %s).\n' \
            "$description" "$status" "$delay" "$((attempt + 1))" "$attempts" >&2
        sleep "$delay"
        delay=$((delay * 2))
        attempt=$((attempt + 1))
    done
}
