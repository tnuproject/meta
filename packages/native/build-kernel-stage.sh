#!/bin/sh
# Installs a kernel built by scripts/build-kernel.sh into a staging directory as
# /boot/kernel.META. Never touches the running system's /boot.
#
# Usage: packages/native/build-kernel-stage.sh -s SRC -o OBJ -d STAGE
set -eu

src=
obj=
stage=
while getopts s:o:d: option; do
	case $option in
	s) src=$OPTARG ;;
	o) obj=$OPTARG ;;
	d) stage=$OPTARG ;;
	*) exit 64 ;;
	esac
done
[ -n "$src" ] && [ -n "$obj" ] && [ -n "$stage" ] || {
	echo "usage: $0 -s source -o objects -d stage" >&2
	exit 64
}
case $stage in /*) ;; *) stage=$PWD/$stage ;; esac
[ ! -e "$stage" ] || {
	echo "build-kernel-stage: $stage exists" >&2
	exit 73
}
repository=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$stage"
env MAKEOBJDIRPREFIX="$obj" make -C "$src" installkernel \
	KERNCONFDIR="$repository/kernel/conf" KERNCONF=META \
	INSTKERNNAME=kernel.META DESTDIR="$stage" WITHOUT_KERNEL_SYMBOLS=yes
