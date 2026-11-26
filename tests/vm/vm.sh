#!/bin/bash
# Runs a disposable FreeBSD virtual machine for tests. Linux host with QEMU.
#
#   vm.sh fetch             download and verify the FreeBSD image
#   vm.sh start             boot a fresh overlay and wait for SSH
#   vm.sh run < script      run a script in the machine as root
#   vm.sh sync              copy this source tree to /root/meta
#   vm.sh copy FILE...      copy files to /root
#   vm.sh get PATH DEST     copy a file out of the machine
#   vm.sh reboot            reboot and wait for SSH
#   vm.sh stop              power off and delete the overlay
#
# META_VM_DIR selects the working directory. Only localhost can reach the machine.
set -eu

RELEASE=15.1-RELEASE
IMAGE=FreeBSD-$RELEASE-amd64-BASIC-CLOUDINIT-zfs.qcow2
URL=https://download.freebsd.org/releases/VM-IMAGES/$RELEASE/amd64/Latest
DIR=${META_VM_DIR:-$HOME/.cache/meta-freebsd-vm}
PORT=${META_VM_PORT:-2222}
REPOSITORY=$(cd "$(dirname "$0")/../.." && pwd)

mkdir -p "$DIR"

SSH_OPTIONS=(-q -i "$DIR/id" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
	-o ConnectTimeout=5 -o BatchMode=yes)

guest() { ssh "${SSH_OPTIONS[@]}" -p "$PORT" root@127.0.0.1 "$@"; }

fetch() {
	cd "$DIR"
	[ ! -f base.qcow2 ] || return 0
	curl -fsSO "$URL/CHECKSUM.SHA512"
	curl -fsSL -C - -o "$IMAGE.xz" "$URL/$IMAGE.xz"
	local expected actual
	expected=$(sed -n "s/^SHA512 ($IMAGE.xz) = //p" CHECKSUM.SHA512)
	actual=$(sha512sum "$IMAGE.xz" | cut -d' ' -f1)
	if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
		echo "vm: checksum mismatch for $IMAGE.xz" >&2
		exit 1
	fi
	xz -dc "$IMAGE.xz" >base.qcow2.new
	mv base.qcow2.new base.qcow2
	rm "$IMAGE.xz"
}

wait_ssh() {
	local boot_before=${1:-} boot
	for _ in $(seq 1 120); do
		boot=$(guest sysctl -n kern.boottime 2>/dev/null || true)
		if [ -n "$boot" ] && [ "$boot" != "$boot_before" ] &&
			guest test -f /var/run/meta-vm-ready -o -f /root/.meta-vm-ready 2>/dev/null; then
			return 0
		fi
		sleep 2
	done
	echo "vm: no answer" >&2
	return 1
}

make_seed() {
	[ -f "$DIR/id" ] || ssh-keygen -q -t ed25519 -N '' -f "$DIR/id"
	mkdir -p "$DIR/seed"
	printf 'instance-id: meta-test\nlocal-hostname: metatest\n' >"$DIR/seed/meta-data"
	cat >"$DIR/seed/user-data" <<EOF
#cloud-config
users:
  - name: tester
    shell: /bin/sh
runcmd:
  - mkdir -p /root/.ssh
  - echo '$(cat "$DIR/id.pub")' > /root/.ssh/authorized_keys
  - chmod 700 /root/.ssh
  - chmod 600 /root/.ssh/authorized_keys
  - echo 'PermitRootLogin prohibit-password' >> /etc/ssh/sshd_config
  - touch /root/.meta-vm-ready
  - service sshd restart
EOF
	(cd "$DIR" && genisoimage -quiet -o seed.iso -V cidata -r -J seed)
}

start() {
	[ -f "$DIR/base.qcow2" ] || fetch
	make_seed
	rm -f "$DIR/overlay.qcow2" "$DIR/target.qcow2"
	qemu-img create -q -f qcow2 -b "$DIR/base.qcow2" -F qcow2 "$DIR/overlay.qcow2" 60G
	qemu-img create -q -f qcow2 "$DIR/target.qcow2" 40G
	boot
	wait_ssh
}

# Boots the machine in the foreground when META_VM_FOREGROUND is set.
boot() {
	local accel=tcg cpu=max
	# Without an invariant TSC, guests of guests ask the hypervisor for the time.
	if [ -w /dev/kvm ]; then accel=kvm cpu=host,migratable=off,+invtsc; fi
	local command=(qemu-system-x86_64 -accel "$accel" -cpu "$cpu" -smp "${META_VM_CPUS:-8}" -m "${META_VM_MEMORY:-8G}"
		-display none -serial "file:$DIR/console.log" -device virtio-rng-pci
		-drive "file=$DIR/overlay.qcow2,if=virtio,format=qcow2"
		-drive "file=$DIR/target.qcow2,if=virtio,format=qcow2"
		-cdrom "$DIR/seed.iso" -pidfile "$DIR/pid"
		-netdev "user,id=n0,hostfwd=tcp:127.0.0.1:$PORT-:22" -device "virtio-net-pci,netdev=n0"
		-netdev "user,id=n1,net=10.0.3.0/24" -device "virtio-net-pci,netdev=n1")
	if [ -n "${META_VM_FOREGROUND:-}" ]; then
		exec "${command[@]}"
	fi
	"${command[@]}" -daemonize
}

case ${1:-} in
fetch) fetch ;;
start) start ;;
prepare)
	[ -f "$DIR/base.qcow2" ] || fetch
	make_seed
	rm -f "$DIR/overlay.qcow2" "$DIR/target.qcow2"
	qemu-img create -q -f qcow2 -b "$DIR/base.qcow2" -F qcow2 "$DIR/overlay.qcow2" 60G
	qemu-img create -q -f qcow2 "$DIR/target.qcow2" 40G
	;;
boot) boot ;;
wait) wait_ssh ;;
run) guest /bin/sh -s ;;
sync)
	cd "$REPOSITORY"
	tar --exclude=./target --exclude=./.git --exclude=./.claude --exclude=./dist -cf - . |
		guest 'rm -rf /root/meta.new && mkdir /root/meta.new && tar -xf - -C /root/meta.new &&
			{ [ ! -d /root/meta/target ] || mv /root/meta/target /root/meta.new/target; } &&
			rm -rf /root/meta && mv /root/meta.new /root/meta'
	;;
copy)
	shift
	scp "${SSH_OPTIONS[@]}" -P "$PORT" "$@" root@127.0.0.1:/root/
	;;
get) scp "${SSH_OPTIONS[@]}" -P "$PORT" "root@127.0.0.1:$2" "$3" ;;
reboot)
	before=$(guest sysctl -n kern.boottime)
	guest '(sleep 1; reboot) >/dev/null 2>&1 &'
	sleep 5
	wait_ssh "$before"
	;;
stop)
	guest '(sleep 1; shutdown -p now) >/dev/null 2>&1 &' || true
	for _ in $(seq 1 30); do
		if [ ! -f "$DIR/pid" ] || ! kill -0 "$(cat "$DIR/pid")" 2>/dev/null; then break; fi
		sleep 1
	done
	rm -f "$DIR/overlay.qcow2" "$DIR/target.qcow2" "$DIR/seed.iso" "$DIR/console.log"
	rm -rf "$DIR/seed"
	;;
*)
	sed -n '2,/^set/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2
	exit 64
	;;
esac
