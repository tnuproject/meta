# M9, third part: an update that cannot boot.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
sh packages/native/build-packages.sh -b "$BIN" -o /root/p2 -V 26.0.2 meta-tools meta-release >/dev/null
make_release 26.0.2 3 /root/p2/*.pkg
metaupd --source /root/ota/releases download >/dev/null
check "installed" metaupd install --yes 26.0.2
be=$(next_be)
# Sabotage after validation: without init the kernel panics and resets.
bectl mount "$be" /mnt
chflags noschg /mnt/sbin/init
mv /mnt/sbin/init /mnt/sbin/init.gone
rm -rf /mnt/rescue/init /mnt/sbin/oinit /mnt/sbin/init.bak
bectl umount "$be"
echo "$be" >/root/broken-be
ok "sabotaged $be"
