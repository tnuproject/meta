# Installer

Status: EXPERIMENTAL. It produces plans. No code in it writes to a disk.

`meta-installer` is a keyboard-driven terminal interface that needs no graphics.

## Screens

| Screen | Fields |
|---|---|
| 1. Welcome | Mode: typical, minimal, custom |
| 2. Language and keyboard | Language, layout |
| 3. Timezone | Timezone |
| 4. Network | Hostname, DHCP or later |
| 5. Disk and filesystem | Disk, ZFS or UFS, encryption, swap |
| 6. META edition | None, Debian, Arch |
| 7. User configuration | Name, groups, sudo, password |
| 8. Desktop environment | Seven desktops or console only |
| 9. Drivers and firmware | EDS; shows detected hardware |
| 10. System components | Linux compatibility |
| 11. Bootloader | FreeBSD loader, or GRUB as RESEARCH REQUIRED |
| 12. Review | Every step, with destructive ones marked |

Keys: Up and Down move between fields, or scroll the review; Left and Right change a
choice; Enter continues; Page Up and Page Down change screens; Esc asks before quitting.

## Rules

- No disk is selected until the user chooses one.
- Disks in use are marked. Optical and small media are not offered.
- sudo is always installed, and the root password is locked. A user without sudo produces a
  warning, because nobody could then administer the system.
- EDS follows the desktop choice unless set explicitly.
- Desktop packages missing from FreeBSD's repository at the last check produce a warning.
- Passwords stay in memory that is cleared on exit, and are never written or printed.
- All text input is validated against strict character sets, since it would later reach
  system tools.

Custom mode currently shows the same screens. Its additional partitioning choices are
PLANNED.

## Unattended

```sh
meta-installer --answers installer/answers.example.toml
```

prints the plan as JSON. Answer files cannot contain passwords.

## Carrying out a plan

PLANNED. Requirements for that code:

1. A separate program that takes a plan, so the interface stays unprivileged.
2. Confirmation by typing the disk's name, after showing its size, model and partitions.
3. Refusal when the disk has mounted or imported filesystems.
4. FreeBSD's tools (`gpart`, `zpool`, `zfs`, `bsdinstall` parts, `pkg -r`), not new ones.
5. Passwords passed to `pw` on standard input.
6. Tests against file-backed virtual disks only, until proven.

## Testing

The interface was driven through tmux on FreeBSD 15.1-RELEASE-p7, from start to a saved
plan. Automated tests render every screen at 80x25 and at extreme sizes.
