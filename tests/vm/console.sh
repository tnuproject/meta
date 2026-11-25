#!/bin/bash
# Boots a second virtual machine and drives its serial console with a script.
#
#   console.sh bios|uefi DISK [ISO] < serial-script
#
# DISK is a qcow2 image; it is used through a temporary overlay unless
# META_KEEP is set. "none" means no disk. A blank 40G disk is made when DISK does
# not exist.
set -eu

DIR=${META_VM_DIR:-$HOME/.cache/meta-freebsd-vm}
HERE=$(cd "$(dirname "$0")" && pwd)
firmware=$1
disk=$2
iso=${3:-}
work=$(mktemp -d "$DIR/console.XXXXXX")
trap 'kill "$(cat "$work/pid" 2>/dev/null)" 2>/dev/null || true; rm -rf "$work"' EXIT

accel=tcg cpu=max
if [ -w /dev/kvm ]; then accel=kvm cpu=host; fi
command=(qemu-system-x86_64 -accel "$accel" -cpu "$cpu" -smp 4 -m "${META_VM_MEMORY:-4G}"
	-display none -daemonize -pidfile "$work/pid"
	-chardev "socket,id=console,path=$work/serial,server=on,wait=off" -serial chardev:console
	-monitor "unix:$work/monitor,server=on,wait=off"
	-netdev user,id=n0 -device virtio-net-pci,netdev=n0)
if [ "$disk" != none ]; then
	if [ ! -e "$disk" ]; then
		qemu-img create -q -f qcow2 "$disk" 40G
	fi
	if [ -z "${META_KEEP:-}" ]; then
		# A copy, because the main machine may still hold the image open.
		cp "$disk" "$work/base.qcow2"
		qemu-img create -q -f qcow2 -b "$work/base.qcow2" -F qcow2 "$work/disk.qcow2"
		disk=$work/disk.qcow2
	fi
	command+=(-drive "file=$disk,if=virtio,format=qcow2")
fi
if [ -n "$iso" ]; then
	command+=(-cdrom "$iso" -boot d)
fi
if [ "$firmware" = uefi ]; then
	cp /usr/share/OVMF/OVMF_VARS_4M.fd "$work/vars.fd"
	command+=(-machine q35
		-drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE_4M.fd
		-drive "if=pflash,format=raw,file=$work/vars.fd")
fi
"${command[@]}"
status=0
python3 "$HERE/serial.py" "$work/serial" "${META_LOG:-$DIR/console-last.log}" || status=$?
exit "$status"
