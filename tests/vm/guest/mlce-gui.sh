# MLCE: a Linux X11 program from a guest on the host's display, without acceleration.
# Expects a running Debian machine called VM with an open network.
set -eu
. /root/meta/tests/vm/lib.sh
VM=${VM:-deb}
for package in xorg-vfbserver xauth xwininfo; do
	pkg info -e $package || pkg install -qy $package </dev/null
done
guest() { su -m tester -c 'exec meta-linux vm exec "$0" -- sh -c "$1"' "$VM" "$1" </dev/null; }
guest 'sudo apt-get install -q -y --no-install-recommends xauth x11-apps' >/tmp/mlce-gui.log 2>&1 ||
	{ tail /tmp/mlce-gui.log; fail "installing x11-apps"; }

home=/tmp/mlce-gui
rm -rf $home && install -d -o tester -m 700 $home
pkill -f 'Xvfb :7' 2>/dev/null || true
cookie=$(openssl rand -hex 16)
su -m tester -c "HOME=$home XAUTHORITY=$home/.Xauthority xauth add :7 . $cookie" 2>/dev/null
su -m tester -c "exec Xvfb :7 -screen 0 800x600x24 -auth $home/.Xauthority" >/dev/null 2>&1 &
sleep 5
run() { su -m tester -c "HOME=$home XAUTHORITY=$home/.Xauthority DISPLAY=:7 exec $*" </dev/null; }
# xclock is a program of the guest; a script brings it along.
printf '#!/bin/sh\nexec xclock\n' >$home/clock
chmod 755 $home/clock
run "meta-exec run --runtime vm --instance $VM $home/clock" >/tmp/mlce-gui.log 2>&1 &&
	fail "a display was passed on without being asked for"
grep -q "Can't open display" /tmp/mlce-gui.log || { cat /tmp/mlce-gui.log; fail "unexpected error"; }
ok "a guest does not see the display unless asked to"
run "meta-exec run --x11 --runtime vm --instance $VM $home/clock" >/tmp/mlce-gui.log 2>&1 &
found=
for _ in $(seq 60); do
	run "xwininfo -root -tree" 2>/dev/null | grep -qi xclock && found=1 && break
	sleep 5
done
[ -n "$found" ] || { cat /tmp/mlce-gui.log; fail "no window"; }
run "xwininfo -root -tree" | grep -i xclock
ok "a window of a guest's program is on the host's display"
pkill -f 'meta-exec run --x11' || true
pkill -f 'Xvfb :7' || true
echo "PASS mlce-gui"
