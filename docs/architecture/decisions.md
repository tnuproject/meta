# Design decisions

Each entry records a choice, why it was made, and what would change it.

## 1. FreeBSD subsystems are kept

META adds userspace. The scheduler, memory management, VFS, network stack and driver
framework remain FreeBSD's. Replacing one needs a proposal that shows a META feature
cannot be built otherwise.

## 2. Base on FreeBSD 15

FreeBSD 15 installs the base system as packages (pkgbase), which is what lets metaupd
update kernel, base and META components with one mechanism inside a boot environment.

Development uses 15.1-RELEASE. Support for it ends on 31 March 2027; 15.2 is
scheduled for 8 December 2026 and supported until 30 September 2027
([schedule](https://www.freebsd.org/releases/15.2R/schedule/)). Each point release is
supported for about three months after the next one, so META must be able to follow
every point release. stable/15 is supported until 31 December 2029.

FreeBSD's handbook still calls pkgbase major upgrades experimental. That is a risk for
META 27, not for 26.

## 3. FreeBSD's loader is the default boot loader

The plan names GRUB subject to validation. Three findings speak against making it the
default now:

- FreeBSD's `loader.efi` understands ZFS boot environments, including the one-time
  activation metaupd depends on. This was tested.
- The GRUB package for FreeBSD is 2.02 (`grub2-efi-2.02_21`, checked 2026-11-25), a
  2017 upstream release that predates the fixes for the 2020 BootHole
  vulnerabilities. Whether the port carries them as patches was not checked.
- GRUB reads ZFS with its own code, which lags behind OpenZFS pool features.

If GRUB is offered, it should chain-load `loader.efi` rather than load the kernel.
RESEARCH REQUIRED: Secure Boot, dual boot, and whether a maintained GRUB can be
packaged.

## 4. Linux userlands are separate roots

APT and Pacman run in `/compat/meta/debian` and `/compat/meta/arch`. They never install
into the host. See [editions](../packaging/editions.md).

## 5. linux2BSD does not translate anything

Linux programs for the same CPU already contain compatible instructions, and
Linuxulator already implements Linux system calls. linux2BSD chooses, checks and
explains.

## 6. Update trust does not depend on GitHub

GitHub Releases is transport. Trust comes from META's Ed25519 keys in versioned root
metadata, following ideas from [The Update Framework](https://theupdateframework.io/)
with fewer roles. The full TUF specification and its Rust implementation were
considered; the reduced design was chosen to keep the privileged code small. It lacks
separate snapshot and timestamp roles, so freeze protection relies on manifest expiry.
See [threat model](../security/threat-model.md).

This decision should be reviewed by someone with TUF experience before the first
release.

## 7. Signatures cover the exact bytes

Detached signatures are made over the file as published. There is no canonical JSON,
and no parsing before verification.

## 8. The base system's HTTPS client is used

metaupd runs `fetch(1)` on FreeBSD instead of linking a TLS library. This keeps the
code small and follows the system's TLS updates. It depends on `fetch` behaviour, such
as following redirects without restricting them to HTTPS; authenticity never rests on
transport.

## 9. No D-Bus dependency in command line tools

The tools work on a minimal system. A daemon will provide D-Bus for desktops and share
the same engine library.

## 10. Rust with pure-Rust dependencies

Every dependency builds without a C compiler. FreeBSD can be cross-checked from any
host, and the native build needs only the `rust` package.
