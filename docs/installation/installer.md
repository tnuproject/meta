# Installer

Status: EXPERIMENTAL.

Two programs, so that the interface never needs to write to a disk:

- `meta-installer` collects choices and saves a plan.
- `meta-install-run` carries a plan out. It is the only code in META that writes to disks.

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

## Before a disk is touched

`meta-install-run` refuses unless all of this holds, and only lists devices to find out:

1. The plan validates again and needs nothing unsupported.
2. The disk exists and nothing has it open. Mounted filesystems and imported pools count.
3. The disk's name was repeated, typed on the terminal or given with `--confirm`.
4. The user has a password, given on standard input or typed.
5. The pool name is free and the target directory empty.

The current partitions are shown before asking.

## What it does

GPT with EFI, BIOS boot, optional swap and ZFS partitions; a pool with a boot environment
layout and separate datasets for `/home`, `/var/log` and update state; FreeBSD's minimal set
and kernel with META's packages through `pkg`; deferred package triggers; both BIOS and UEFI
boot code; configuration; the user.

The password goes to `pw` on standard input and appears in no file, argument or log. Root is
locked, sudo is installed and its rules are checked, and single-user mode asks for
authentication.

If a step fails, everything is unmounted and the pool exported. The disk has been erased by
then, but is free for another attempt.

With an X11 desktop, the X server is configured for the firmware's framebuffer, because
without a kernel graphics driver it finds two framebuffer drivers and refuses to choose.
Remove `/usr/local/etc/X11/xorg.conf.d/10-meta-framebuffer.conf` after installing drm-kmod.

## Unattended

```sh
meta-installer --answers installer/answers.example.toml > plan.json
echo "$PASSWORD" | meta-install-run --plan plan.json --confirm ada0 --password-stdin \
    --repositories /path/to/pkg/repos
```

Answer files and plans cannot contain passwords.

## Not supported

| Item | Status |
|---|---|
| UFS | PLANNED |
| Disk encryption | RESEARCH REQUIRED |
| Custom partitioning, dual boot, several disks | PLANNED |
| GRUB | RESEARCH REQUIRED |
| Creating an edition root during installation | PLANNED |

An edition chosen during installation is created after the first boot with
`meta-compat create`.

## Evidence

`tests/vm/suite.sh m5`: refusals, installation, inspection of the result, and boot with BIOS
and UEFI to a login with wrong and right passwords, sudo, locale, timezone, swap, network and
package integrity. `suite.sh m12` adds XFCE with a graphical login. Not tested on physical
disks.
