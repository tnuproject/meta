# M4: NetworkManager's own nmcli, from a Debian root, against meta-netd.
# Expects vtnet0 (ssh) and vtnet1 (expendable), both with DHCP.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
rm -rf /root/p4
ifconfig vtnet1 | grep -q 'inet ' || {
	sysrc -q ifconfig_vtnet1=DHCP >/dev/null
	service netif start vtnet1 >/dev/null 2>&1
	sleep 3
}
sh packages/native/build-packages.sh -b "$BIN" -o /root/p4 -V 26.0 meta-services >/dev/null
pkg info -e meta-services && pkg delete -qy meta-services
pkg add -q /root/p4/meta-services-26.0.pkg
service dbus onestatus >/dev/null 2>&1 || service dbus onestart >/dev/null
service dbus onereload >/dev/null 2>&1 || true
service meta_netd onestop >/dev/null 2>&1 || true
service meta_netd onestart >/dev/null
sleep 3
pgrep -q meta-netd || fail "meta-netd is not running"
expect_failure "a second instance is refused" 'already running' meta-netd

if meta-compat list | grep '^debian' | grep -q 'not created'; then
	service linux onestart >/dev/null 2>&1 || true
	meta-compat create debian >/root/create.log 2>&1 || { tail -5 /root/create.log; fail create; }
fi
meta-compat stop debian
meta-compat start debian
mount -p | grep -q /compat/meta/debian/run/dbus || fail "the bus is not shared"
[ -x /compat/meta/debian/usr/bin/nmcli ] ||
	meta-compat exec debian -- /usr/bin/apt-get -q -y install --no-install-recommends network-manager >/root/nm.log 2>&1 ||
	{ tail -5 /root/nm.log; fail "apt install network-manager"; }
nm() { meta-compat exec debian -- /usr/bin/nmcli "$@"; }
nm --version
echo "--- nmcli general"
nm general status
echo "--- nmcli device"
nm device status
nm -t -f DEVICE,TYPE,STATE device status >/tmp/dev.out
grep -q '^vtnet0:ethernet:connected$' /tmp/dev.out || fail "vtnet0"
grep -q '^vtnet1:ethernet:connected$' /tmp/dev.out || fail "vtnet1"
grep -q '^lo0:loopback:' /tmp/dev.out || fail "lo0"
ok "nmcli device status shows FreeBSD's interfaces"
echo "--- nmcli device show"
nm -t -f GENERAL.DEVICE,GENERAL.HWADDR,GENERAL.MTU,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS device show vtnet0 | tee /tmp/show.out
address=$(ifconfig vtnet0 | awk '/inet /{print $2; exit}')
grep -q "IP4.ADDRESS.1.:$address/24" /tmp/show.out || fail "address"
grep -q "IP4.GATEWAY:$(route -n get default | awk '/gateway/{print $2}')" /tmp/show.out || fail "gateway"
ok "addresses, gateway and name server match FreeBSD's"
nm connection show | grep -q 'Wired vtnet1' || fail "profiles"
ok "connection profiles are listed"

# Changes follow FreeBSD, whoever makes them.
ifconfig vtnet1 down
sleep 5
nm -t -f DEVICE,STATE device status | grep -q '^vtnet1:connected$' && fail "did not notice ifconfig down"
ifconfig vtnet1 up
sleep 5
nm -t -f DEVICE,STATE device status | grep -q '^vtnet1:connected$' || fail "did not notice ifconfig up"
ok "changes made with ifconfig are noticed"

nm device disconnect vtnet1
sleep 2
ifconfig vtnet1 | grep -q 'inet ' && fail "the address is still there"
nm -t -f DEVICE,STATE device status | grep -q '^vtnet1:disconnected$' || { nm device; fail "not disconnected"; }
ok "nmcli device disconnect"
nm --wait 40 device connect vtnet1 || fail "connect"
i=0
until ifconfig vtnet1 | grep -q 'inet 10.0.3'; do
	i=$((i + 1))
	[ $i -lt 30 ] || fail "no address"
	sleep 1
done
sleep 4
nm -t -f DEVICE,STATE device status | grep -q '^vtnet1:connected$' || { nm device; fail "not connected"; }
ok "nmcli device connect obtained an address by DHCP"

# An unprivileged user without an active session is refused by polkit.
if meta-compat exec debian --user tester -- /usr/bin/nmcli device disconnect vtnet1 >/tmp/deny.out 2>&1; then
	fail "an unprivileged user disconnected the network"
fi
cat /tmp/deny.out
ifconfig vtnet1 | grep -q 'inet 10.0.3' || fail "disconnected anyway"
ok "unprivileged users are refused"
meta-compat exec debian --user tester -- /usr/bin/nmcli -t -f DEVICE device status | grep -q vtnet0 || fail "cannot read"
ok "but may look"

ifconfig epair create >/dev/null
sleep 5
nm -t -f DEVICE device status | grep -q '^epair0a$' || fail "new interface"
ifconfig epair0a destroy
sleep 5
nm -t -f DEVICE device status | grep -q '^epair0a$' && fail "gone interface"
ok "interfaces appear and disappear"
pgrep -q meta-netd || fail "meta-netd died"
echo PASS m4
