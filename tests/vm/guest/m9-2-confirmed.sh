# M9, second part: the machine booted the update and confirmed it by itself.
set -eu
. /root/meta/tests/vm/lib.sh
case $(running_be) in meta-26.0.1-*) ok "running the new environment" ;; *) fail "running $(running_be)" ;; esac
[ "$(permanent_be)" = "$(running_be)" ] || fail "the update was not confirmed"
ok "confirmed at boot by rc.d/metaupd_confirm"
grep -q '^VERSION=26.0.1$' /etc/meta-release || fail "wrong version"
[ -z "${KERNEL_STAGE:-}" ] || uname -v | grep -q META || fail "not the META kernel: $(uname -v)"
metaupd status | grep -q waiting && fail "still waiting"
metaupd --source /root/ota/releases check | grep -q 'No update' || fail "offers the installed version"
metaupd history
