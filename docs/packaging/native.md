# Native packages

Status: EXPERIMENTAL.

pkg is authoritative for the host, and FreeBSD's repositories are used unchanged. META adds
packages; it does not fork the ports tree.

## Building

On FreeBSD:

```sh
cargo build --workspace --release --locked
sh packages/native/build-pkg.sh -b target/release -o dist
pkg info -F dist/meta-tools-26.0.a0.pkg
```

Built, installed, checked with `pkg check -s` and removed on FreeBSD 15.1-RELEASE-p7,
2026-11-25.

## Versions

pkg orders `26.0.a0 < 26.0.a1 < 26.0`, which was confirmed with `pkg version -t`. META
versions map as `26.0-alpha1` to `26.0.a1`, `beta` to `b` and `rc` to `r`.

## Repository

PLANNED. A META repository would be signed with a key of its own, be enabled in addition to
FreeBSD's, and have a lower priority so it cannot replace FreeBSD packages. `os-release`
would carry META's identity through a package that owns that file.

RESEARCH REQUIRED: FreeBSD's base packages own `/etc/os-release` generation, and META must
not fight them over it.

## Overrides

See [force policy](force-policy.md).
