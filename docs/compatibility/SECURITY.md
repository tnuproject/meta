# Security of the compatibility layers

Status: EXPERIMENTAL. Reviewed by its author only.

A virtual machine is a strong boundary, not an absolute one. A jail is a weaker one, and
Linuxulator is none: a Linux program there has the rights of its user.

## Privileges

| Program | Runs as | Why |
|---|---|---|
| `meta-eds` (looking), `meta-exec`, `meta-linux app`, `container`, `vm exec` | The user | Nothing needs more |
| `meta-eds install`, `enable`, ... | root | pkg, kernel modules, settings |
| `meta-linux vm create`, `start`, `stop`, `set`, `remove` | root | Datasets, jail, `vmm` |
| `meta-linux vm supervise` | root | Restarts bhyve, cleans up |
| `bhyve`, `bhyve-slirp-helper` | The machine's owner, in a jail, in Capsicum | What a guest attacks |
| Guest | Its own kernel | |

No program is set-user-ID. None asks for privileges by itself.

## The machine's jail

`/var/run/meta-linux/NAME` holds read-only `nullfs` mounts of `/bin`, `/lib`, `/libexec`,
`/usr/lib`, `/usr/libexec`, `/usr/sbin` and `/usr/local/lib`, copies of the firmware and the
linker's hints, the machine's disk, and its shared directories. Its `/etc` lists three users, root,
nobody and the owner, without passwords; with an open network it also has the host's `resolv.conf`.
There is no home directory and no other machine's disk.

devfs(8) hides everything except `null`, `zero`, `random`, `vmmctl` and the machine's own
nodes in `vmm` and `vmm.io`, which belong to the owner there and to root on the host.

The jail has the host's IPv4 addresses, because its helper listens on the host's loopback
interface. It has no child jails and no IPv6.

Not limited: processor time, and what `vmmctl` allows, which includes creating further
machines. kern.racct is off by default, so rctl(8) limits are not used.

## Trust

| Input | Treated as | Checks |
|---|---|---|
| Guest image | Untrusted until verified | SHA-256 or SHA-512 pinned in `linux/images.toml`, HTTPS, converted only afterwards |
| Guest | Hostile | Console output is stripped of control characters; its files belong to the owner |
| Machine configuration | Written by root | Validated at every load; unknown settings are refused |
| Names, paths, ports | Untrusted | Allow lists; bhyve's separators cannot occur |
| Program to run | Untrusted | Parsed without executing; size limits |
| `PROGRAM.requirements.toml` | Untrusted | Can only add requirements |
| Driver source given to `predict` | Untrusted | Read as text |
| LinuxKPI and system call registries | Reviewed data | Generated, checked by tests |

Commands are started with argument vectors and absolute paths. The only shell involved is
the guest's, for which every argument is quoted. Tests pass hostile strings through a real
shell.

## Shares

bhyve serves shares as the machine's owner, so the guest's root cannot make a file that
belongs to anyone else. Guest users other than root and the owner cannot use a share. Files the guest's root creates
belong to the owner, and giving them to root fails, so programs that insist on it report an
error. It can set the set-user-ID bit on the owner's files. Shares are
therefore accepted only:

- on a filesystem mounted `nosuid`,
- outside system directories,
- when the directory itself, not a link, belongs to the owner.

The host must not trust a file from a share: it was written by the guest.

## Network

| Mode | Guest reaches | Host reaches |
|---|---|---|
| `isolated` (default) | Nothing | SSH and forwarded ports, on loopback |
| `open` | The network, and **services on the host's loopback interface** through 10.0.2.2 | The same |

libslirp can forbid access to the host's loopback interface; bhyve's helper does not offer
it. Any local user can connect to a forwarded port; SSH still needs the owner's key.

## Secrets

Each machine has an SSH key that only its owner can read, and a host key made on the host.
The host's copy of the private half is deleted after it is written to the seed image, which
the owner can read.

## Launching

Only `TERM`, `COLORTERM`, `LANG`, `LC_*` and `TZ` are passed to a guest. SSH agents, ports and
displays are not forwarded. `--x11` gives the program the display: it can read what is typed
into other windows.

## Containers

An engine in a guest is as powerful there as on any Linux system. Podman runs without
privileges in the guest; Docker and containerd run as its root. Memory and process limits are
set. `--privileged` is not offered.

## Drivers

- Nothing is loaded, installed or changed without a plan and `--yes`.
- Components META has not tested need `--accept-untested`.
- No module is unloaded; modules outside the component are not touched.
- A prediction never leads to an action.
- Firmware comes from signed packages that FreeBSD distributes.

## Review

| Concern | Result |
|---|---|
| Command injection | Argument vectors; quoting tested with hostile strings |
| Path traversal | Names restricted; configurations moved to another name are refused |
| Following links as root | The share and its path must be real; the VM directory is root's |
| Deleting through mounts | Mount table checked before anything is deleted, and the deletion refused otherwise |
| Terminal control from a guest | Filtered |
| Signals to the wrong process | Process number from a root-owned file; the wrong one would only be another process of bhyve's |
| Races on restart | One supervisor per machine; starting a running machine is refused |
| Denial of service | Memory and disk bounded by the configuration; processor time is not |
| Audit | `/var/db/meta-linux/audit.log`, one line per change |

Open:

- `vmmctl` in the jail allows more machines than the configured one.
- The supervisor is a root process that stays for as long as the machine runs.
- The audit log is a file root can edit.
- Nobody outside the project has reviewed this.
