# M6: a signed package repository, used and attacked as a client would.
set -eu
. /root/meta/tests/vm/lib.sh
cd /root/meta
rm -rf /root/pkgs /root/repo /root/keys /usr/local/etc/pkg/repos/META.conf
mkdir -p /root/keys
check "packages are built" sh packages/native/build-packages.sh -b "$BIN" -o /root/pkgs -V 26.0 \
	${KERNEL_STAGE:+-k "$KERNEL_STAGE"}
pkg key --create -t eddsa /root/keys/repo.key >/dev/null
expect_failure "a key inside the repository is refused" 'refusing' \
	sh -c 'mkdir -p /root/bad && cp /root/keys/repo.key /root/bad/k && sh packages/native/build-repository.sh -p /root/pkgs -o /root/bad -k /root/bad/k'
check "the repository is built and signed" \
	sh packages/native/build-repository.sh -p /root/pkgs -o /root/repo -k /root/keys/repo.key
mkdir -p /usr/local/etc/meta/trust /usr/local/etc/pkg/repos
cp /root/repo/meta.pub /usr/local/etc/meta/trust/packages.pub
sed 's|@URL@|file:///root/repo|' packages/native/META.conf.in >/usr/local/etc/pkg/repos/META.conf
check "the signature is accepted" pkg update -f -r META
check "META packages install from it" pkg install -y -r META meta-tools
check "installed files match" pkg check -s meta-tools
[ "$(/usr/local/bin/meta version | head -1 | cut -d' ' -f1)" = META ] || fail "meta does not run"
pkg delete -qy meta-tools

# An attacker without the key.
pkg key --create -t eddsa /root/keys/other.key >/dev/null
rm -rf /root/repo2
sh packages/native/build-repository.sh -p /root/pkgs -o /root/repo2 -k /root/keys/other.key >/dev/null
sed 's|@URL@|file:///root/repo2|' packages/native/META.conf.in >/usr/local/etc/pkg/repos/META.conf
# pkg update exits with 0 here, so what counts is that nothing can be installed.
pkg update -f -r META 2>&1 | grep -q 'Invalid signature' || fail "the wrong key was not noticed"
ok "a repository signed by another key is noticed"
if pkg install -y -r META meta-tools >/dev/null 2>&1; then fail "installed from the wrong key"; fi
ok "and nothing installs from it"
sed 's|@URL@|file:///root/repo|' packages/native/META.conf.in >/usr/local/etc/pkg/repos/META.conf
pkg update -q -f -r META
package=$(find /root/repo/All -name 'meta-tools-*.pkg')
cp "$package" /root/saved.pkg
printf 'x' | dd of="$package" bs=1 seek=2000 conv=notrunc 2>/dev/null
pkg clean -qay >/dev/null 2>&1 || true
expect_failure "a modified package is refused" 'checksum\|size mismatch\|cannot\|failed' \
	pkg install -y -r META meta-tools
cp /root/saved.pkg "$package"
echo PASS m6
