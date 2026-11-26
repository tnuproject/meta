# MDCF on a real FreeBSD system: discovery, evaluation, and the life cycle of a
# driver that can be loaded and unloaded safely.
set -eu
. /root/meta/tests/vm/lib.sh

before=$(kldstat | sort)
for command in status scan hardware drivers firmware compatibility capabilities diagnose report; do
	meta-eds $command >/tmp/mdcf.out || fail "meta-eds $command"
	[ -s /tmp/mdcf.out ] || fail "meta-eds $command says nothing"
	meta-eds --json $command | jq -e . >/dev/null || fail "meta-eds --json $command"
done
[ "$(kldstat | sort)" = "$before" ] || fail "looking changed the loaded modules"
ok "looking changes nothing, and everything has a machine-readable form"
meta-eds scan | grep -q 'CONFIRMED.*ethernet-virtio' || fail "virtio is not confirmed"
ok "a device META was tested with is confirmed"
meta-eds scan | grep -q 'CONFIRMED.*usb-xhci' && fail "an untested device is confirmed"
ok "an attached but untested device is not"
[ "$(meta-eds firmware | grep -c gpu-firmware || true)" -eq 0 ] || fail "invented firmware"

meta-eds linuxkpi pci_enable_device | grep -q "declared in linux/pci.h" || fail "pci_enable_device"
meta-eds linuxkpi no_such_function_at_all >/tmp/mdcf.out 2>&1 || true
grep -q "not declared" /tmp/mdcf.out || fail "an unknown interface is known"
ok "LinuxKPI interfaces are looked up"

if meta-eds list | grep -q '^rng-virtio'; then
	if pciconf -l | grep -q 'vendor=0x1af4 device=0x1005'; then
		meta-eds plan enable rng-virtio
		kldstat -qm virtio_random && kldunload virtio_random
		sysrc -q kld_list-=virtio_random >/dev/null
		meta-eds enable rng-virtio | grep -q 'yes' || fail "no confirmation asked"
		kldstat -qm virtio_random && fail "a module was loaded without confirmation"
		expect_failure "an untested driver needs a second confirmation" 'untested' \
			meta-eds enable --yes rng-virtio
		meta-eds enable --yes --accept-untested rng-virtio
		kldstat -qm virtio_random || fail "not loaded"
		meta-eds verify rng-virtio
		ok "a driver is enabled after a plan and two confirmations"
		meta-eds disable --yes rng-virtio
		sysrc -n kld_list | grep -q virtio_random && fail "disabling left a setting behind"
		kldstat -qm virtio_random || fail "a module was unloaded"
		ok "and disabled for the next boot, without being unloaded"
	else
		echo "skipped: no VirtIO entropy device"
	fi
fi
expect_failure "an unknown component is refused" 'no-such' meta-eds plan enable no-such
echo "PASS mdcf"
