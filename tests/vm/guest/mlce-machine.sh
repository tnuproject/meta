# MLCE: a Linux distribution with its own kernel and systemd, under bhyve.
# IMAGE, CGROUPS and VM select what is tested; KEEP leaves the machine running.
set -eu
. /root/meta/tests/vm/lib.sh
VM=${VM:-deb}
IMAGE=${IMAGE:-debian-12}
CGROUPS=${CGROUPS:-v2}
# Nested virtualization is very slow.
WAIT=${WAIT:-3000}

for package in edk2-bhyve libslirp qemu-tools; do
	pkg info -e $package || pkg install -qy $package </dev/null
done
for tool in meta-linux meta-exec; do install -m 755 "$BIN/$tool" /usr/local/bin/; done
# Runs a script in the guest, as the machine's owner.
# This script arrives on standard input, which ssh would otherwise read.
guest() { su -m tester -c 'exec meta-linux vm exec "$0" -- sh -c "$1"' "$VM" "$1" </dev/null; }
guest_input() { su -m tester -c 'exec meta-linux vm exec "$0" -- sh -c "$1"' "$VM" "$1"; }

if ! meta-linux vm status "$VM" >/dev/null 2>&1; then
	meta-linux vm create --owner tester --image "$IMAGE" --cgroups "$CGROUPS" "$VM" |
		grep -q 'Nothing was changed' || fail "creating without --yes"
	[ ! -e "/var/db/meta-linux/vms/$VM" ] || fail "something was created without --yes"
	ok "creation shows its plan first"
	meta-linux vm create --owner tester --image "$IMAGE" --cgroups "$CGROUPS" --yes "$VM"
fi
# A guest of a guest of a guest is so slow that systemd's time limits expire. On such
# hosts, SLOW=1 raises them, in a Debian disk that has not been started yet. Nothing
# else of the image is changed.
if [ -n "${SLOW:-}" ] && [ "$IMAGE" = debian-12 ] && [ ! -e "/var/db/meta-linux/vms/$VM/console.log" ]; then
	kldload -n ext2fs
	unit=$(mdconfig -a -t vnode -f "/var/db/meta-linux/vms/$VM/data/disk.raw")
	mkdir -p /mnt/guest
	mount -t ext2fs "/dev/${unit}p1" /mnt/guest
	mkdir -p /mnt/guest/etc/systemd/system.conf.d
	printf '[Manager]
DefaultTimeoutStartSec=30min
DefaultDeviceTimeoutSec=30min
' 		>/mnt/guest/etc/systemd/system.conf.d/slow.conf
	sed -i '' -e 's/earlyprintk=ttyS0,115200//' -e '/^[[:space:]]*linux[[:space:]]/s/$/ mitigations=off nowatchdog highres=off quiet systemd.default_timeout_start_sec=1800 systemd.default_device_timeout_sec=1800/' 		/mnt/guest/boot/grub/grub.cfg
	umount /mnt/guest
	mdconfig -d -u "$unit"
fi
if meta-linux vm status "$VM" | grep -q Stopped; then
	meta-linux vm start "$VM"
fi
meta-linux vm start --wait 1 "$VM" 2>&1 | grep -q 'is running' || fail "started twice"
ok "a running machine is not started again"

# Confinement.
pgrep -x bhyve >/dev/null || fail "bhyve is not running"
owners=$(ps -axo user,comm | awk '$2 ~ /^bhyve/ { print $1 }' | sort -u)
[ "$owners" = tester ] || fail "bhyve runs as $owners"
for pid in $(pgrep bhyve); do
	[ "$(ps -o jid= -p "$pid" | tr -d ' ')" != 0 ] || fail "process $pid is not in a jail"
done
ok "bhyve and its helper run as the owner, in a jail"
root=/var/run/meta-linux/$VM
[ ! -e $root/etc/ssh ] && [ "$(wc -l <$root/etc/master.passwd)" -le 3 ] || fail "the jail sees /etc"
[ ! -e $root/home ] && [ ! -e $root/root ] || fail "the jail sees home directories"
[ ! -e $root/dev/mem ] && [ ! -e $root/dev/pci ] && [ ! -e $root/dev/zfs ] || fail "the jail sees devices"
ok "the jail sees neither the host's files nor its devices"
sockstat -4l | grep bhyve | grep -vq '127.0.0.1:' && fail "a port is open to the network"
ok "nothing listens on the network"

deadline=$(($(date +%s) + WAIT))
until guest true 2>/dev/null; do
	[ "$(date +%s)" -lt "$deadline" ] || { meta-linux vm console "$VM" | tail -20; fail "no answer"; }
	meta-linux vm status "$VM" | grep -q Running || { meta-linux vm console "$VM" | tail -20; fail "stopped"; }
	sleep 10
done
ok "the guest answers"
until guest "systemctl is-system-running" 2>/dev/null | grep -Eq 'running|degraded'; do
	[ "$(date +%s)" -lt "$deadline" ] || fail "systemd is not ready"
	sleep 15
done
guest "cat /etc/os-release" | grep -E '^PRETTY_NAME'
guest "uname -sr"
[ "$(guest "cat /proc/1/comm")" = systemd ] || fail "systemd is not the first process"
ok "systemd is the first process: $(guest "systemctl is-system-running" || true)"
guest "systemctl --failed --no-legend" || true

guest "sudo unshare --pid --fork --mount-proc sh -c 'echo \$\$'" | grep -qx 1 ||
	fail "namespaces"
ok "a process is number 1 in a namespace of its own"
case $CGROUPS in
v2)
	[ "$(guest "stat -fc %T /sys/fs/cgroup")" = cgroup2fs ] || fail "not cgroup v2"
	guest "sudo systemd-run --quiet --wait --pipe -p MemoryMax=64M sh -c 'cat /sys/fs/cgroup\$(cut -d: -f3 /proc/self/cgroup)/memory.max'" |
		grep -qx 67108864 || fail "a limit of cgroup v2"
	;;
v1)
	[ "$(guest "stat -fc %T /sys/fs/cgroup")" = tmpfs ] || fail "not cgroup v1"
	guest "sudo systemd-run --quiet --wait --pipe -p MemoryLimit=64M sh -c 'cat /sys/fs/cgroup/memory\$(grep :memory: /proc/self/cgroup | cut -d: -f3)/memory.limit_in_bytes'" |
		grep -qx 67108864 || fail "a limit of cgroup v1"
	;;
esac
ok "cgroup $CGROUPS limits a service"
guest "sudo modprobe dummy && lsmod | grep -q '^dummy' && sudo rmmod dummy" || fail "kernel modules"
ok "a Linux kernel module is loaded and unloaded"
guest "sudo dmesg" >/tmp/mlce-dmesg 2>/dev/null || true
grep -i 'hypervisor detected' /tmp/mlce-dmesg || true

# Sharing.
share=/var/meta-linux/share/$VM
echo from-the-host >"$share/host-file"
guest "cat /mnt/host/share/host-file" | grep -qx from-the-host || fail "the guest does not see the share"
guest "echo from-the-guest >/mnt/host/share/guest-file"
[ "$(cat "$share/guest-file")" = from-the-guest ] || fail "the host does not see the guest's file"
[ "$(stat -f %Su "$share/guest-file")" = tester ] || fail "the guest's file belongs to $(stat -f %Su "$share/guest-file")"
ok "files pass both ways"
rm -f "$share/root-file"
guest "sudo sh -c 'echo x >/mnt/host/share/root-file'" 2>/dev/null && fail "the guest's root made a file of root's"
[ ! -e "$share/root-file" ] || [ "$(stat -f %Su "$share/root-file")" = tester ] || fail "a file of root's appeared"
ok "the guest's root cannot make files that belong to anyone but the owner"
guest "sudo sh -c 'cp /bin/true /mnt/host/share/suid; chown 0 /mnt/host/share/suid; chmod 4755 /mnt/host/share/suid'" 2>/dev/null || true
[ "$(find "$share" -user root -perm -4000 | wc -l)" -eq 0 ] || fail "a set-user-ID root program was made"
zfs get -Ho value setuid "$(zfs list -Ho name "$share")" | grep -qx off || fail "the share allows set-user-ID programs"
ok "no set-user-ID root program can be left on the host"

# The network.
if meta-linux vm status "$VM" | grep -q Isolated; then
	guest "timeout 10 bash -c '</dev/tcp/10.0.2.2/22'" 2>/dev/null && fail "the guest reaches the host"
	guest "timeout 10 bash -c '</dev/tcp/1.1.1.1/443'" 2>/dev/null && fail "the guest reaches the network"
	ok "an isolated guest reaches neither the host nor the network"
fi

# Launching.
[ "$(su -m tester -c "meta-linux vm exec $VM -- printf '<%s>' 'a  b' '\$HOME' ''" </dev/null)" = "<a  b><\$HOME><>" ] || fail "arguments"
[ "$(echo text | guest_input 'exec cat')" = text ] || fail "standard input"
set +e
guest 'exit 42'
[ $? -eq 42 ] || fail "exit code"
guest 'kill -9 $$'
[ $? -eq 137 ] || fail "death by signal"
set -e
ok "arguments, standard input and exit codes pass through"
guest 'trap "exit 9" TERM; echo ready; while :; do sleep 0.2; done' >/tmp/mlce-signal &
pid=$!
for _ in $(seq 60); do grep -q ready /tmp/mlce-signal && break; sleep 1; done
launcher=$(pgrep -f "meta-linux vm exec $VM" | head -1)
kill -TERM "$launcher"
set +e
wait $pid
code=$?
set -e
[ $code -eq 9 ] || fail "the signal was not passed on ($code)"
ok "a signal reaches the program in the guest"
pw usershow stranger >/dev/null 2>&1 || pw useradd stranger -m -s /bin/sh
expect_failure "another user cannot use the machine" 'belongs to tester' \
	su -m stranger -c "meta-linux vm exec $VM -- true"
expect_failure "nor read its key" 'Permission denied' \
	su -m stranger -c "cat /var/db/meta-linux/vms/$VM/client_key"
expect_failure "nor change machines" 'needs root' su -m tester -c "meta-linux vm stop $VM"

meta-linux vm diagnose "$VM" || true
meta-linux --json vm status "$VM" | grep -q '"state": "running"' || fail json
[ -n "${KEEP:-}" ] || {
	meta-linux vm stop --timeout 900 "$VM"
	meta-linux vm status "$VM" | grep -E 'state|last'
	meta-linux vm status "$VM" | grep -q 'powered off' || fail "the guest did not power off by itself"
	! mount | grep -q "/var/run/meta-linux/$VM" || fail "mounts were left behind"
	! jls -n name | grep -q "meta-vm-$VM" || fail "the jail was left behind"
	ok "the guest powers off when asked, and nothing is left behind"
	grep -q "stop	$VM	ok" /var/db/meta-linux/audit.log || fail "no audit record"
	ok "operations are recorded"
}
echo "PASS mlce-machine $IMAGE $CGROUPS"
