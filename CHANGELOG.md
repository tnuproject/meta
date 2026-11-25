# Changelog

META has not been released. Versions follow `MAJOR.MINOR[.PATCH][-STAGE N]`.

## Unreleased: 26.0 "uglydemon"

### Fixed

- linux2BSD no longer accepts FreeBSD's own libraries for Linux programs.

### Added

- Rust workspace with the `meta` command and shared platform detection.
- linux2BSD prototype: ELF identification, library and symbol version resolution
  inside Linux userlands, compatibility database, running programs through
  Linuxulator.
- EDS prototype: PCI and USB detection, driver database with recorded evidence,
  `meta-eds`.
- metaupd prototype: signed root and release metadata, threshold signatures, key
  rotation, downgrade protection, verified and resumable downloads, installation
  planning, boot environment rollback, tamper-evident history.
- `metaupd-release` for generating, signing and verifying releases.
- metaupd installs releases into boot environments, confirms them after a healthy boot,
  and returns to the previous system otherwise.
- metaupdd, the update service for desktops, and META Update Manager.
- Installer interface with twelve screens, and meta-install-run, which carries out its
  plans.
- Installation image for BIOS and UEFI that works without a network.
- meta-compat: Debian and Arch roots from signed archives, as jails.
- linux2BSD runs programs in those roots.
- meta-netd: a NetworkManager-compatible service for wired networks.
- Signed package repository and packages for tools, services, installer and kernel.
- Pinned kernel source, and a LinuxKPI gap analysis tool with a report on ath11k.
- Virtual machine tests for whole milestones.
- Package ecosystem isolation and override policy.
- Prototype packages for FreeBSD, Debian and Arch.
- META kernel configuration and build script.
- Architecture documents, CI and release preparation workflows.
