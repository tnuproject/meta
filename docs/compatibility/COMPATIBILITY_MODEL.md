# Compatibility model

Status: IMPLEMENTED. Code: [compatibility/model](../../compatibility/model).

## Requirements

Written as `kind:value`.

| Kind | Example | Meaning |
|---|---|---|
| `arch` | `arch:amd64` | Machine code |
| `abi` | `abi:linux` | Operating system interface |
| `library` | `library:libc.so.6` | Shared library |
| `symbol-version` | `symbol-version:libc.so.6 GLIBC_2.38` | Version of a library's symbols |
| `syscall` | `syscall:unshare` | Linux system call |
| `kernel` | `kernel:cgroup-v2` | Kernel feature |
| `kernel-version` | `kernel-version:6.1` | Lowest Linux version |
| `init` | `init:systemd` | Service manager as the first process |
| `container` | `container:docker` | Container engine |
| `device` | `device:kvm` | Device access |
| `desktop` | `desktop:x11` | Display, sound |
| `linuxkpi` | `linuxkpi:mhi_sync_power_up` | Kernel interface of a driver |
| `firmware` | `firmware:gpu-firmware-amd-kmod-navi33` | Firmware |

## Evidence

Every claim carries where it comes from, weakest first:

| Source | Example |
|---|---|
| `heuristic` | A path found as text in a file |
| `static-analysis` | A library in the dynamic section |
| `declared` | `PROGRAM.requirements.toml` |
| `database` | An entry of META's, with its own reference |
| `documentation` | A manual page |
| `source-code` | A placeholder in FreeBSD's source |
| `observation` | A device node, a failed call |
| `test` | A script in `tests/vm` |

Sources are compared, never added. A need is **firm** when its strongest evidence is more than
heuristic; only firm needs rule something out.

## Provision

What a runtime offers for a requirement: `provided`, `partial`, `configurable`, `missing`,
`unknown`. An implemented system call is `partial`, because an implementation may be
incomplete; only a test could say more. Unknown is not treated as missing.

## Registries

| File | Content | Made by |
|---|---|---|
| `compatibility/data/interfaces.toml` | Kernel features, engines, devices, desktop | Hand; every entry has a reference |
| `compatibility/data/linuxulator-15.1.toml` | Every system call: implemented or placeholder | `generate.py` from FreeBSD's source |
| `linuxkpi/registry/15.1.toml` | Every function LinuxKPI declares, and those it calls unfinished | `generate.py` |
| `linuxkpi/registry/advice.toml` | What to do about common missing interfaces | Hand |
| `linux/analyzer/data/programs.toml` | Programs and what their documentation says they need | Hand |
| `linux/images.toml` | Guest images and digests | Hand |
| `eds/driver-database/data/*.toml` | Hardware | Hand |

```sh
python3 compatibility/generate.py /usr/src 15.1-RELEASE-p7 linuxulator
python3 compatibility/generate.py /usr/src 15.1-RELEASE-p7 linuxkpi
```

## Graph

Programs, requirements, runtimes, devices, drivers, kernel interfaces and firmware are nodes;
edges say `requires`, `provides`, `lacks`, `driven-by`, `selected`, `rejected` or
`conflicts-with` and carry evidence. An edge without evidence is refused.

```sh
meta-exec explain --graph ./program | dot -Tsvg > why.svg
```
