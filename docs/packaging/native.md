# Native packages

Status: EXPERIMENTAL. No repository is published.

pkg is authoritative for the host, and FreeBSD's repositories are used unchanged. META adds
packages; it does not fork the ports tree.

| Package | Contents |
|---|---|
| `meta-tools` | Command line tools and the boot confirmation service |
| `meta-release` | `/etc/meta-release`, the installed version |
| `meta-kernel` | The META kernel beside FreeBSD's, as `/boot/kernel.META` |
| `meta-installer` | Interface and installer |
| `meta-services` | metaupdd and meta-netd |
| `meta-update-manager` | Graphical front end |

Services are off until enabled; the installer enables them with a desktop.

## Building

On FreeBSD:

```sh
cargo build --workspace --release --locked
sh packages/native/build-packages.sh -b target/release -o dist -V 26.0-alpha1
pkg key --create -t eddsa /secure/place/repository.key
sh packages/native/build-repository.sh -p dist -o repository -k /secure/place/repository.key
```

The key must not be inside the repository directory; the script refuses that.

## Versions

pkg orders `26.0.a1 < 26.0`. META versions map as `26.0-alpha1` to `26.0.a1`, `beta` to `b`
and `rc` to `r`.

## Repository

Signed with EdDSA. Clients use [META.conf.in](../../packages/native/META.conf.in) with the
public key from their installation medium. It has a lower priority than FreeBSD's, so it
cannot replace FreeBSD packages.

`tests/vm/suite.sh m6` installs from it, then tries a repository signed by another key and a
modified package. Both are refused.

`pkg update` exits with 0 even when a repository's signature is invalid; it removes the
repository instead. Scripts must not rely on its exit status.

## Overrides

See [force policy](force-policy.md).
