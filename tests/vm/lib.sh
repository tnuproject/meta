# Helpers for guest test scripts. Sourced with /bin/sh on FreeBSD.
# shellcheck shell=sh

BIN=${BIN:-/root/meta/target/release}
export PATH="$BIN:$PATH"
# The kernel built by m1, when there is one.
KERNEL_STAGE=${KERNEL_STAGE:-}
[ -n "$KERNEL_STAGE" ] || [ ! -d /root/kstage1 ] || KERNEL_STAGE=/root/kstage1

fail() {
	echo "FAIL: $*" >&2
	exit 1
}

ok() { echo "ok: $*"; }

# check DESCRIPTION COMMAND...
check() {
	description=$1
	shift
	"$@" >/tmp/check.out 2>&1 || {
		cat /tmp/check.out >&2
		fail "$description"
	}
	ok "$description"
}

# expect_failure DESCRIPTION PATTERN COMMAND...
expect_failure() {
	description=$1
	pattern=$2
	shift 2
	if "$@" >/tmp/check.out 2>&1; then
		cat /tmp/check.out >&2
		fail "$description: unexpectedly succeeded"
	fi
	grep -q "$pattern" /tmp/check.out || {
		cat /tmp/check.out >&2
		fail "$description: expected '$pattern'"
	}
	ok "$description"
}

# Creates a trust anchor with throwaway keys in /root/ota.
make_trust() (
	mkdir -p /root/ota
	cd /root/ota || exit 1
	[ ! -f release.secret ] || exit 0
	for key in root release; do
		metaupd-release keygen --out $key.secret >$key.txt 2>/dev/null
	done
	cat >root.toml <<EOF
version = 1
expires = "2099-01-01T00:00:00Z"

[roles.root]
threshold = 1
keys = ["$(sed -n 's/^public key *//p' root.txt)"]

[roles.release]
threshold = 1
keys = ["$(sed -n 's/^public key *//p' release.txt)"]
EOF
	mkdir -p /usr/local/etc/meta/trust
	metaupd-release root --config root.toml --out /usr/local/etc/meta/trust/root.json
	metaupd-release sign --role root --file /usr/local/etc/meta/trust/root.json \
		--signatures /usr/local/etc/meta/trust/root.sig.json --key-file root.secret
)

# make_release VERSION SEQUENCE PACKAGE_FILE...
make_release() {
	release_version=$1
	release_sequence=$2
	shift 2
	directory=/root/ota/releases/v$release_version
	mkdir -p "$directory"
	echo "# META $release_version" >"$directory/CHANGELOG.md"
	cat >/root/ota/release.toml <<EOF
version = "$release_version"
codename = "uglydemon"
channel = "${CHANNEL:-stable}"
sequence = $release_sequence
valid_days = 30
changelog = "CHANGELOG.md"

[compatibility]
arch = ["amd64"]
min_meta_version = "${MIN:-26.0}"
min_metaupd_version = "26.0-alpha0"
osreldate = { min = 1500000, max = 1599999 }

[[artifact]]
name = "CHANGELOG.md"
category = "documentation"
apply = "none"
EOF
	for file in "$@"; do
		cp "$file" "$directory/"
		category=meta-system
		case $file in *kernel*) category=kernel ;; esac
		cat >>/root/ota/release.toml <<EOF

[[artifact]]
name = "$(basename "$file")"
category = "$category"
apply = "pkg-in-boot-environment"
requires_reboot = true
EOF
	done
	metaupd-release manifest --config /root/ota/release.toml --artifacts "$directory"
	rm -f "$directory/meta-manifest.sig.json"
	metaupd-release sign --role release --file "$directory/meta-manifest.json" \
		--signatures "$directory/meta-manifest.sig.json" --key-file /root/ota/release.secret
}

running_be() { bectl list -H | awk -F'\t' '$2 ~ /N/ { print $1 }'; }
next_be() { bectl list -H | awk -F'\t' '$2 ~ /T/ { t = $1 } $2 ~ /R/ { r = $1 } END { print t ? t : r }'; }
permanent_be() { bectl list -H | awk -F'\t' '$2 ~ /R/ { print $1 }'; }
