# Firmware

Status: EXPERIMENTAL. No hardware that needs firmware has been tried.

FreeBSD's fwget(8) knows which package a device needs. META asks it and adds nothing, so the
list is as good as fwget's.

| Step | How |
|---|---|
| Find | `fwget -n -q` |
| Compare | `pkg info` |
| Failures | Messages of the kernel about firmware that could not be loaded |
| Install | `meta-eds plan install ID`, then `install`; or `fwget` itself |

## Integrity and licences

Packages are signed by FreeBSD and verified by pkg(8). META ships no firmware, does not copy
it into images, and does not download it from anywhere else. Each package names its licence;
`pkg info -l` shows the file.

## Limits

- Firmware needed before the root filesystem is mounted has to be loaded from `loader.conf` by
  hand.
- Processor microcode is the `cpu-microcode` package; it is not covered here.
- fwupd and UEFI updates are out of scope.
