#!/bin/sh
# Builds a signed pkg repository from META packages.
#
# Usage: packages/native/build-repository.sh -p PACKAGES -o REPOSITORY -k KEY
#
# KEY is an EdDSA key made with `pkg key --create -t eddsa`. It must not be in the
# repository directory.
set -eu

packages=
output=
key=
while getopts p:o:k: option; do
	case $option in
	p) packages=$OPTARG ;;
	o) output=$OPTARG ;;
	k) key=$OPTARG ;;
	*) exit 64 ;;
	esac
done
[ -n "$packages" ] && [ -n "$output" ] && [ -n "$key" ] || {
	echo "usage: $0 -p packages -o repository -k key" >&2
	exit 64
}
[ -f "$key" ] || {
	echo "build-repository: $key does not exist" >&2
	exit 66
}
mkdir -p "$output/All"
case $(cd "$(dirname "$key")" && pwd)/ in "$(cd "$output" && pwd)"/*)
	echo "build-repository: refusing to keep the key inside the repository" >&2
	exit 73
	;;
esac
cp "$packages"/*.pkg "$output/All/"
pkg repo -q "$output" "eddsa:$key"
pkg key --public -t eddsa "$key" >"$output/meta.pub"
echo "$output"
