# Roadmap

A milestone is finished when its exit criterion has been met by something that was
run. Criteria are not changed to fit results. Dates are deliberately absent.

| Milestone | Scope | Status |
|---|---|---|
| M0 | Repository and project architecture | IMPLEMENTED |
| M1 | FreeBSD development environment and kernel build pipeline | EXPERIMENTAL |
| M2 | META userspace and Rust CLI | IMPLEMENTED |
| M3 | linux2BSD prototype | IMPLEMENTED |
| M4 | META Network research and backend prototype | IMPLEMENTED |
| M5 | META installer TUI | IMPLEMENTED |
| M6 | Native pkg infrastructure | EXPERIMENTAL |
| M7 | Debian and Arch compatibility editions | IMPLEMENTED |
| M8 | EDS and LinuxKPI research | EXPERIMENTAL |
| M9 | metaupd OTA prototype and signed release metadata | IMPLEMENTED |
| M10 | Desktop integrations | IMPLEMENTED |
| M11 | Bootable META ISO | IMPLEMENTED |
| M12 | First complete META 26.0 release candidate | EXPERIMENTAL |

An implemented milestone means a working prototype, not a finished component. Component
status is in [status.toml](status.toml).

## Exit criteria and evidence

Everything below was run on FreeBSD 15.1-RELEASE-p7 in QEMU on 2026-11-25 with the scripts
in [tests/vm](tests/vm/README.md).

**M1.** CI builds the META kernel from a pinned FreeBSD source and boots it in a virtual
machine. *Met except for CI:* `suite.sh m1` fetches the commit in
[kernel/SOURCE](kernel/SOURCE), builds, packages and boots it. The workflow has not run on
GitHub.

**M3.** linux2BSD runs programs from roots other than the emulation path, through a jail.
*Met* by `suite.sh m3`, with Debian and Arch.

**M4.** A prototype daemon answers the NetworkManager D-Bus calls `nmcli device status`
makes, backed by real FreeBSD state. *Met* by `suite.sh m4`, using Debian's nmcli 1.42.4.

**M5.** The installer carries out a plan on a virtual disk. *Met* by `suite.sh m5`; the
result boots with BIOS and UEFI.

**M6.** A signed pkg repository with META packages, built in CI. *Met except for CI:*
`suite.sh m6`.

**M7.** A tool creates, enters, updates and removes an edition root safely. *Met* by
`suite.sh m7`, for both editions.

**M8.** The database has entries validated on at least ten physical machines, and a
LinuxKPI gap analysis exists for one chosen driver family. *Half met:* there is an
[analysis of ath11k](linuxkpi/ath11k.md). **No physical machine has been tested.**

**M9.** metaupd applies a release inside a boot environment, validates it, activates it for
one boot and confirms or reverts. *Met* by `suite.sh m9`, including an update that cannot
boot.

**M10.** One desktop shows and starts updates through the D-Bus API. *Met* by
`suite.sh m10`, in XFCE.

**M11.** An ISO boots into the installer in QEMU under BIOS and UEFI. *Met* by
`suite.sh m11`, which also completes both installations without a network.

**M12.** Installation, update, rollback and one desktop work on physical hardware. *Not
met.* `suite.sh m12` does all of it in a virtual machine with throwaway keys. **It has not
been done on physical hardware, and there are no META signing keys.**

## What is left for M1, M6, M8 and M12

| Needed | By whom |
|---|---|
| Push to GitHub and a passing *System tests* workflow | A maintainer (M1, M6) |
| Reports from ten physical machines, following [the procedure](eds/compatibility/README.md) | Contributors with hardware (M8) |
| Installation, update and rollback on physical hardware | A maintainer with a spare machine (M12) |
| A [key ceremony](docs/security/release-signing.md) and a second maintainer | Maintainers (M12) |

## Next

Physical hardware. Virtual machines have shown what they can.
