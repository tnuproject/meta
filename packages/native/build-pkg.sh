#!/bin/sh
# Builds the meta-tools FreeBSD package with pkg(8).
#
# Usage: packages/native/build-pkg.sh -b BINARIES -o OUTPUT [-v VERSION]
set -eu

usage() {
	echo "usage: $0 -b binary-directory -o output-directory [-v version]" >&2
	exit 64
}

binaries=
output=
# pkg-version(8) orders 26.0.a0 before 26.0.
version=26.0.a0
while getopts b:o:v: option; do
	case $option in
	b) binaries=$OPTARG ;;
	o) output=$OPTARG ;;
	v) version=$OPTARG ;;
	*) usage ;;
	esac
done
[ -n "$binaries" ] && [ -n "$output" ] || usage
case $version in
'' | *[!0-9a-z.]*) usage ;;
esac
command -v pkg >/dev/null || {
	echo "build-pkg: pkg(8) is required" >&2
	exit 69
}

here=$(cd "$(dirname "$0")" && pwd)
repository=$(cd "$here/../.." && pwd)
stage=$(mktemp -d "${TMPDIR:-/tmp}/meta-pkg.XXXXXX")
trap 'rm -rf "$stage"' EXIT INT TERM

root=$stage/root
license=usr/local/share/licenses/meta-tools
mkdir -p "$root/usr/local/bin" "$root/$license" "$output"
: >"$stage/plist"
for program in meta linux2bsd meta-eds meta-installer metaupd; do
	install -m 0555 "$binaries/$program" "$root/usr/local/bin/$program"
	echo "bin/$program" >>"$stage/plist"
done
install -m 0444 "$repository/LICENSE" "$root/$license/LICENSE"
echo "share/licenses/meta-tools/LICENSE" >>"$stage/plist"

sed "s/@VERSION@/$version/" "$here/MANIFEST.in" >"$stage/+MANIFEST"
pkg create -q -M "$stage/+MANIFEST" -p "$stage/plist" -r "$root" -o "$output"
echo "$output/meta-tools-$version.pkg"
