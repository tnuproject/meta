#!/bin/bash
# Runs milestone tests in the virtual machine started by vm.sh.
#
#   suite.sh build            build the workspace in the machine
#   suite.sh m1 m3 m4 ...     run the named milestones
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
DIR=${META_VM_DIR:-$HOME/.cache/meta-freebsd-vm}
vm() { bash "$HERE/vm.sh" "$@"; }

# guest SCRIPT [VARIABLE=VALUE...]
guest() {
	local script=$1
	shift
	echo "== $script $*"
	{
		for assignment in "$@"; do echo "export $assignment"; done
		cat "$HERE/guest/$script.sh"
	} | vm run
}

console() {
	local firmware=$1 disk=$2 script=$3 iso=${4:-}
	echo "== $script ($firmware)"
	bash "$HERE/console.sh" "$firmware" "$disk" ${iso:+"$iso"} <"$HERE/guest/$script.serial"
}

packages() { echo "for p in $*; do pkg info -e \$p || pkg install -qy \$p </dev/null; done" | vm run; }

for target in "$@"; do
	case $target in
	build)
		vm sync
		packages rust
		echo 'cd /root/meta && cargo build --workspace --release --locked' | vm run
		;;
	unit) echo 'cd /root/meta && cargo build --workspace --locked && cargo test --workspace --locked && sh tests/ota-e2e.sh target/debug' | vm run ;;
	m1)
		guest m1-1-build
		vm reboot
		guest m1-2-booted
		vm reboot
		;;
	m3 | m7)
		packages gnupg debootstrap
		guest m7-editions EDITION=debian REMOVE=1
		guest m7-editions EDITION=arch REMOVE=1
		;;
	m4)
		packages dbus polkit gnupg debootstrap
		guest m4-netd
		;;
	m5)
		packages sudo
		guest m5-install
		console bios "$DIR/target.qcow2" m5-boot
		console uefi "$DIR/target.qcow2" m5-boot
		;;
	m6) guest m6-repository ;;
	m9)
		guest m9-1-install
		vm reboot
		sleep 8
		guest m9-2-confirmed
		guest m9-3-broken
		vm reboot
		sleep 8
		guest m9-4-reverted
		vm reboot
		sleep 8
		guest m9-5-rollback
		vm reboot
		;;
	m10)
		packages dbus polkit xfce xorg-vfbserver xdotool scrot python3 py312-pygobject gtk3 noto-basic
		guest m10-desktop
		;;
	m11)
		guest m11-iso SERIAL=1
		vm get /root/meta.iso "$DIR/meta.iso"
		for firmware in bios uefi; do
			rm -f "$DIR/iso-$firmware.qcow2"
			META_KEEP=1 console $firmware "$DIR/iso-$firmware.qcow2" m11-install "$DIR/meta.iso"
			META_KEEP=1 console $firmware "$DIR/iso-$firmware.qcow2" m11-boot
		done
		;;
	mdcf)
		packages jq
		guest mdcf
		;;
	mlce)
		guest mlce-machine VM=deb KEEP=1
		guest mlce-requirements VM=deb
		echo 'meta-linux vm stop --timeout 900 deb && meta-linux vm set --network open deb && meta-linux vm start --wait 6000 deb' | vm run
		guest mlce-containers VM=deb
		guest mlce-gui VM=deb
		echo 'meta-linux vm stop --timeout 900 deb' | vm run
		guest mlce-machine VM=debv1 CGROUPS=v1
		guest mlce-machine VM=arch IMAGE=arch
		;;
	m12)
		guest m12-prepare
		console bios "$DIR/target.qcow2" m12-rehearsal
		;;
	*)
		echo "unknown target $target" >&2
		exit 64
		;;
	esac
done
