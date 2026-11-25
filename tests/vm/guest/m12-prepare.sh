# M12: a release candidate with a desktop on the second disk, and a signed update
# for it. Rehearsal only: throwaway keys, virtual hardware.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
disk=vtbd1
zpool list -H -o name | grep -qx metapool && zpool export -f metapool
rm -rf /root/p12 /root/p12new /root/repo12 /root/repos12 /root/trust12 /mnt12 /root/ota/releases
mkdir -p /root/repos12 /root/trust12 /mnt12 /root/keys
all="meta-tools meta-release meta-installer meta-services meta-update-manager"
# shellcheck disable=SC2086
sh packages/native/build-packages.sh -b "$BIN" -o /root/p12 -V 26.0-rc1 $all >/dev/null
# shellcheck disable=SC2086
sh packages/native/build-packages.sh -b "$BIN" -o /root/p12new -V 26.0-rc2 -k /root/kstage1 $all meta-kernel >/dev/null
[ -f /root/keys/repo.key ] || pkg key --create -t eddsa /root/keys/repo.key >/dev/null
sh packages/native/build-repository.sh -p /root/p12 -o /root/repo12 -k /root/keys/repo.key >/dev/null
(make_trust)
cp /root/repo12/meta.pub /usr/local/etc/meta/trust/packages.pub
cp /usr/local/etc/meta/trust/* /root/trust12/
sed 's|@URL@|file:///root/repo12|' packages/native/META.conf.in >/root/repos12/META.conf
sed '/FreeBSD-base/,/^}/s/enabled: no/enabled: yes/' /etc/pkg/FreeBSD.conf >/root/repos12/FreeBSD.conf
CHANNEL=testing MIN=26.0-rc1 make_release 26.0-rc2 1 /root/p12new/*.pkg
cat >/root/answers12.toml <<EOF
hostname = "metabox"
disk = "$disk"
swap_gib = 1
desktop = "xfce"
[user]
name = "alice"
full_name = "Alice Example"
sudo = true
EOF
meta-installer --answers /root/answers12.toml >/root/plan12.json
echo secret | meta-install-run --plan /root/plan12.json --repositories /root/repos12 --trust /root/trust12 \
	--target /mnt12 --serial-console --password-stdin --pool metapool --confirm $disk >/root/install12.log 2>&1 ||
	{ tail -15 /root/install12.log; fail install; }
ok "installed with XFCE"
# Stands in for the GitHub release, which does not exist.
zpool import -N -R /mnt12 metapool && zfs mount metapool/ROOT/default && zfs mount -a
cp -R /root/ota/releases /mnt12/home/releases
for service in metaupdd meta_netd lightdm dbus; do
	grep -q "^${service}_enable=\"YES\"" /mnt12/etc/rc.conf || fail "$service is not enabled"
done
zpool export metapool
sync
echo PASS m12-prepare
