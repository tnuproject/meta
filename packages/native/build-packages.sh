#!/bin/sh
# Builds META's FreeBSD packages with pkg(8).
#
# Usage: packages/native/build-packages.sh -b BINARIES -o OUTPUT -V META_VERSION
#            [-k KERNEL_STAGE] [PACKAGE...]
#
# Each directory below packages/native with a MANIFEST.in is a package. Its
# "files" list has lines of: mode, source, destination. Sources are relative to
# the repository, or to the binary directory when they start with "@".
# meta-kernel is built only when -k names an installed kernel (see
# build-kernel-stage.sh).
set -eu

usage() {
	echo "usage: $0 -b binaries -o output -V meta-version [-k kernel-stage] [package ...]" >&2
	exit 64
}

binaries=
output=
meta_version=
kernel=
while getopts b:o:V:k: option; do
	case $option in
	b) binaries=$OPTARG ;;
	o) output=$OPTARG ;;
	V) meta_version=$OPTARG ;;
	k) kernel=$OPTARG ;;
	*) usage ;;
	esac
done
shift $((OPTIND - 1))
[ -n "$binaries" ] && [ -n "$output" ] && [ -n "$meta_version" ] || usage
command -v pkg >/dev/null || {
	echo "build-packages: pkg(8) is required" >&2
	exit 69
}

here=$(cd "$(dirname "$0")" && pwd)
repository=$(cd "$here/../.." && pwd)
mkdir -p "$output"
output=$(cd "$output" && pwd)

# 26.0-alpha1 becomes 26.0.a1, which pkg-version(8) orders before 26.0.
version=$(echo "$meta_version" |
	sed -nE 's/^([0-9]+\.[0-9]+(\.[0-9]+)?)$/\1/p; s/^([0-9]+\.[0-9]+(\.[0-9]+)?)-(a|b|r)[a-z]*([0-9]+)$/\1.\3\4/p')
[ -n "$version" ] || {
	echo "build-packages: invalid META version $meta_version" >&2
	exit 64
}

stage=$(mktemp -d "${TMPDIR:-/tmp}/meta-pkg.XXXXXX")
trap 'rm -rf "$stage"' EXIT INT TERM

create() {
	name=$1
	root=$2
	plist=$3
	sed -e "s/@VERSION@/$version/g" -e "s/@META_VERSION@/$meta_version/g" \
		"$here/$name/MANIFEST.in" >"$stage/+MANIFEST"
	pkg create -q -M "$stage/+MANIFEST" -p "$plist" -r "$root" -o "$output"
	echo "$output/$name-$version.pkg"
}

build() {
	name=$1
	root=$stage/$name
	plist=$stage/$name.plist
	mkdir -p "$root"
	: >"$plist"
	if [ "$name" = meta-kernel ]; then
		[ -n "$kernel" ] || return 0
		cp -Rp "$kernel/boot" "$root/boot"
		mkdir -p "$root/boot/loader.conf.d"
		echo 'kernel="kernel.META"' >"$root/boot/loader.conf.d/meta-kernel.conf"
		(cd "$root" && find boot -type f | sort | sed 's|^|/|') >"$plist"
		create "$name" "$root" "$plist"
		return
	fi
	while read -r mode source destination; do
		case $mode in '' | '#'*) continue ;; esac
		case $source in
		@*) file=$binaries/${source#@} ;;
		*) file=$repository/$source ;;
		esac
		mkdir -p "$root$(dirname "$destination")"
		sed -e "s/@META_VERSION@/$meta_version/g" "$file" >"$root$destination.new" 2>/dev/null ||
			cp "$file" "$root$destination.new"
		case $source in @*) cp "$file" "$root$destination.new" ;; esac
		mv "$root$destination.new" "$root$destination"
		chmod "$mode" "$root$destination"
		echo "$destination" >>"$plist"
	done <"$here/$name/files"
	create "$name" "$root" "$plist"
}

if [ $# -eq 0 ]; then
	for manifest in "$here"/*/MANIFEST.in; do
		build "$(basename "$(dirname "$manifest")")"
	done
else
	for name in "$@"; do
		build "$name"
	done
fi
