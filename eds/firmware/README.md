# Firmware

Status: PLANNED. META ships no firmware. FreeBSD packages it, for example
`gpu-firmware-kmod` and `wifi-firmware-kmod`, with the vendors' licences.

Two things are called firmware and must not be mixed:

- **Loaded firmware** is copied to a device by its driver at every start. It is a file in a
  package and is covered by boot environments.
- **Device firmware** is stored in the device, like UEFI or SSD firmware. Updating it
  cannot be undone and is never automatic.

FreeBSD's `fwget(8)` finds packages with loaded firmware for installed hardware. EDS should
use it rather than keep its own list. RESEARCH REQUIRED.
