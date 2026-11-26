# Architecture

META takes FreeBSD as it is and adds userspace around it. A FreeBSD subsystem is
replaced only when a concrete META feature cannot be built on it.

## Layers

| Layer | Provided by | META's part | Status |
|---|---|---|---|
| Kernel, scheduler, memory | FreeBSD | Kernel configuration with a META identity | EXPERIMENTAL |
| Filesystems | FreeBSD (ZFS, UFS) | Layout | EXPERIMENTAL |
| Network stack | FreeBSD | None | PLANNED |
| Network management | META Network | D-Bus service for wired networks | EXPERIMENTAL |
| Drivers | FreeBSD, LinuxKPI | EDS detection, evaluation and plans | EXPERIMENTAL |
| Linux system calls | FreeBSD Linuxulator | None | PLANNED |
| Linux programs | linux2BSD, meta-exec | Inspection, runtime choice, launching | EXPERIMENTAL |
| Linux kernel features | bhyve, Linux in a guest | Machines and containers | EXPERIMENTAL |
| Host packages | FreeBSD pkg | META packages, override policy | EXPERIMENTAL |
| Linux packages | APT, Pacman in their own roots | Isolation, meta-compat | EXPERIMENTAL |
| Updates | metaupd | Verification, installation, rollback | EXPERIMENTAL |
| Installation | meta-installer | Interface, installation, image | EXPERIMENTAL |
| Desktops | FreeBSD packages | Package sets, Update Manager | EXPERIMENTAL |

## Principles

1. **FreeBSD first.** Linux compatibility is a guest of the system. Linux package
   managers never own host files.
2. **One owner per file.** Every path belongs to exactly one package ecosystem.
   [packages/policy](packages/README.md) states this as tested code.
3. **Verify, then use.** Untrusted input becomes a distinct type only after
   verification. metaupd cannot download or plan from an unverified manifest
   because the functions do not accept one.
4. **Refuse rather than pretend.** An operation that is not implemented exits with
   status 69 and changes nothing.
5. **Claims carry evidence.** The driver and compatibility databases reject
   entries without a source, and "validated" entries without a recorded test.
6. **Small privileged surface.** Detection and reporting need no privileges.
   Privileged operations are separate commands.

## Source layout

| Directory | Contents |
|---|---|
| [common](common) | Version, platform detection, command execution, status registry |
| [cli](cli) | The `meta` command |
| [linux2bsd](linux2bsd/README.md) | ELF inspection, library resolution, runtime choice |
| [eds](eds/README.md) | Hardware detection, driver database, `meta-eds` |
| [compatibility](docs/compatibility/COMPATIBILITY_MODEL.md) | Shared model and registries |
| [linux](docs/linux-compatibility/MLCE_ARCHITECTURE.md) | Machines, program analysis, runtime selection, `meta-linux`, `meta-exec` |
| [networking](networking/README.md) | Network status; META Network design |
| [metaupd](metaupd/README.md) | Update metadata, verification, client, release tool |
| [installer](installer/README.md) | `meta-installer` |
| [packages](packages/README.md) | Policy, editions, package prototypes |
| [kernel](kernel/README.md) | Kernel configuration |
| [linuxkpi](linuxkpi/README.md) | Research notes |
| [desktop](desktop/README.md), [jails](jails/README.md), [coreutils](coreutils/README.md), [installer-iso](installer-iso/README.md) | Designs and data |
| [docs](docs) | Subsystem documents |

## Crate dependencies

```
meta-common
 |- meta-net
 |- meta-eds-detect -- meta-eds-db -- meta-eds, meta-installer
 |- linux2bsd
 |- metaupd-rollback ----------------------------.
 '- metaupd-metadata -- metaupd-github -----------+-- metaupd
                     '- metaupd-verify -----------+
                                                  '-- metaupd-release
meta-pkg-policy (no dependencies on other META crates)
meta-compat = common + pkg-policy; used by linux2bsd
meta-model (no dependencies); used by meta-mdcf, meta-analyzer, meta-maes
meta-mdcf = eds-db + model; used by meta-eds
meta-linux-vm = common + compat
meta-maes = analyzer + linux2bsd + compat + linux-vm; used by meta-linux, meta-exec
meta-netd = net + meta-dbus; metaupdd = metaupd + meta-dbus
meta = common + net + eds + linux2bsd + pkg-policy
```

All third-party crates are pure Rust. The workspace forbids `unsafe` code.

## Base version

META 26.0 is developed against FreeBSD 15.1-RELEASE, which FreeBSD supports until
31 March 2027. FreeBSD 15.2-RELEASE is scheduled for 8 December 2026. META should
move to 15.2 before its first release candidate; see
[decision 2](docs/architecture/decisions.md#2-base-on-freebsd-15).
