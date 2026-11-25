# M9, last part: deliberate rollback to the original system.
set -eu
. /root/meta/tests/vm/lib.sh
[ "$(running_be)" = default ] || fail "running $(running_be)"
grep -q '^VERSION=26.0$' /etc/meta-release || fail "wrong version"
metaupd history | grep -q rollback || fail "history was lost in the rollback"
ok "rolled back to 26.0 with history intact"
