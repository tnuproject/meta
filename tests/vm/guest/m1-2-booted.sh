# M1, second part: the kernel booted.
set -eu
. /root/meta/tests/vm/lib.sh
[ "$(running_be)" = meta-m1 ] || fail "running $(running_be)"
uname -v | grep -q ' META$' || fail "not the META kernel: $(uname -v)"
[ "$(sysctl -n kern.bootfile)" = /boot/kernel.META/kernel ] || fail "$(sysctl -n kern.bootfile)"
kldload -n linux64 && kldstat -qm linux64elf || fail "modules do not match the kernel"
kldstat | grep -q zfs || fail "no ZFS"
dmesg | grep -i 'panic\|fatal trap' && fail "kernel errors"
ok "$(uname -v)"
echo PASS m1
