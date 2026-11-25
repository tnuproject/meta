# META 26.0 "uglydemon"

**FreeBSD at its core. Linux compatibility by design.**

META is a FreeBSD-based operating system project. FreeBSD provides the kernel,
drivers, storage and networking; Linux compatibility is an integrated part of the
system rather than an add-on. META is not a Linux distribution.

> [!IMPORTANT]
> META is not an installable operating system yet. This repository holds the
> architecture, tested prototypes of several components, and the infrastructure
> to develop them. There is no ISO, no package repository and no release. The
> [status table](#status) says what works today.

## Vision

- A system that stays recognizably FreeBSD: ZFS, jails, pkg and rc, with changes
  contributed upstream where possible.
- Linux applications that start like any other program when they are supported,
  and explain themselves when they are not.
- Debian and Arch package ecosystems that cannot damage the host system.
- Updates that are cryptographically verified and can be rolled back.
- Plain statements about what works and how that is known.

## Status

Labels: IMPLEMENTED works as documented, EXPERIMENTAL works in part and may
change, IN DEVELOPMENT is being written, PLANNED is designed only, RESEARCH
REQUIRED has open feasibility questions. [status.toml](status.toml) is the source
of this table, and CI keeps the two in agreement.

| Component | Status | Today |
|---|---|---|
| meta CLI | IMPLEMENTED | Version, platform, hardware, network and compatibility reports |
| linux2BSD | EXPERIMENTAL | Identifies Linux programs, resolves libraries and symbol versions, runs supported ones through Linuxulator |
| metaupd | EXPERIMENTAL | Checks, verifies and downloads releases; selects boot environments; does not install updates |
| metaupd-release | EXPERIMENTAL | Generates, signs and verifies release metadata |
| EDS | EXPERIMENTAL | Detects hardware and reports known support; does not install drivers |
| meta-installer | EXPERIMENTAL | Keyboard-driven interface that saves a plan; does not install |
| Package policy | EXPERIMENTAL | Isolation and override rules as tested code; not connected to pkg |
| Kernel build | EXPERIMENTAL | FreeBSD GENERIC with a META identity; built and booted in QEMU |
| Native packages | EXPERIMENTAL | FreeBSD package of the META tools; no repository |
| META Network | RESEARCH REQUIRED | NetworkManager-compatible D-Bus service; only status reporting exists |
| LinuxKPI | RESEARCH REQUIRED | Expanded Linux driver compatibility |
| GRUB | RESEARCH REQUIRED | FreeBSD's loader is used until GRUB is validated |
| metaupd D-Bus | PLANNED | Interface definition only; no daemon |
| Editions | PLANNED | Debian and Arch compatibility roots; metadata packages only |
| Desktops | PLANNED | Package sets recorded; none tested |
| Desktop updates | PLANNED | Discover, GNOME Software and META Update Manager integration |
| Containers | PLANNED | Jail templates for compatibility environments |
| Core utilities | PLANNED | FreeBSD base utilities; no replacements written |
| Installation media | PLANNED | No bootable image exists |

What was tested, where and when is recorded in
[docs/development/testing.md](docs/development/testing.md).

## Architecture

```
 Applications     FreeBSD programs            Linux programs
                        |                          |
 META userspace   meta  meta-eds  metaupd      linux2BSD
                        |             |            |
 Packages         pkg (host)          |     APT / Pacman in separate roots
                        |             |            |
 FreeBSD          base system    ZFS boot     Linuxulator, jails
                        |        environments      |
 Kernel           FreeBSD: scheduler, memory, VFS, network, drivers, LinuxKPI
```

Start with [ARCHITECTURE.md](ARCHITECTURE.md).

| Subsystem | Document |
|---|---|
| Design decisions | [docs/architecture/decisions.md](docs/architecture/decisions.md) |
| Boot and filesystem layout | [docs/architecture/boot-and-storage.md](docs/architecture/boot-and-storage.md) |
| linux2BSD | [docs/compatibility/linux2bsd.md](docs/compatibility/linux2bsd.md) |
| LinuxKPI | [docs/compatibility/linuxkpi.md](docs/compatibility/linuxkpi.md) |
| Containers | [docs/compatibility/containers.md](docs/compatibility/containers.md) |
| META Network | [docs/networking/meta-network.md](docs/networking/meta-network.md) |
| EDS | [docs/hardware/eds.md](docs/hardware/eds.md) |
| metaupd | [docs/updates/metaupd.md](docs/updates/metaupd.md) |
| Release metadata | [docs/updates/metadata-format.md](docs/updates/metadata-format.md) |
| Release process | [docs/updates/release-process.md](docs/updates/release-process.md) |
| Desktop update integration | [docs/updates/desktop-integration.md](docs/updates/desktop-integration.md) |
| Packaging and editions | [docs/packaging/editions.md](docs/packaging/editions.md) |
| Installer | [docs/installation/installer.md](docs/installation/installer.md) |
| Security | [docs/security/threat-model.md](docs/security/threat-model.md) |

## Components

**linux2BSD** inspects a Linux program, picks a Linux userland for it, checks its
libraries, symbol versions and kernel requirement, and starts it through FreeBSD's
Linuxulator. It translates neither machine code nor system calls.

**META Network** is planned as a service with a NetworkManager-compatible D-Bus
interface on top of FreeBSD's network stack. No NetworkManager port exists, and
META does not claim one.

**EDS** (Extended Driver Support) detects hardware and records, for each driver,
whether it is native, LinuxKPI, firmware, userspace or unsupported, and whether
META has tested it.

**metaupd** updates the whole system from signed releases published on
[GitHub Releases](https://github.com/tnuproject/meta/releases). GitHub is only
transport: every release must be signed by META keys. No keys have been published,
so metaupd currently refuses every update.

**The installer** is a keyboard-driven terminal interface. This version produces a
plan and contains no code that writes to disks.

### Editions

Both editions share the same FreeBSD kernel, base system and META tools. They
differ in the compatibility environment installed next to it.

- **META Debian Edition**: APT and DEB packages in `/compat/meta/debian`.
- **META Arch Edition**: Pacman and Arch packages in `/compat/meta/arch`.

pkg remains authoritative for the host in both.

### Desktops

Planned: KDE Plasma, GNOME, XFCE, Cinnamon, i3, Sway, Hyprland, or no desktop.
[desktop/desktops.toml](desktop/desktops.toml) records the packages and whether
they existed in FreeBSD's repository when last checked. On 2026-11-25 the KDE
Plasma desktop packages did not.

## Development

Requirements:

- Rust 1.89 or newer. `rust-toolchain.toml` selects it under rustup; on FreeBSD
  use `pkg install rust`.
- Python 3.11 or newer for the documentation and data checks.
- FreeBSD 15.1 for anything that touches the system. A virtual machine is enough.

Most code builds and its tests run on FreeBSD, Linux, macOS and Windows; commands
that need FreeBSD say so elsewhere.

```sh
cargo build --workspace --locked
cargo test --workspace --locked
sh tests/ota-e2e.sh target/debug
make check
```

```sh
target/debug/meta info
target/debug/meta doctor
target/debug/linux2bsd inspect /path/to/program
target/debug/meta-installer --answers installer/answers.example.toml
```

See [building](docs/development/building.md) and
[testing](docs/development/testing.md), including how to run a FreeBSD virtual
machine.

## Limitations

- Nothing installs META, and metaupd does not apply updates.
- Validation so far used one QEMU virtual machine. No physical hardware, desktop,
  Wi-Fi, suspend or GPU acceleration has been tested.
- Linux support depends on Linuxulator. Programs that need namespaces, cgroups,
  systemd, Linux kernel modules or a newer kernel interface will not work.
- LinuxKPI does not make arbitrary Linux drivers work on FreeBSD.
- Firmware, the EFI system partition and data outside a boot environment cannot
  be rolled back with it.
- amd64 only. aarch64 is untested.

## Roadmap

Milestones M0 to M12 are in [ROADMAP.md](ROADMAP.md).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Hardware reports are especially useful.
Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## License and trademarks

META is released under the [BSD 2-Clause License](LICENSE).

FreeBSD is a registered trademark of the FreeBSD Foundation. META is an independent
project, not affiliated with or endorsed by the FreeBSD Project or the FreeBSD
Foundation. See [assets/branding](assets/branding/README.md).
