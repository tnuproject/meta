# Changelog

META has not been released. Versions follow `MAJOR.MINOR[.PATCH][-STAGE N]`.

## Unreleased: 26.0 "uglydemon"

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
- Installer interface with twelve screens, producing installation plans.
- Package ecosystem isolation and override policy.
- Prototype packages for FreeBSD, Debian and Arch.
- META kernel configuration and build script.
- Architecture documents, CI and release preparation workflows.
