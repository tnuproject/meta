# M1, first part: builds META's kernel from the pinned source and stages it in a
# boot environment.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
command -v git >/dev/null || pkg install -qy git-tiny </dev/null >/dev/null 2>&1
check "pinned source" sh scripts/fetch-kernel-source.sh /root/src
rm -rf /root/kstage1 /root/pk1
sh scripts/build-kernel.sh -s /root/src -o /root/obj1 >/root/kb1.log 2>&1 || { tail -5 /root/kb1.log; fail build; }
grep -a 'Kernel build for META completed' /root/kb1.log
sh packages/native/build-kernel-stage.sh -s /root/src -o /root/obj1 -d /root/kstage1 >/root/ks1.log 2>&1 || { tail -5 /root/ks1.log; fail stage; }
sh packages/native/build-packages.sh -b "$BIN" -o /root/pk1 -V 26.0 -k /root/kstage1 meta-kernel
bectl destroy -Fo meta-m1 >/dev/null 2>&1 || true
bectl create meta-m1
bectl mount meta-m1 /mnt >/dev/null
pkg -c /mnt info -e meta-kernel && pkg -c /mnt delete -qy meta-kernel
mkdir -p /mnt/var/cache/m1 && cp /root/pk1/meta-kernel-26.0.pkg /mnt/var/cache/m1/
pkg -c /mnt add -q /var/cache/m1/meta-kernel-26.0.pkg
pkg -c /mnt check -sq meta-kernel || fail "kernel package"
[ -f /mnt/boot/kernel/kernel ] || fail "FreeBSD's own kernel is gone"
bectl umount meta-m1
bectl activate -t meta-m1 >/dev/null
ok "packaged beside FreeBSD's kernel, selected for one boot"
