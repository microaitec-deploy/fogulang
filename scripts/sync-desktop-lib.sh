#!/bin/sh
# Copies the desktop library (../libs/desktop, where it is developed and
# published from) into libs/desktop of this repository, so that the macOS
# hosts can be built by GitHub Actions (.github/workflows/desktop-hosts.yml):
# a macOS executable can only be built on a Mac.
#
#   sh scripts/sync-desktop-lib.sh          # copy
#   sh scripts/sync-desktop-lib.sh --check  # exit 1 when the copy is out of date
#
# Only sources are copied: no build outputs, dependencies or app data.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
SRC="$ROOT/../libs/desktop"
DST="$ROOT/libs/desktop"
[ -f "$SRC/fogu.json" ] || { echo "sync-desktop-lib: $SRC not found" >&2; exit 1; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
(cd "$SRC" && tar -cf - --exclude=target --exclude=node_modules --exclude=dist \
    --exclude=.bundle --exclude=.desktop-data --exclude=hosts --exclude='*.part' \
    --exclude=bun.lock --exclude=package-lock.json .) | (cd "$WORK" && tar -xf -)

if [ "${1:-}" = "--check" ]; then
    if diff -r "$WORK" "$DST" >/dev/null 2>&1; then
        echo "libs/desktop is up to date"
        exit 0
    fi
    echo "libs/desktop differs from ../libs/desktop: run sh scripts/sync-desktop-lib.sh" >&2
    exit 1
fi

rm -rf "$DST"
mkdir -p "$DST"
(cd "$WORK" && tar -cf - .) | (cd "$DST" && tar -xf -)
echo "libs/desktop <- ../libs/desktop ($(grep -m1 '"version"' "$DST/fogu.json" | tr -d ' ,'))"
