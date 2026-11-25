#!/bin/sh
# Builds meta-compat-environment, the package that identifies and protects a
# META Debian Edition compatibility root.
#
# Usage: packages/debian/build-deb.sh OUTPUT
set -eu

[ $# -eq 1 ] || {
	echo "usage: $0 output-directory" >&2
	exit 64
}
command -v dpkg-deb >/dev/null || {
	echo "build-deb: dpkg-deb is required" >&2
	exit 69
}

here=$(cd "$(dirname "$0")" && pwd)
repository=$(cd "$here/../.." && pwd)
mkdir -p "$1"
output=$(cd "$1" && pwd)
stage=$(mktemp -d "${TMPDIR:-/tmp}/meta-deb.XXXXXX")
trap 'rm -rf "$stage"' EXIT INT TERM
umask 022

# Debian orders 26.0~dev before 26.0.
version=26.0~dev0
root=$stage/root
doc=$root/usr/share/doc/meta-compat-environment
mkdir -p "$root/DEBIAN" "$root/etc/apt/preferences.d" "$doc"
chmod 755 "$stage" "$root"
sed "s/@VERSION@/$version/" "$here/control.in" >"$root/DEBIAN/control"
printf '/etc/apt/preferences.d/meta-compat\n/etc/meta-compat-release\n' >"$root/DEBIAN/conffiles"
install -m 644 "$here/preferences" "$root/etc/apt/preferences.d/meta-compat"
printf 'META_VERSION=26.0\nMETA_EDITION=debian\n' >"$root/etc/meta-compat-release"
chmod 644 "$root/etc/meta-compat-release" "$root/DEBIAN/conffiles" "$root/DEBIAN/control"
install -m 644 "$repository/LICENSE" "$doc/copyright"

package=$output/meta-compat-environment_${version}_all.deb
dpkg-deb --root-owner-group --build "$root" "$package" >/dev/null
dpkg-deb --info "$package" >/dev/null
echo "$package"
