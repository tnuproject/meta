# M9, fourth part: the machine came back on the previous system by itself.
set -eu
. /root/meta/tests/vm/lib.sh
case $(running_be) in meta-26.0.1-*) ok "back on 26.0.1" ;; *) fail "running $(running_be)" ;; esac
grep -q '^VERSION=26.0.1$' /etc/meta-release || fail "wrong version"
metaupd history | tail -1 | grep -q 'confirm *failed' || fail "the failure was not recorded"
metaupd status | grep -q waiting && fail "still waiting"
ok "failure recorded"
metaupd history

# Downgrade protection survived, because the state is outside the environment.
rm -rf /root/ota/releases/v26.0.2
metaupd --source /root/ota/releases check 2>&1 | grep -q withheld || fail "replay was not noticed"
ok "replay noticed"
bectl destroy -o "$(cat /root/broken-be)"
metaupd rollback default --once --yes
echo PASS m9
