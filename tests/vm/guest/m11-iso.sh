# M11: builds the installation image. Needs M5's repositories.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
[ -d /root/repos5 ] && [ -d /root/trust5 ] || fail "run m5-install.sh first"
# The image is made with FreeBSD's release scripts, from the pinned source that m1
# also uses; a machine without m1 fetches it here.
src=/usr/src
[ -f $src/release/amd64/mkisoimages.sh ] || {
	src=/root/src
	sh scripts/fetch-kernel-source.sh $src >/dev/null
}
sh packages/native/build-packages.sh -b "$BIN" -o /root/p5 -V 26.0 meta-tools meta-release meta-installer >/dev/null
sh packages/native/build-repository.sh -p /root/p5 -o /root/repo5 -k /root/keys/repo.key >/dev/null
rm -f /root/meta.iso
expect_failure "an incomplete trust directory is refused" 'is missing' \
	sh installer-iso/build-iso.sh -r /root/repos5 -t /root/keys -s $src -o /root/meta.iso
sh installer-iso/build-iso.sh -r /root/repos5 -t /root/trust5 -s $src -o /root/meta.iso ${SERIAL:+-S} >/root/iso.log 2>&1 ||
	{ tail -20 /root/iso.log; fail "build-iso"; }
tail -2 /root/iso.log
file /root/meta.iso
echo PASS m11-build
