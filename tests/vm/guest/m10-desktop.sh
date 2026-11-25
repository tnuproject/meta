# M10: an XFCE session shows an update and installs it through metaupd's service.
# Needs M9's keys and releases, and the packages xfce, xorg-vfbserver, xdotool,
# scrot, py312-pygobject and gtk3.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
rm -f /usr/local/etc/polkit-1/rules.d/10-meta-test.rules
version=${VERSION_UNDER_TEST:-26.0.3}
# Rising on every run, as metaupd remembers what it has seen.
sequence=${SEQUENCE:-$(date +%s)}
(make_trust)
rm -rf /root/p10 /root/p10new /root/ota/releases/v$version /var/db/metaupd/staging
sh packages/native/build-packages.sh -b "$BIN" -o /root/p10 -V 26.0 meta-services meta-update-manager >/dev/null
for p in meta-services meta-update-manager; do
	pkg info -e $p && pkg delete -qy $p
	pkg add -q /root/p10/$p-26.0.pkg
done
pkg info -e meta-tools meta-release || fail "run M9 first"
sh packages/native/build-packages.sh -b "$BIN" -o /root/p10new -V "$version" meta-tools meta-release >/dev/null
make_release "$version" "$sequence" /root/p10new/*.pkg
service dbus onestatus >/dev/null 2>&1 || service dbus onestart >/dev/null
service dbus onereload >/dev/null 2>&1 || true
sleep 1
service metaupdd onestop >/dev/null 2>&1 || true
sysrc -q metaupdd_flags="--source /root/ota/releases" >/dev/null
service metaupdd onestart >/dev/null
sleep 2
bus() { gdbus call --system --dest io.github.tnuproject.MetaUpd1 --object-path /io/github/tnuproject/MetaUpd1 --method "$@"; }
gdbus introspect --system --dest io.github.tnuproject.MetaUpd1 --object-path /io/github/tnuproject/MetaUpd1 --xml >/tmp/live.xml
# The published interface definition is what the service really offers.
python3 - <<'PY' || fail "the interface differs from its definition"
import xml.etree.ElementTree as e
def names(path):
    out = set()
    for i in e.parse(path).getroot().findall("interface"):
        if i.get("name") == "io.github.tnuproject.MetaUpd1":
            for c in i:
                out.add((c.tag, c.get("name"), "".join(a.get("type") for a in c.findall("arg")) or c.get("type")))
    return out
a, b = names("/usr/local/share/dbus-1/interfaces/io.github.tnuproject.MetaUpd1.xml"), names("/tmp/live.xml")
print("only defined:", a - b, "only live:", b - a) if a != b else None
raise SystemExit(a != b)
PY
ok "the service matches its published interface"

user() { su -m tester -c "$*"; }
if user "gdbus call --system --dest io.github.tnuproject.MetaUpd1 --object-path /io/github/tnuproject/MetaUpd1 --method io.github.tnuproject.MetaUpd1.Install $version" >/tmp/out 2>&1; then
	fail "an unprivileged user installed an update"
fi
grep -q NotAuthorized /tmp/out || { cat /tmp/out; fail "unexpected error"; }
ok "unprivileged users are refused by polkit"

# In a test there is nobody to type a password: allow this one user.
cat >/usr/local/etc/polkit-1/rules.d/10-meta-test.rules <<'RULE'
polkit.addRule(function(action, subject) {
    if (action.id.indexOf("io.github.tnuproject.metaupd.") == 0 && subject.user == "tester") return polkit.Result.YES;
});
RULE
service polkitd onerestart >/dev/null 2>&1 || pkill polkitd || true
sleep 2

pkill Xvfb xfce4-session 2>/dev/null || true
sleep 1
mkdir -p /home/tester && chown tester /home/tester
cat >/home/tester/session.sh <<'SESSION'
export DISPLAY=:7 HOME=/home/tester XDG_RUNTIME_DIR=/tmp/xdg-tester LC_ALL=C.UTF-8
mkdir -p -m 700 $XDG_RUNTIME_DIR
exec dbus-run-session -- xfce4-session
SESSION
(user "Xvfb :7 -screen 0 1280x800x24 -nolisten tcp" >/tmp/xvfb.log 2>&1 &)
sleep 3
(user "sh /home/tester/session.sh" >/tmp/xfce.log 2>&1 &)
i=0
until user "DISPLAY=:7 xdotool search --class xfce4-panel" >/dev/null 2>&1; do
	i=$((i + 1))
	[ $i -lt 60 ] || { tail -5 /tmp/xfce.log; fail "XFCE did not start"; }
	sleep 1
done
sleep 6
ok "an XFCE session is running"
shot() {
	rm -f /tmp/shot.png
	user "DISPLAY=:7 scrot /tmp/shot.png" && mv /tmp/shot.png "/root/$1.png"
}
x() { user "DISPLAY=:7 HOME=/home/tester xdotool $*"; }
(user "DISPLAY=:7 HOME=/home/tester XDG_RUNTIME_DIR=/tmp/xdg-tester LC_ALL=C.UTF-8 meta-update-manager" >/tmp/mum.log 2>&1 &)
i=0
until x "search --name META.Update.Manager" >/tmp/win 2>/dev/null && [ -s /tmp/win ]; do
	i=$((i + 1))
	[ $i -lt 40 ] || { cat /tmp/mum.log; fail "the Update Manager did not start"; }
	sleep 1
done
sleep 6
shot m10-available
bus io.github.tnuproject.MetaUpd1.GetAvailable | tee /tmp/avail
grep -q "$version" /tmp/avail || fail "the update was not found"
ok "the Update Manager checked and found META $version"
window=$(tail -1 /tmp/win)
x "windowactivate --sync $window" || true
before=$(bectl list -H | wc -l)
# Alt+I is the Install button.
x "key --window $window alt+i" || true
x "key alt+i"
i=0
until metaupd status | grep -q "waiting *META $version"; do
	i=$((i + 1))
	[ $i -lt 120 ] || { shot m10-failed; metaupd history | tail -3; cat /tmp/mum.log; fail "not installed"; }
	sleep 1
done
sleep 3
shot m10-installed
[ "$(bectl list -H | wc -l)" -gt "$before" ] || fail "no boot environment"
gdbus call --system --dest io.github.tnuproject.MetaUpd1 --object-path /io/github/tnuproject/MetaUpd1 \
	--method org.freedesktop.DBus.Properties.Get io.github.tnuproject.MetaUpd1 State | grep -q RebootRequired || fail "state"
ok "the Update Manager installed META $version"
case $(next_be) in meta-$version-*) ok "activated for one boot" ;; *) fail "$(next_be)" ;; esac
rm -f /usr/local/etc/polkit-1/rules.d/10-meta-test.rules
pkill -u tester || true
echo PASS m10
