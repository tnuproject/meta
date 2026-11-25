# Testing

## Automated

| Kind | Where |
|---|---|
| Unit tests | In each crate |
| ELF identification and decisions, including every truncation and byte corruption of a sample | `linux2bsd/tests/elf.rs` |
| Update attacks and recovery | `metaupd/tests/tests/updates.rs` |
| OTA tools end to end | `tests/ota-e2e.sh` |
| Interface rendering at several sizes | `installer/src/ui.rs` |

Files under `fixtures/` are output captured from FreeBSD 15.1-RELEASE-p7 in QEMU, and are
kept unmodified.

## On FreeBSD

Use a virtual machine with a ZFS root; several features need boot environments. FreeBSD
publishes images with cloud-init support, such as
`FreeBSD-15.1-RELEASE-amd64-BASIC-CLOUDINIT-zfs.qcow2.xz`. Verify the published SHA-512
before use, and work on an overlay:

```sh
qemu-img create -f qcow2 -b base.qcow2 -F qcow2 overlay.qcow2 40G
qemu-system-x86_64 -accel kvm -cpu host -smp 4 -m 8G -display none \
    -drive file=overlay.qcow2,if=virtio -cdrom seed.iso -serial file:console.log \
    -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22 -device virtio-net-pci,netdev=n0
```

`seed.iso` is a cloud-init NoCloud image with your SSH key.

In the machine:

```sh
pkg install rust
cargo test --workspace --locked
sh tests/ota-e2e.sh target/debug
target/debug/meta doctor
target/debug/meta-eds scan
target/debug/metaupd rollback --list
```

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
| Native package | Built, added, checked, removed | Passed |
| Debian root | [Experiment](../packaging/editions.md#what-has-been-tried) | Worked with devfs mounted |
| Jail template | Started and stopped with the Debian root | Passed |
| Installer | Driven through tmux to a saved plan | Passed |
| Fastfetch | Configuration rendered | FreeBSD logo |
| Shell scripts | ShellCheck 0.11.0 | No findings |
| Workflows | actionlint 1.7.12 | No findings; not run on GitHub |
| Update state | `meta doctor` before and after creating a dataset | Warned, then passed |

The workspace's tests also pass on Windows 11 (Rust 1.99.0) and on Ubuntu 24.04 under WSL
(Rust 1.99.0). The minimum version, 1.89, is checked only by CI.

Not done: physical hardware, aarch64, UEFI boot of the META kernel, a desktop session,
building the Arch package, running the GitHub workflows.
