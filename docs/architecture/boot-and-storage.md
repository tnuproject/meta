# Boot and storage

Status: EXPERIMENTAL. `meta-install-run` creates this layout. Items marked *observed* were
seen on FreeBSD 15.1-RELEASE-p7.

## Disk layout

| Partition | Purpose |
|---|---|
| EFI system partition | `loader.efi`, and GRUB if it is ever offered |
| Swap | Optional; encrypted with a one-time key when disk encryption is on |
| ZFS pool `zroot` | Everything else |

UFS stays available for systems that do not want ZFS. It has no boot environments, so
updates cannot be rolled back; the installer says so.

## Datasets

| Dataset | Mount point | Inside boot environment |
|---|---|---|
| `zroot/ROOT/<name>` | `/` | yes |
| `zroot/home` | `/home` | no |
| `zroot/var/log` | `/var/log` | no |
| `zroot/meta/state` | `/var/db/metaupd` | no |
| `zroot/meta/compat` | `/compat/meta` | no |

### Update state must live outside the boot environment

*Observed:* in FreeBSD's default layout `/var/db` is inside the boot environment.
Rolling back would therefore also revert metaupd's record of the newest release it has
seen, which is what prevents downgrades, and its history. The installer creates a separate
dataset for `/var/db/metaupd`; without one, metaupd refuses to install and `meta doctor`
warns.

The consequence is that the state can be newer than the running system. metaupd takes
the installed version from the system itself, never from the state.

### Compatibility roots

Roots are large and change independently of the base, so they are outside the boot
environment. Rolling back the base does not roll them back; metaupd reports such
changes as not covered.

## Boot environments

*Observed:* `metaupd rollback NAME --once --yes` selected an environment with
`bectl activate -t`; the next boot used it and the boot after that returned to the
permanent one. A kernel built from [kernel/conf/META](../../kernel/conf/META) was booted
the same way.

Updates rely on this: activate once, boot, check, and only then activate permanently. An
update whose kernel panicked returned to the old environment by itself.

## What boot environments do not cover

| Item | Why | Approach |
|---|---|---|
| EFI system partition | FAT, outside ZFS | Keep the previous loader beside the new one; update last |
| Device firmware | In the device | Never automatic; documented as irreversible |
| `/home`, databases | Shared between environments | Migrations must be backward compatible, or marked irreversible |
| Pool feature flags | Pool-wide | Never upgraded by metaupd |
| External filesystems | Not in the pool | Out of scope |
| Compatibility roots | Own datasets | Snapshot before changing |

## Encryption

RESEARCH REQUIRED. Options are GELI below ZFS, which `loader.efi` can unlock, and native
ZFS encryption, which it cannot use for a root dataset in the same way. The installer
offers the choice in its plan and marks it as open.
