#!/bin/sh
# Fetches the pinned FreeBSD source tree. git verifies it against the commit.
#
# Usage: scripts/fetch-kernel-source.sh DIRECTORY
set -eu

[ $# -eq 1 ] || {
	echo "usage: $0 directory" >&2
	exit 64
}
pin=$(cd "$(dirname "$0")/.." && pwd)/kernel/SOURCE
repository=$(sed -n 's/^repository=//p' "$pin")
commit=$(sed -n 's/^commit=//p' "$pin")
mkdir -p "$1"
cd "$1"
if [ "$(git rev-parse HEAD 2>/dev/null || true)" = "$commit" ]; then
	echo "already at $commit"
	exit 0
fi
[ -d .git ] || git init -q
git fetch -q --depth 1 "$repository" "$commit"
git -c advice.detachedHead=false checkout -q FETCH_HEAD
[ "$(git rev-parse HEAD)" = "$commit" ] || {
	echo "fetch-kernel-source: unexpected commit" >&2
	exit 65
}
git fsck --no-progress --connectivity-only >/dev/null
echo "fetched $commit"
