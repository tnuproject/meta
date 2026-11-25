# Roadmap

A milestone is finished when its exit criteria are met by something that can be
run. Dates are deliberately absent.

| Milestone | Scope | Status |
|---|---|---|
| M0 | Repository and project architecture | IMPLEMENTED |
| M1 | FreeBSD development environment and kernel build pipeline | EXPERIMENTAL |
| M2 | META userspace and Rust CLI | IMPLEMENTED |
| M3 | linux2BSD prototype | EXPERIMENTAL |
| M4 | META Network research and backend prototype | RESEARCH REQUIRED |
| M5 | META installer TUI | EXPERIMENTAL |
| M6 | Native pkg infrastructure | EXPERIMENTAL |
| M7 | Debian and Arch compatibility editions | PLANNED |
| M8 | EDS and LinuxKPI research | EXPERIMENTAL |
| M9 | metaupd OTA prototype and signed release metadata | EXPERIMENTAL |
| M10 | Desktop integrations | PLANNED |
| M11 | Bootable META ISO | PLANNED |
| M12 | First complete META 26.0 release candidate | PLANNED |

## Exit criteria

**M1.** CI builds the META kernel from a pinned FreeBSD source and boots it in a
virtual machine. *Today:* [scripts/build-kernel.sh](scripts/build-kernel.sh) built
it on FreeBSD 15.1-RELEASE-p7 and it booted in QEMU, by hand. CI does not do this.

**M3.** linux2BSD runs programs from roots other than the emulation path, through a
jail. *Today:* it runs programs from the emulation path and identifies when another
root is needed.

**M4.** A prototype daemon answers the NetworkManager D-Bus calls `nmcli device
status` makes, backed by real FreeBSD state. *Today:* only status reporting.

**M5.** The installer carries out a plan on a virtual disk. *Today:* plans only.

**M6.** A signed pkg repository with META packages, built in CI. *Today:* one
package, built and installed by hand on FreeBSD.

**M7.** A tool creates, enters, updates and removes an edition root safely. *Today:*
a Debian 12 root was created and used by hand; see
[editions](docs/packaging/editions.md#what-has-been-tried).

**M8.** The database has entries validated on at least ten physical machines, and a
LinuxKPI gap analysis exists for one chosen driver family. *Today:* three entries
validated, all virtual hardware.

**M9.** metaupd applies a release inside a boot environment, validates it, activates
it for one boot and confirms or reverts. *Today:* everything up to applying, plus
rollback.

**M10.** One desktop shows and starts updates through the D-Bus API.

**M11.** An ISO boots into the installer in QEMU under BIOS and UEFI.

**M12.** Installation, update, rollback and one desktop work on physical hardware.

## Next

M9 and M1 come first. Applying updates inside a boot environment is the core of the
system, depends on nothing else, and can be tested completely in a virtual machine.
