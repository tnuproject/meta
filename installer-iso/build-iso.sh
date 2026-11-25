#!/bin/sh
# Builds a bootable META installation image for BIOS and UEFI.
#
# Usage: installer-iso/build-iso.sh -r REPOSITORIES -t TRUST -s SRC -o IMAGE
#            [-V META_VERSION] [-S] [PACKAGE...]
#
# REPOSITORIES  pkg configuration with FreeBSD-base, FreeBSD-ports and META
# TRUST         directory with root.json, root.sig.json and packages.pub
# SRC           FreeBSD source tree, for its image scripts
# -S            make the serial port the main console, for testing
# PACKAGE       extra packages to carry for offline installation
#
# The image holds a small live system and a repository with everything an
# installation without a desktop needs, so no network is required.
set -eu

usage() {
	sed -n '2,/^set/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2
	exit 64
}

repositories=
trust=
src=
image=
version=26.0
serial=
while getopts r:t:s:o:V:S option; do
	case $option in
	r) repositories=$OPTARG ;;
	t) trust=$OPTARG ;;
	s) src=$OPTARG ;;
	o) image=$OPTARG ;;
	V) version=$OPTARG ;;
	S) serial=yes ;;
	*) usage ;;
	esac
done
shift $((OPTIND - 1))
[ -n "$repositories" ] && [ -n "$trust" ] && [ -n "$src" ] && [ -n "$image" ] || usage
if [ "$(uname -s)" != FreeBSD ]; then
	echo "build-iso: a FreeBSD host is required" >&2
	exit 69
fi
case $(uname -m) in
amd64) ;;
*)
	echo "build-iso: only amd64 is supported" >&2
	exit 69
	;;
esac
for file in root.json root.sig.json packages.pub; do
	[ -f "$trust/$file" ] || {
		echo "build-iso: $trust/$file is missing" >&2
		exit 66
	}
done
case $image in /*) ;; *) image=$PWD/$image ;; esac
here=$(cd "$(dirname "$0")" && pwd)

work=$(mktemp -d "${TMPDIR:-/var/tmp}/meta-iso.XXXXXX")
trap 'chflags -R noschg "$work" 2>/dev/null || true; rm -rf "$work"' EXIT INT TERM
root=$work/root
media=$root/usr/local/share/meta-installer
mkdir -p "$root/usr/share/keys" "$root/usr/local/etc/meta/trust" "$media/repos"
cp -R /usr/share/keys/ "$root/usr/share/keys/"
cp "$trust/root.json" "$trust/root.sig.json" "$trust/packages.pub" "$root/usr/local/etc/meta/trust/"

# What an installation needs; kept in step with installer/src/execute.rs.
target_packages=$(sed -n '/^const PACKAGES/,/^];/s/^ *"\(.*\)",$/\1/p' "$here/../installer/src/execute.rs")
[ -n "$target_packages" ] || {
	echo "build-iso: cannot read the package list" >&2
	exit 65
}

echo "==> live system"
# shellcheck disable=SC2086
pkg -r "$root" -R "$repositories" -o IGNORE_OSVERSION=yes -o ASSUME_ALWAYS_YES=yes \
	install -q -y $target_packages meta-installer
pkg -r "$root" clean -qay >/dev/null 2>&1 || true
rm -rf "$root/var/cache/pkg"

echo "==> packages for offline installation"
# A separate, empty root, so that nothing counts as installed already.
empty=$work/empty
mkdir -p "$empty/usr/share/keys" "$empty/usr/local/etc/meta/trust" "$empty/fetched"
cp -R /usr/share/keys/ "$empty/usr/share/keys/"
cp "$trust/packages.pub" "$empty/usr/local/etc/meta/trust/"
# Exactly what the live system resolved. Following declared dependencies alone
# would miss packages that are only needed for their shared libraries.
resolved=$(pkg -r "$root" query '%n')
for_empty() {
	pkg -r "$empty" -R "$repositories" -o IGNORE_OSVERSION=yes -o ASSUME_ALWAYS_YES=yes "$@"
}
# shellcheck disable=SC2086
for_empty fetch -q -y -o "$work/fetched" $resolved
if [ $# -gt 0 ]; then
	for_empty fetch -q -y -d -o "$work/fetched" "$@"
fi
mkdir -p "$media/packages"
find "$work/fetched" -name '*.pkg' -exec cp {} "$media/packages/" \;
pkg repo -q "$media/packages"
# The medium itself is what the user verifies; see docs/installation/media.md.
cat >"$media/repos/media.conf" <<EOF
media: {
  url: "file:///usr/local/share/meta-installer/packages",
  signature_type: "none",
  enabled: yes
}
EOF

echo "==> configuration"
label=META_$(echo "$version" | tr -c 'A-Za-z0-9\n' '_' | tr '[:lower:]' '[:upper:]')
echo "/dev/iso9660/$label / cd9660 ro 0 0" >"$root/etc/fstab"
cat >"$root/etc/rc.conf" <<EOF
hostname="meta-live"
root_rw_mount="NO"
varmfs="YES"
varsize="64m"
tmpmfs="YES"
tmpsize="256m"
hostid_enable="NO"
entropy_file="NO"
dumpdev="NO"
cron_enable="NO"
sendmail_enable="NONE"
syslogd_flags="-ss"
metaupd_confirm_enable="NO"
EOF
cat >>"$root/boot/loader.conf" <<EOF
zfs_load="YES"
autoboot_delay="3"
kern.geom.label.disk_ident.enable="0"
kern.geom.label.gptid.enable="0"
EOF
if [ -n "$serial" ]; then
	cat >>"$root/boot/loader.conf" <<EOF
console="comconsole,vidconsole"
boot_multicons="YES"
boot_serial="YES"
EOF
fi
install -m 0555 "$here/rc.local" "$root/etc/rc.local"
cap_mkdb "$root/etc/login.conf"
touch "$root/META-LIVE"
mkdir -p "$root/mnt"

echo "==> image"
(cd "$src/release/amd64" && sh ./mkisoimages.sh -b "$label" "$image" "$root")
ls -lh "$image"
