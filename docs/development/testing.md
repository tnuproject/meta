# Testing

## Automated

| Kind | Where |
|---|---|
| Unit tests | In each crate |
| ELF identification and decisions, including every truncation and byte corruption of a sample | `linux2bsd/tests/elf.rs` |
| Update attacks and recovery | `metaupd/tests/tests/updates.rs` |
| Ordering and cleanup of update installation | `metaupd/tests/tests/apply.rs` |
| Refusals and cleanup of the installer | `installer/src/execute.rs` |
| OTA tools end to end | `tests/ota-e2e.sh` |
| Whole milestones on FreeBSD | `tests/vm` |
| Interface rendering at several sizes | `installer/src/ui.rs` |

Files under `fixtures/` are output captured from FreeBSD 15.1-RELEASE-p7 in QEMU, and are
kept unmodified.

## On FreeBSD

The [virtual machine tests](../../tests/vm/README.md) run whole milestones, including reboots,
installations and a graphical session.

## Validation record

2026-11-25, FreeBSD 15.1-RELEASE-p7 amd64, pkgbase, ZFS root, QEMU i440FX with KVM.

| Area | What was done | Result |
|---|---|---|
| Build | Workspace built with Rust 1.98.1 from packages | Passed |
| Tests | All workspace tests, `ota-e2e.sh` | Passed |
| meta | All commands | Output matched the system |
| EDS | Scan as root | Matched `pciconf` and `usbconfig` |
| linux2BSD | [Cases](../compatibility/linux2bsd.md#results) | Matched, except one with a missing symbol |
| metaupd transport | `check` against the GitHub API with a throwaway anchor | No releases found, as expected |
| metaupd permissions | Trust directory with mode 777 | Refused |
| Rollback | One-time activation, two reboots | Booted the target, then returned |
| Kernel | META kernel built, installed into a boot environment, booted | `uname` reported META |
| Update installation | `suite.sh m9`: five reboots, one failed update | Passed |
| Package repository | `suite.sh m6`: wrong key, modified package | Passed |
| Editions and linux2BSD | `suite.sh m7`: Debian 12 and Arch | Passed |
| Network service | `suite.sh m4`: nmcli 1.42.4 | Passed |
| Installation | `suite.sh m5`: BIOS and UEFI | Passed |
| Installation image | `suite.sh m11`: BIOS and UEFI, no network | Passed |
| Desktop updates | `suite.sh m10`: XFCE on a virtual display | Passed |
| Rehearsal | `suite.sh m12`: XFCE login, signed update with kernel, rollback | Passed with BIOS |
| Installer | Driven through tmux to a saved plan | Passed |
| Fastfetch | Configuration rendered | FreeBSD logo |
| Shell scripts | ShellCheck 0.11.0 | No findings |
| Workflows | actionlint 1.7.12 | No findings; not run on GitHub |
| Update state | `meta doctor` before and after creating a dataset | Warned, then passed |

### Compatibility layers

2026-11-25 and 26, same FreeBSD machine, now also with an invariant TSC. bhyve ran inside it,
so guests were nested three deep (Hyper-V, KVM, bhyve): a Debian boot took 20 to 60 minutes.

| Area | What was done | Result |
|---|---|---|
| Discovery | `devinfo -v`, `pciconf -lc`, `usbconfig` captured as fixtures; `tests/vm/guest/mdcf.sh` | Passed; nothing changed by looking |
| Evaluation | VirtIO network CONFIRMED, xHCI and HDA PROBABLE | As expected |
| LinuxKPI prediction | ath11k, rtw88, iwlwifi from releng/15.1 | See [predictions](../drivers/MPDC_PREDICTION_ENGINE.md) |
| Driver life cycle | Plans and refusals; enabling a module | Plans passed; enabling not run, no VirtIO entropy device |
| Images | Debian 12 and Arch downloaded and verified by digest | Passed |
| Confinement | bhyve and its network helper as the owner, in a jail, no host files or devices visible | Passed |
| Debian 12 guest | `mlce-machine.sh`: systemd as process 1, PID namespace, cgroup v2 memory limit, `modprobe dummy`, 9p share both ways, no root-owned files on the host, isolated network | Passed |
| Launching | Arguments, standard input, exit codes 42 and 137, SIGTERM passed on, other users refused | Passed |
| Same program, two kernels | `mlce-requirements.sh`: a static Linux program asks for 17 system calls | Linuxulator lacks 13 of them, the guest none; matches the registry |
| Runtime selection | Linuxulator chosen without requirements; machine chosen when namespaces are declared or a trial failed for `unshare`; preferences cannot override | Passed |
| Stopping | ACPI power button | The guest stopped; whether by itself or after the time limit could not be told, so `forced off` was added |
| Arch guest | Created from the verified image, started; confinement checks of `mlce-machine.sh` | Passed; the boot itself was not waited for |
| Open network | `apt-get update` in the guest | bhyve's network helper aborted; see [limitations](../compatibility/KNOWN_LIMITATIONS.md) |

Problems found and corrected on the way: slirp's restricted mode fails for root in 15.1 (bhyve
now runs as the owner); 9p refused every mount until the jail knew `nobody`; Debian images are
sparse archives that bsdtar cannot unpack (qcow2 is used); systemd's 90 second limits expire
under triple nesting (the tests raise them).

The workspace's tests also pass on Windows 11 (Rust 1.99.0) and on Ubuntu 24.04 under WSL
(Rust 1.99.0). The minimum version, 1.89, is checked only by CI.

Not done for the compatibility layers: cgroup v1, a complete Arch boot, containers and graphical
programs in a guest (`mlce-containers.sh`, `mlce-gui.sh` exist), an entropy device for the driver
life cycle.

Not done: physical hardware, aarch64, the desktop rehearsal with UEFI, graphics acceleration,
Wi-Fi, suspend, desktops other than XFCE, building the Arch package, running the GitHub
workflows.
