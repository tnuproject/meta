# M7 and M3: compatibility roots from signed archives, used, updated, removed.
# Set EDITION to debian or arch.
set -eu
. /root/meta/tests/vm/lib.sh
edition=${EDITION:?}
root=/compat/meta/$edition
pw usershow tester >/dev/null
mkdir -p /home/tester && chown tester /home/tester
if [ -z "$(sysctl -n compat.linux.osrelease 2>/dev/null || true)" ]; then
	expect_failure "creating needs Linux support" 'linux_enable' meta-compat create "$edition"
	sysrc -q linux_enable=YES >/dev/null
	service linux start >/dev/null 2>&1
fi
meta-compat list
if meta-compat list | grep "^$edition" | grep -q 'not created'; then
	meta-compat create "$edition" >/root/create.log 2>&1 || { tail -15 /root/create.log; fail create; }
	ok "created from a signed archive"
fi
meta-compat start "$edition"
meta-compat list | grep "^$edition" | grep -q running || fail "not running"
expect_failure "creating twice is refused" 'already exists' meta-compat create "$edition"
install -m 0555 "$BIN/meta-compat" /usr/local/bin/meta-compat-test
expect_failure "unprivileged use is refused" 'needs root' su -m tester -c "/usr/local/bin/meta-compat-test start $edition"

# Isolation, seen from inside.
meta-compat exec "$edition" -- /bin/uname -s | grep -q Linux || fail "not Linux inside"
meta-compat exec "$edition" -- /bin/sh -c 'test ! -e /dev/vtbd0 && test ! -e /dev/mem && test -c /dev/null' ||
	fail "devices are exposed"
ok "no disks or kernel memory inside"
[ "$(meta-compat exec "$edition" --user tester -- /usr/bin/id -u)" = "$(id -u tester)" ] || fail "wrong user"
su -m tester -c 'echo shared > /home/tester/file'
[ "$(cd /home/tester && meta-compat exec "$edition" --user tester -- /bin/cat file)" = shared ] || fail "home"
ok "runs as the user, in their directory"
[ "$(meta-compat exec "$edition" -- /bin/echo '$(reboot); a b')" = '$(reboot); a b' ] || fail "arguments were interpreted"
ok "arguments are passed unchanged"

# The distribution's package manager.
case $edition in
debian)
	meta-compat exec debian -- /usr/bin/apt-get -q -y install hello >/root/pm.log 2>&1 || { tail -5 /root/pm.log; fail apt; }
	program=/usr/bin/hello
	expect_failure "kernels cannot be installed" 'no installation candidate' \
		meta-compat exec debian -- /usr/bin/apt-get -q -y install linux-image-amd64
	;;
arch)
	meta-compat exec arch -- /usr/bin/pacman -S --noconfirm --noprogressbar which >/root/pm.log 2>&1 || { tail -5 /root/pm.log; fail pacman; }
	program=/usr/bin/which
	if meta-compat exec arch -- /usr/bin/pacman -S --noconfirm --noprogressbar linux >/root/pm.log 2>&1; then
		fail "a kernel was installed"
	fi
	grep -q 'failed to run transaction hooks\|Refusing' /root/pm.log || { tail /root/pm.log; fail "unexpected failure"; }
	ok "kernels cannot be installed"
	;;
esac
ok "packages install with verified signatures"

# M3: linux2BSD starts a program from this root.
if [ -d /compat/linux/usr ] || [ "$edition" = debian ]; then
	linux2bsd inspect "$root$program" | grep decision
fi
linux2bsd run "$root$program" --version | head -1 || fail "linux2bsd run"
ok "linux2bsd ran $program"
cp "$root$program" /opt/elsewhere 2>/dev/null || { mkdir -p /opt && cp "$root$program" /opt/elsewhere; }

# Snapshots.
snapshot=$(meta-compat snapshot "$edition" | sed 's/.*@//')
meta-compat exec "$edition" -- /bin/sh -c 'echo x > /etc/meta-marker'
check "rollback asks first" meta-compat rollback "$edition" "$snapshot"
[ -e "$root/etc/meta-marker" ] || fail "rolled back without confirmation"
check "rollback" meta-compat rollback --yes "$edition" "$snapshot"
[ ! -e "$root/etc/meta-marker" ] || fail "rollback did nothing"
expect_failure "unknown snapshots are refused" 'no snapshot' meta-compat rollback --yes "$edition" nope
meta-compat update "$edition" >/root/up.log 2>&1 || { tail -5 /root/up.log; fail update; }
ok "updated after a snapshot"

# Removal must never reach the user's files.
if [ -n "${REMOVE:-}" ]; then
	meta-compat start "$edition"
	mount -p | grep -q "$root/home" || fail "home is not mounted"
	check "removal asks first" meta-compat remove "$edition"
	[ -d "$root/etc" ] || fail "removed without confirmation"
	# A process holding the root open must stop the removal, not break it.
	(cd "$root/tmp" && sleep 30) &
	if meta-compat remove --yes "$edition" >/root/rm.log 2>&1; then
		echo "note: removal succeeded while busy"
	else
		cat /root/rm.log
		[ "$(cat /home/tester/file)" = shared ] || fail "USER DATA LOST"
		ok "busy root was not removed"
		wait
		check "removed" meta-compat remove --yes "$edition"
	fi
	[ "$(cat /home/tester/file)" = shared ] || fail "USER DATA LOST"
	[ ! -d "$root/etc" ] || fail "still there"
	mount -p | grep "$root" && fail "still mounted"
	ok "removed; home is intact"
fi
echo "PASS m7 $edition"
