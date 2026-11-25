# Installation media

Status: EXPERIMENTAL. No image is published.

[installer-iso/build-iso.sh](../../installer-iso/build-iso.sh) builds a hybrid image for BIOS
and UEFI on a FreeBSD host:

```sh
sh installer-iso/build-iso.sh -r REPOSITORIES -t TRUST -s /usr/src -o meta.iso
```

It contains a small live system that starts the installer, and a package repository with
everything an installation without a desktop needs. Desktops need a network.

The packages are exactly those the live system resolved. Following declared dependencies
alone misses packages needed only for a shared library, which `pkg fetch -d` does not see.

## Verifying a medium

The medium carries the trust anchor for updates, so everything after the first installation
depends on it. Its own packages are not checked against a signature during installation.

When images are published, their SHA-256 will be part of a signed release manifest. Verify it
with `metaupd-release verify` on another system before writing the medium. How a user without
any META system obtains the root key fingerprints safely is an open question: RESEARCH
REQUIRED.

## Evidence

`tests/vm/suite.sh m11`: boots with BIOS and UEFI, drives the interface on a serial console,
gives a wrong confirmation, retries, installs without a network, and boots the result.
Not tested from USB media or on physical machines.
