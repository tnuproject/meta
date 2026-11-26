# META 26.0 "uglydemon"

**FreeBSD at its core. Linux compatibility by design.**

META is a FreeBSD-based operating system project. FreeBSD provides the kernel,
drivers, storage and networking; Linux compatibility is an integrated part of the
system rather than an add-on. META is not a Linux distribution.

> [!IMPORTANT]
> META is pre-release software. It can be built, installed, updated and rolled
> back in a virtual machine, and has **never been run on physical hardware**.
> Nothing is published: no image, no package repository, no release, no signing
> keys. The [status table](#status) says what works today.

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
| linux2BSD | EXPERIMENTAL | Checks Linux programs and runs them through Linuxulator or an edition's jail |
| Editions | EXPERIMENTAL | Debian and Arch roots: created from signed archives, run, updated, rolled back, removed |
| metaupd | EXPERIMENTAL | Verifies, downloads and installs signed releases into boot environments; rolls back |
| metaupd D-Bus | EXPERIMENTAL | Update service for desktops, authorized by polkit |
| Desktop updates | EXPERIMENTAL | META Update Manager, tested in XFCE; no Discover or GNOME Software plugin |
| metaupd-release | EXPERIMENTAL | Generates, signs and verifies release metadata |
| meta-installer | EXPERIMENTAL | Installs on a whole disk with ZFS; no UFS, encryption or custom partitioning |
| Installation media | EXPERIMENTAL | Build script for a BIOS and UEFI image that installs without a network |
| EDS | EXPERIMENTAL | Detects hardware, weighs evidence about drivers, plans and carries out changes; no physical hardware tested |
| Linux machines | EXPERIMENTAL | Debian and Arch with their own kernel under bhyve, unprivileged; Debian 12 verified in a nested virtual machine |
| Adaptive execution | EXPERIMENTAL | Finds out what a program needs and runs it in the least demanding runtime that has it |
| META Network | EXPERIMENTAL | NetworkManager-compatible service for wired networks; no Wi-Fi or VPN |
| Kernel build | EXPERIMENTAL | FreeBSD GENERIC with a META identity, from a pinned source, as a package |
| Native packages | EXPERIMENTAL | META packages and a signed repository; none is published |
| Package policy | EXPERIMENTAL | Isolation and override rules as tested code; not connected to pkg |
| Desktops | EXPERIMENTAL | XFCE installed and used in a virtual machine; the others are untested |
| Containers | EXPERIMENTAL | Compatibility roots as jails; Docker, Podman and containerd inside Linux machines |
| LinuxKPI | RESEARCH REQUIRED | Registry, gap analysis and porting predictions; no driver work |
| GRUB | RESEARCH REQUIRED | FreeBSD's loader is used until GRUB is validated |
| Core utilities | PLANNED | FreeBSD base utilities; no replacements written |

What was tested, where and when is recorded in
[docs/development/testing.md](docs/development/testing.md).

## Architecture

```
 Applications     FreeBSD programs            Linux programs
                        |                          |
 META userspace   meta  meta-eds  metaupd      meta-exec, linux2BSD
                        |             |            |
 Packages         pkg (host)          |     APT / Pacman in separate roots
                        |             |            |
 FreeBSD          base system    ZFS boot     Linuxulator, jails, bhyve
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
| Compatibility layers | [docs/compatibility/MASTER_ARCHITECTURE.md](docs/compatibility/MASTER_ARCHITECTURE.md) |
| Hardware drivers | [docs/drivers/MDCF_ARCHITECTURE.md](docs/drivers/MDCF_ARCHITECTURE.md) |
| Linux machines | [docs/linux-compatibility/MLCE_ARCHITECTURE.md](docs/linux-compatibility/MLCE_ARCHITECTURE.md) |
| Runtime selection | [docs/linux-compatibility/RUNTIME_SELECTION.md](docs/linux-compatibility/RUNTIME_SELECTION.md) |
| META Network | [docs/networking/meta-network.md](docs/networking/meta-network.md) |
| EDS | [docs/hardware/eds.md](docs/hardware/eds.md) |
| metaupd | [docs/updates/metaupd.md](docs/updates/metaupd.md) |
| Release metadata | [docs/updates/metadata-format.md](docs/updates/metadata-format.md) |
| Release process | [docs/updates/release-process.md](docs/updates/release-process.md) |
| Desktop update integration | [docs/updates/desktop-integration.md](docs/updates/desktop-integration.md) |
| Packaging and editions | [docs/packaging/editions.md](docs/packaging/editions.md) |
| Installer | [docs/installation/installer.md](docs/installation/installer.md) |
| Installation media | [docs/installation/media.md](docs/installation/media.md) |
| Security | [docs/security/threat-model.md](docs/security/threat-model.md) |

## Components

**linux2BSD** inspects a Linux program, picks a Linux userland for it, checks its
libraries, symbol versions and kernel requirement, and starts it through FreeBSD's
Linuxulator, in an edition's jail when needed. It translates neither machine code
nor system calls.

**META Network** is `meta-netd`, a service with a subset of NetworkManager's D-Bus
interface on top of FreeBSD's network tools. It covers wired networks. It is not a
port of NetworkManager; none exists.

**EDS** (Extended Driver Support) detects hardware, weighs the evidence about each
driver, and says what is confirmed, probable, incompatible or simply unknown. It
changes the system only after showing a plan.

**Linux machines** are Debian or Arch with their own kernel and systemd, under bhyve,
for programs that need what FreeBSD's kernel does not have. bhyve runs as the user, in
a jail.

**Adaptive execution** (`meta-exec`) finds out what a program needs and starts it
natively, through Linuxulator, in an edition or in a machine, and explains why.

**metaupd** updates the whole system from signed releases published on
[GitHub Releases](https://github.com/tnuproject/meta/releases). It installs into a
new boot environment, tries it for one boot, and keeps it only if that works. GitHub
is only transport: every release must be signed by META keys. No keys have been
published, so metaupd currently refuses every update.

**The installer** is a keyboard-driven terminal interface that produces a plan. A
separate program carries it out after the disk's name has been repeated.

### Editions

Both editions share the same FreeBSD kernel, base system and META tools. They
differ in the compatibility environment installed next to it.

- **META Debian Edition**: APT and DEB packages in `/compat/meta/debian`.
- **META Arch Edition**: Pacman and Arch packages in `/compat/meta/arch`.

pkg remains authoritative for the host in both.

### Desktops

Offered: KDE Plasma, GNOME, XFCE, Cinnamon, i3, Sway, Hyprland, or no desktop. Only
XFCE has been tested.
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

See [building](docs/development/building.md), [testing](docs/development/testing.md) and
the [virtual machine tests](tests/vm/README.md).

## Limitations

- Validation so far used QEMU virtual machines. No physical hardware, Wi-Fi, suspend
  or GPU acceleration has been tested.
- The installer uses a whole disk with ZFS. No encryption, dual boot or UFS.
- metaupd updates packages in a boot environment. It does not update the EFI system
  partition or firmware.
- Linux programs that need namespaces, cgroups, systemd, Linux kernel modules or a
  newer kernel interface do not work on Linuxulator; they need a Linux machine, which
  has no accelerated graphics. See
  [known limitations](docs/compatibility/KNOWN_LIMITATIONS.md).
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
