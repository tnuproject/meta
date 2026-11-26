# Known limitations

## General

- Neither every Linux program nor every Linux driver can work, and META does not try to make
  that true.
- amd64 only.
- Nothing has been tried on physical hardware. bhyve was tested in a FreeBSD machine that
  itself ran under QEMU and KVM, which says nothing about speed.

## Hardware

- Three database entries are validated, all for virtual hardware. Everything else is at best
  PROBABLE.
- PROBABLE means that the kernel attached a driver, not that the device works.
- Devices on I2C, SDIO and Thunderbolt are seen only as far as ACPI describes them.
- Capabilities such as video decoding are derived from device nodes and not exercised.
- LinuxKPI interfaces are known by name. A declared function may do less than Linux's.
- Prediction does not compile anything, and names made by macros count as missing.
- A display driver that hangs the machine has to be undone with a boot environment or from
  the loader.

## Linuxulator

Not in FreeBSD 15.1, whatever the userland: namespaces, cgroups, Linux kernel modules, eBPF,
io_uring, perf events, landlock, pidfd calls, the new mount calls, keyrings. 84 system calls
are placeholders. "Implemented" calls may be incomplete.

## Machines

- Starting one needs root.
- Disk, processors and memory are fixed while a machine runs; memory is not given back.
- No snapshots, suspend, migration or nested virtualization.
- No accelerated graphics: bhyve has no virtio-gpu. PCI passthrough is not offered.
- No audio. bhyve can emulate a sound device; META does not configure it.
- No Secure Boot or TPM.
- Where hypervisors are nested three deep, as in the tests, a guest takes a quarter of an hour
  to boot and systemd's default time limits expire. The tests raise them.
- Only root and the owner can use a share in the guest, and files cannot be given to root
  there.
- Shares use 9p, which is slow and does not notify the guest of changes.
- With `--network open`, bhyve's network helper of FreeBSD 15.1 (`bhyve-slirp-helper`) aborted
  once during a large download, and the machine stopped with it. Its message goes nowhere, so
  the cause is not known. Isolated machines were not affected. Containers and graphical
  programs, which need an open network to be installed, are therefore untested.
- User-mode networking has no ICMP, no IPv6, and is slower than a bridge.
- An open network reaches the host's loopback services.
- Images are pinned. Their servers delete old ones after some time, after which creating a
  machine fails until the pin is renewed. Existing machines are not affected.
- The kernel is the one the distribution installed. Others are installed inside the guest with
  its package manager and selected with its boot loader.
- cgroup v1 works with Debian 12 only. Debian 13 and Arch have removed it.

## Programs

- The analysis sees a file, not what it loads or starts.
- Scripts are sent where their interpreter exists; what they run is not followed.
- Programs copied into a guest do not bring their libraries or data. Put them into the shared
  directory.
- The working directory is kept only inside a share.
- Job control (Ctrl-Z) of a program in a guest stops ssh, not the program.
- Standard output and standard error are merged when a terminal is used, as with any ssh.

## Graphical programs

- X11 only, unaccelerated, with `--x11`. Forwarded programs can watch the display.
- Wayland programs run through XWayland-less X11 support of their toolkit, if it has any.
- The clipboard works as far as X11 forwarding carries it. Nothing else is shared.

## Containers

- Only in a machine. Docker and containerd need the guest's root.
- They are Linux containers, not FreeBSD jails, and share nothing with `meta-compat`.
- Data in images and volumes lives on the machine's disk.
