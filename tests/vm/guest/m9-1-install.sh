# M9, first part: a good release with a META kernel is applied.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
zfs list zroot/metaupd >/dev/null 2>&1 || {
	make_trust
	sh packages/native/build-packages.sh -b "$BIN" -o /root/p0 -V 26.0 meta-tools meta-release >/dev/null
	pkg add -q /root/p0/*.pkg
	make_release 26.0.1 2 /root/p0/meta-release-26.0.pkg
	expect_failure "state inside the boot environment is refused" 'inside the boot environment' \
		sh -c 'metaupd --source /root/ota/releases download && metaupd install --yes 26.0.1'
	rm -rf /root/ota/releases /var/db/metaupd
	zfs create -o mountpoint=/var/db/metaupd zroot/metaupd
}
sh packages/native/build-packages.sh -b "$BIN" -o /root/p1 -V 26.0.1 ${KERNEL_STAGE:+-k "$KERNEL_STAGE"} \
	meta-tools meta-release ${KERNEL_STAGE:+meta-kernel} >/dev/null
make_release 26.0.1 2 /root/p1/*.pkg
before=$(bectl list -H | wc -l)
check "release found" metaupd --source /root/ota/releases check
check "release downloaded" metaupd --source /root/ota/releases download
check "dry run changes nothing" metaupd install --dry-run 26.0.1
[ "$(bectl list -H | wc -l)" = "$before" ] || fail "dry run created a boot environment"

# A staged artifact changed after the download must not be installed.
staged=$(find /var/db/metaupd/staging/26.0.1 -name 'meta-release-*.pkg')
cp "$staged" /root/keep.pkg
printf 'x' | dd of="$staged" bs=1 seek=100 conv=notrunc 2>/dev/null
expect_failure "modified staged package is refused" 'SHA-256\|bytes, expected' metaupd install --yes 26.0.1
cp /root/keep.pkg "$staged"
[ "$(bectl list -H | wc -l)" = "$before" ] || fail "a boot environment was left behind"

check "installed into a new boot environment" metaupd install --yes 26.0.1
grep -q '^VERSION=26.0$' /etc/meta-release || fail "the running system was modified"
[ "$(permanent_be)" = "$(running_be)" ] || fail "the new environment is already permanent"
case $(next_be) in meta-26.0.1-*) ok "activated for one boot" ;; *) fail "not activated: $(next_be)" ;; esac
mount -p | grep -q /var/db/metaupd/mnt && fail "the clone is still mounted"
expect_failure "a second install waits" 'waiting for a reboot' metaupd install --yes 26.0.1
metaupd status
