# M5: installs META onto the second virtual disk.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
disk=vtbd1
zpool list -H -o name | grep -qx metapool && zpool export -f metapool
rm -rf /root/p5 /root/repo5 /root/repos5 /root/trust5 /mnt5
mkdir -p /root/repos5 /root/trust5 /mnt5 /root/keys
sh packages/native/build-packages.sh -b "$BIN" -o /root/p5 -V 26.0 meta-tools meta-release meta-installer >/dev/null
[ -f /root/keys/repo.key ] || pkg key --create -t eddsa /root/keys/repo.key >/dev/null
sh packages/native/build-repository.sh -p /root/p5 -o /root/repo5 -k /root/keys/repo.key >/dev/null
(make_trust)
mkdir -p /usr/local/etc/meta/trust
cp /root/repo5/meta.pub /usr/local/etc/meta/trust/packages.pub
cp /usr/local/etc/meta/trust/* /root/trust5/
sed 's|@URL@|file:///root/repo5|' packages/native/META.conf.in >/root/repos5/META.conf
# FreeBSD's own repositories, with the base system enabled.
sed '/FreeBSD-base/,/^}/s/enabled: no/enabled: yes/' /etc/pkg/FreeBSD.conf >/root/repos5/FreeBSD.conf

cat >/root/answers.toml <<EOF
mode = "custom"
hostname = "metabox"
timezone = "Europe/Rome"
keymap = "it"
language = "it_IT.UTF-8"
disk = "$disk"
swap_gib = 1
[user]
name = "alice"
full_name = "Alice Example"
sudo = true
EOF
meta-installer --answers /root/answers.toml >/root/plan.json
grep -q '"executable": true' /root/plan.json || fail "the plan is not executable"
run() { meta-install-run --plan /root/plan.json --repositories /root/repos5 --trust /root/trust5 --target /mnt5 --serial-console --password-stdin "$@"; }

echo secret | expect_failure "wrong confirmation" 'does not match' run --pool metapool --confirm vtbd0
echo secret | expect_failure "existing pool name" 'already exists' run --pool zroot --confirm $disk
: | expect_failure "missing password" 'needs a password' run --pool metapool --confirm $disk
sed "s/\"$disk\"/\"vtbd0\"/" /root/plan.json >/root/plan0.json
echo secret | expect_failure "the running system's disk" 'in use' \
	meta-install-run --plan /root/plan0.json --repositories /root/repos5 --target /mnt5 --password-stdin --pool metapool --confirm vtbd0
gpart show $disk >/dev/null 2>&1 && before=$(gpart show $disk) || before=none
[ "$before" = none ] || echo "note: $disk already had partitions"

echo secret | run --pool metapool --confirm $disk >/root/install.log 2>&1 || { tail -20 /root/install.log; fail install; }
grep '^==>\|is installed' /root/install.log
zpool list -H -o name | grep -qx metapool && fail "the pool was not exported"
mount -p | grep mnt5 && fail "still mounted"
ok "installed and released the disk"
gpart show -l $disk

# Look inside without booting.
zpool import -N -R /mnt5 metapool && zfs mount metapool/ROOT/default && zfs mount -a
grep -q secret /mnt5/etc/master.passwd && fail "plain text password"
grep '^alice:\$6\$' /mnt5/etc/master.passwd >/dev/null || fail "no password hash"
grep '^root:\*' /mnt5/etc/master.passwd >/dev/null || fail "root is not locked"
grep -q 'alice' /mnt5/etc/group || fail "not in wheel"
grep -rq secret /root/plan.json /mnt5/usr/local/etc /mnt5/var/db/zoneinfo && fail "password leaked"
[ "$(zfs get -H -o value mountpoint metapool/meta/state)" = /mnt5/var/db/metaupd ] || fail "no state dataset"
ok "password hashed, root locked, state outside the boot environment"
pkg -r /mnt5 check -sq meta-tools meta-release sudo || fail "packages damaged"
zpool export metapool
sync
echo PASS m5
