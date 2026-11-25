#!/bin/sh
# Builds the Arch package in a temporary directory with makepkg.
#
# Usage: packages/arch/build-arch.sh OUTPUT
set -eu

[ $# -eq 1 ] || {
	echo "usage: $0 output-directory" >&2
	exit 64
}
command -v makepkg >/dev/null || {
	echo "build-arch: makepkg is required" >&2
	exit 69
}

here=$(cd "$(dirname "$0")" && pwd)
repository=$(cd "$here/../.." && pwd)
mkdir -p "$1"
output=$(cd "$1" && pwd)
stage=$(mktemp -d "${TMPDIR:-/tmp}/meta-arch.XXXXXX")
trap 'rm -rf "$stage"' EXIT INT TERM

cp "$here/PKGBUILD" "$here/meta-compat.hook" "$here/meta-compat-release" \
	"$repository/LICENSE" "$stage/"
cd "$stage"
PKGDEST=$output makepkg --nodeps --noconfirm
