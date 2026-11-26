# MLCE: the same program asks FreeBSD's Linuxulator and a Linux guest which system
# calls exist, and the answers are compared with the registry.
# Expects a running machine called VM that belongs to "tester".
set -eu
. /root/meta/tests/vm/lib.sh
VM=${VM:-deb}
as_tester() { su -m tester -c "XDG_STATE_HOME=/tmp/mlce/state $*" </dev/null; }

install -d -o tester /tmp/mlce
cc --target=x86_64-unknown-linux-gnu -ffreestanding -fno-stack-protector -nostdlib -static \
	-fuse-ld=lld -O1 -o /tmp/mlce/linux-probe /root/meta/tests/vm/guest/linux-probe.c
brandelf -t Linux /tmp/mlce/linux-probe
kldload -n linux64
for tool in meta-linux meta-exec; do install -m 755 "$BIN/$tool" /usr/local/bin/; done

/tmp/mlce/linux-probe >/tmp/mlce/linuxulator
as_tester "meta-exec run --runtime vm /tmp/mlce/linux-probe" >/tmp/mlce/guest
paste /tmp/mlce/linuxulator /tmp/mlce/guest
for name in getpid statx memfd_create; do
	grep -qx "$name present" /tmp/mlce/linuxulator || fail "$name is missing in Linuxulator"
done
ok "Linuxulator has the calls every program uses"
for name in unshare setns pivot_root init_module finit_module bpf perf_event_open \
	io_uring_setup fsopen pidfd_open landlock_create_ruleset; do
	grep -qx "$name missing" /tmp/mlce/linuxulator || fail "Linuxulator has $name; the registry is wrong"
	grep -qx "$name present" /tmp/mlce/guest || fail "the guest lacks $name"
done
ok "namespaces, modules and newer calls exist in the guest only"

# The registry says the same about every call in the probe.
while read -r name state; do
	case $state in present) table=implemented ;; *) table=placeholder ;; esac
	awk -v t="[$table]" -v n="$name" '$0 ~ /^\[/ { on = ($0 == t) } on && $1 == n { found = 1 }
		END { exit !found }' /root/meta/compatibility/data/linuxulator-15.1.toml ||
		echo "registry differs: $name $state"
done </tmp/mlce/linuxulator >/tmp/mlce/differences
[ ! -s /tmp/mlce/differences ] || cat /tmp/mlce/differences

check "a program without requirements runs on Linuxulator" \
	sh -c 'meta-exec explain /tmp/mlce/linux-probe | grep -q "^linuxulator is used"'
printf 'schema = 1\nrequires = ["kernel:namespaces"]\n' >/tmp/mlce/linux-probe.requirements.toml
check "declared namespaces lead to the machine" \
	su -m tester -c "meta-exec explain /tmp/mlce/linux-probe | grep -q '^virtual-machine ($VM) is used'"
as_tester "meta-exec run /tmp/mlce/linux-probe unshare" || fail "unshare in the guest"
ok "and the program finds them there"
expect_failure "a preference does not override a requirement" 'kernel:namespaces is not available' \
	su -m tester -c "meta-exec run --runtime linuxulator /tmp/mlce/linux-probe"
rm /tmp/mlce/linux-probe.requirements.toml

as_tester "meta-exec analyze --trial /tmp/mlce/linux-probe unshare" >/tmp/mlce/trial || true
cat /tmp/mlce/trial
as_tester "meta-exec explain /tmp/mlce/linux-probe" | grep -q "^virtual-machine ($VM) is used" ||
	fail "a failed trial does not lead to the machine"
ok "a program that failed for a missing call is sent to the machine"
expect_failure "nothing is tried out as root" 'not tried out as root' \
	meta-exec analyze --trial /tmp/mlce/linux-probe
rm -rf /tmp/mlce/state
echo "PASS mlce-requirements"
