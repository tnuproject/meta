# Installation media

Status: PLANNED. No image exists and nothing here builds one.

## Approach

Build on FreeBSD's `release(7)` scripts rather than a new image builder: a base system from
pkgbase, the META packages, and `meta-installer` started from `rc.local` in place of
`bsdinstall`.

Needed first: an installer that carries out plans (M5), a package repository (M6) and
published signing keys (M9).

Exit criterion for M11: the image boots into the installer in QEMU under BIOS and UEFI.
