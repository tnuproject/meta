# Linux machines

Status: EXPERIMENTAL.

## Files

| Path | Content | Access |
|---|---|---|
| `/var/db/meta-linux/images/` | Verified images | root |
| `/var/db/meta-linux/vms/NAME/config.toml` | Configuration | Written by root, readable |
| `.../data/` | Disk, UEFI variables, seed image | Owner |
| `.../client_key`, `known_hosts` | SSH | Owner; readable |
| `.../console.log` | Serial console, 8 MiB | root |
| `/var/db/meta-linux/audit.log` | Changes | root |
| `/var/meta-linux/share/NAME` | Shared directory, a dataset of its own | Owner |
| `/var/run/meta-linux/NAME` | Root of the running machine's jail | root |

## Creating

`vm create` prints its plan and changes nothing without `--yes`.

1. The image is downloaded and compared with the digest in
   [images.toml](../../linux/images.toml); a wrong download is deleted.
2. The verified qcow2 image is converted once. Machines get a copy, which takes no space at
   first on ZFS.
3. The shared dataset is created with `setuid=off` and `devices=off`.
4. SSH keys for the owner and for the guest are made.
5. A cloud-init seed image is written: user with the owner's name and number, keys, mounts,
   and for cgroup v1 a kernel parameter and a reboot.

Anything that was made is removed when a step fails.

## Running

`vm start` loads `vmm`, checks shares again, and starts a supervisor that

1. builds the jail described in [SECURITY](../compatibility/SECURITY.md),
2. runs bhyve in it as the owner,
3. starts it again when the guest reboots, at most five times in two minutes,
4. removes the jail, the mounts and the kernel's state when the guest powers off.

| bhyve exit | Meaning | Reaction |
|---|---|---|
| 0 | Reboot | Started again |
| 1 | Power off | End |
| 2 | Halt | End |
| 3 | Triple fault | End, reported by `status` and `diagnose` |
| 4 | Error | End, reported |

`vm start --wait` returns when cloud-init has finished and systemd says `running` or
`degraded`. `vm stop` presses the power button and takes the power away after three minutes, or
`--timeout` seconds; `--force` does so at once. `status` then says `forced off`.

After a crash of the host, the next start removes what was left and begins again.

## Configuration

| Option | Default | Limits |
|---|---|---|
| `--cpus` | 2 | 1 to 64 |
| `--memory` | 2048 MiB | at least 512 |
| `--disk` | 20 GiB | at least 4; can grow |
| `--network` | `isolated` | or `open` |
| `--cgroups` | `v2` | `v1` with Debian 12 |
| `--forward HOST:GUEST` | none | host ports from 1024, loopback only |
| `--share TAG=DIR[,ro]` | none | at most three more; only when created |

Kernels are selected inside the guest, with its package manager and boot loader.

## Why not

| Alternative | Reason |
|---|---|
| vm-bhyve, CBSD | Shell frameworks for administrators; run bhyve as root, and configure bridges |
| libvirt | A large daemon with its own privilege model |
| QEMU without acceleration | Slow |
| A bridge | Changes the host's network |
| PCI passthrough, GPU | Gives a guest hardware; needs an IOMMU and a spare device |

## FreeBSD 15.1 and restricted networking

bhyve's slirp helper enters Capsicum before it looks up the user `nobody`, which fails, so
`slirp` without `open` ends with a broken pipe when bhyve runs as root. Upstream has corrected
the order since. Running bhyve as an ordinary user avoids the lookup.
