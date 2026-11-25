#!/bin/sh
# End-to-end test of the OTA metadata pipeline: key generation, root metadata,
# manifest, signing, verification, and the client's check, download and
# installation plan. Uses throwaway keys and a temporary directory.
#
# Usage: tests/ota-e2e.sh [directory containing metaupd and metaupd-release]
set -eu

bin=${1:-target/debug}
bin=$(cd "$bin" && pwd)
repository=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/meta-ota.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
umask 022
cd "$work"

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

# expect_failure DESCRIPTION PATTERN COMMAND...
expect_failure() {
	description=$1
	pattern=$2
	shift 2
	if "$@" >out.txt 2>&1; then
		cat out.txt >&2
		fail "$description: unexpectedly succeeded"
	fi
	grep -q "$pattern" out.txt || {
		cat out.txt >&2
		fail "$description: expected '$pattern'"
	}
	echo "ok: $description"
}

client() {
	"$bin/metaupd" --state-dir "$work/state" --trust-dir "$work/trust" --source "$work/releases" "$@"
}

# Keys and root metadata
for key in root release stranger; do
	"$bin/metaupd-release" keygen --out "$key.secret" >"$key.txt" 2>/dev/null
done
public() { sed -n 's/^public key *//p' "$1.txt"; }
cat >root.toml <<EOF
version = 1
expires = "2099-01-01T00:00:00Z"

[roles.root]
threshold = 1
keys = ["$(public root)"]

[roles.release]
threshold = 1
keys = ["$(public release)"]
EOF
mkdir -p trust releases
"$bin/metaupd-release" root --config root.toml --out trust/root.json
"$bin/metaupd-release" sign --role root --file trust/root.json \
	--signatures trust/root.sig.json --key-file root.secret

# A release
release=releases/v26.0.1
mkdir "$release"
printf 'example package\n' >"$release/meta-example.pkg"
printf '# META 26.0.1\n' >"$release/CHANGELOG.md"
cat >release.toml <<EOF
version = "26.0.1"
codename = "uglydemon"
channel = "stable"
sequence = 2
valid_days = 30
changelog = "CHANGELOG.md"

[compatibility]
arch = ["amd64", "aarch64", "x86_64"]
min_meta_version = "26.0"
min_metaupd_version = "26.0-alpha0"
osreldate = { min = 1500000, max = 1599999 }

[[artifact]]
name = "meta-example.pkg"
category = "meta-system"
apply = "pkg-in-boot-environment"

[[artifact]]
name = "CHANGELOG.md"
category = "documentation"
apply = "none"
EOF
"$bin/metaupd-release" manifest --config release.toml --artifacts "$release"
"$bin/metaupd-release" sign --role release --file "$release/meta-manifest.json" \
	--signatures "$release/meta-manifest.sig.json" --key-file release.secret
"$bin/metaupd-release" checksums --dir "$release"
(cd "$release" && if command -v sha256sum >/dev/null; then sha256sum -c SHA256SUMS; else shasum -a 256 -c SHA256SUMS; fi) >/dev/null
verify() {
	"$bin/metaupd-release" verify --root trust/root.json --root-signatures trust/root.sig.json --dir "$release"
}
verify

# The client
client check | grep -q 'META 26.0.1' || fail "check did not offer the release"
client download | grep -q 'META 26.0.1 verified' || fail "download failed"
client install 26.0.1 --dry-run >out.txt 2>&1 || true
grep -q 'every staged artifact verified' out.txt || fail "staged release did not verify"
expect_failure "installation is refused, not simulated" \
	'cannot be installed here\|not implemented' client install 26.0.1
client history | grep -q 'download *ok' || fail "history has no download"
echo "ok: check, download, plan and history"

# Attacks
cp -R releases pristine
expect_failure "key inside a Git tree is refused" 'Git working tree' \
	sh -c "mkdir -p repo/.git && '$bin/metaupd-release' keygen --out repo/key"
"$bin/metaupd-release" sign --role root --file trust/root.json --signatures x.json --key-file root.secret
expect_failure "signing a manifest as root metadata is refused" 'malformed root metadata' \
	"$bin/metaupd-release" sign --role root --file "$release/meta-manifest.json" \
	--signatures y.json --key-file root.secret

printf 'tampered package\n' >"$release/meta-example.pkg"
expect_failure "tampered artifact" 'expected' verify
rm -rf releases && cp -R pristine releases

rm "$release/meta-manifest.sig.json"
"$bin/metaupd-release" sign --role release --file "$release/meta-manifest.json" \
	--signatures "$release/meta-manifest.sig.json" --key-file stranger.secret
expect_failure "untrusted signer" 'threshold not met' verify
client check 2>&1 | grep -q 'rejected v26.0.1' || fail "client accepted an untrusted signer"
rm -rf releases && cp -R pristine releases

printf 'extra\n' >"$release/unlisted.pkg"
expect_failure "unlisted file" 'not covered by the manifest' verify
rm -rf releases && cp -R pristine releases

printf '\n' >>trust/root.json
expect_failure "modified trust anchor" 'threshold not met' client check

# The example release description stays valid
example=$repository/metaupd/release/release.toml.example
mkdir example
sed -n '/^\[\[artifact\]\]/,$ s/^name = "\(.*\)"$/\1/p' "$example" | while read -r name; do
	echo "$name" >"example/$name"
done
"$bin/metaupd-release" manifest --config "$example" --artifacts example
echo "ok: example release description"

echo "PASS"
