#!/bin/sh
# Builds the META kernel from a FreeBSD source tree. Never installs it.
#
# Usage: scripts/build-kernel.sh -s SRC -o OBJ [-j JOBS]
set -eu

usage() {
	echo "usage: $0 -s freebsd-source-directory -o object-directory [-j jobs]" >&2
	exit 64
}

src=
obj=
jobs=$(sysctl -n hw.ncpu 2>/dev/null || echo 1)
while getopts s:o:j: option; do
	case $option in
	s) src=$OPTARG ;;
	o) obj=$OPTARG ;;
	j) jobs=$OPTARG ;;
	*) usage ;;
	esac
done
[ -n "$src" ] && [ -n "$obj" ] || usage

if [ "$(uname -s)" != FreeBSD ]; then
	echo "build-kernel: a FreeBSD host is required; this is $(uname -s)" >&2
	exit 69
fi

repository=$(cd "$(dirname "$0")/.." && pwd)
arch=$(uname -m)
if [ ! -f "$src/sys/$arch/conf/GENERIC" ] || [ ! -f "$src/sys/conf/newvers.sh" ]; then
	echo "build-kernel: $src is not a FreeBSD source tree for $arch" >&2
	exit 66
fi
revision=$(sed -n 's/^REVISION="\(.*\)"$/\1/p' "$src/sys/conf/newvers.sh")
case $revision in
15.*) ;;
*)
	echo "build-kernel: META 26.0 is based on FreeBSD 15; $src is $revision" >&2
	exit 65
	;;
esac

mkdir -p "$obj"
echo "Building META from FreeBSD $revision with $jobs jobs"
exec env MAKEOBJDIRPREFIX="$(cd "$obj" && pwd)" make -C "$src" -j "$jobs" \
	KERNCONFDIR="$repository/kernel/conf" KERNCONF=META \
	kernel-toolchain buildkernel
