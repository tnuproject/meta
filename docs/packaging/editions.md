# Packaging and editions

Status: PLANNED, except the policy code, which is EXPERIMENTAL.

## Three ecosystems

| Ecosystem | Manager | Owns | Status |
|---|---|---|---|
| Native | pkg | The host | EXPERIMENTAL |
| Debian | APT, dpkg | `/compat/meta/debian` | PLANNED |
| Arch | Pacman | `/compat/meta/arch` | PLANNED |

Both editions are the same META with a different compatibility environment. The kernel,
base system and META tools are identical.

## Isolation

- Every path has one owner. Anything outside the edition roots belongs to pkg.
- APT and Pacman run only inside their root, in a jail. They cannot reach the host's
  files, so they cannot overwrite them.
- pkg never writes into an edition root after creating it.
- Roots do not overlap each other or system directories.
- Only `/home` and `/tmp` are shared writable.
- There is no shared dependency resolution. A Linux package can never satisfy a native
  dependency, or the reverse.

[packages/policy](../../packages/policy/src/lib.rs) encodes these rules, and tests check
the [edition definitions](../../packages/editions) against them.

FreeBSD's ports contain `dpkg` and `pacman`. They are not used: run on the host, they
would track files next to pkg.

## Bootstrap and trust

Creating a root means trusting the distribution's keys:

- Debian: FreeBSD packages `debian-keyring`, which holds developer keys, but not the
  archive keyring that signs releases. META must ship the archive keyring, obtained and
  checked independently.
- Arch: `archlinux-keyring` is needed, and Pacman itself to bootstrap.

A root must never be created without signature checking.

## What has been tried

On FreeBSD 15.1-RELEASE-p7, 2026-11-25, in a disposable virtual machine:

- `debootstrap --variant=minbase bookworm /compat/meta/debian` completed. Signature
  checking was turned off because the archive keyring was not available. That is
  acceptable only in an experiment.
- Inside the root, `apt-get update` failed to verify repositories until devfs was mounted.
  The message was "gpgv ... required for verification, but neither seems installed",
  although gpgv was present; `/dev/null` was missing. With devfs mounted, signatures
  verified normally.
- `apt-get install hello` worked, and the program ran.
- With the META prototype package installed, `apt-get install linux-image-amd64` and
  `grub-efi-amd64` had no installation candidate, as intended.
- The root started as a jail from the template.

Nothing has been tried with Arch on FreeBSD.

## Prototype packages

| Package | Purpose | Status |
|---|---|---|
| `meta-tools` (pkg) | The META commands | EXPERIMENTAL |
| `meta-compat-environment` (deb) | Marks the root; pins kernels, boot loaders and init systems so APT cannot select them | EXPERIMENTAL |
| `meta-compat-environment` (Arch) | Marks the root; a hook refuses kernels and boot loaders | EXPERIMENTAL |

The Debian package was installed in a real root. The Arch package has not been built
outside CI definitions, and CI has not run.

## Update model

metaupd updates the host. Compatibility roots are updated by their own manager, started by
a META tool that takes a snapshot first. Rolling back the host leaves roots as they are.

## Removing a root

Follow [the procedure](../compatibility/containers.md#removing-a-root). Deleting a root
with file systems mounted inside it destroys their contents.
