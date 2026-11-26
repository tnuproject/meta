# Linux compatibility environment

Status: EXPERIMENTAL. Machines, launching and runtime selection were verified with Debian 12;
see [testing](../development/testing.md#compatibility-layers).

| Level | Provided by | Managed by |
|---|---|---|
| 1 | FreeBSD's Linuxulator | FreeBSD |
| 2 | Debian or Arch userland in a jail, on Linuxulator | [meta-compat](../compatibility/containers.md) |
| 3 | Debian or Arch with its own kernel, under bhyve | `meta-linux` |

Level 2 does not add a kernel interface to level 1; it adds a distribution. Level 3 is the
only one with namespaces, cgroups, systemd and Linux modules.

## Commands

| Command | Purpose | root |
|---|---|---|
| `meta-linux status` | What can run Linux programs here | |
| `meta-linux scan [DIR]` | Linux programs below a directory, and where they would run | |
| `meta-linux runtimes`, `environments` | Kinds of runtime; userlands and machines | |
| `meta-linux image list` | Images and digests | |
| `meta-linux vm create NAME` | Plan; with `--yes`, create | Yes |
| `meta-linux vm start`, `stop`, `restart`, `set`, `remove` | Life cycle | Yes |
| `meta-linux vm list`, `status`, `diagnose` | State | |
| `meta-linux vm console`, `exec` | Output and commands, owner only | |
| `meta-linux app diagnose`, `app run`, `run` | Programs | |
| `meta-linux container install`, `run`, `list`, `stop` | Containers, owner only | |
| `meta-exec analyze`, `explain`, `run` | [Runtime selection](RUNTIME_SELECTION.md) | |

`--json` gives a machine-readable form of everything that reports.

| Exit code | Meaning |
|---|---|
| 0 | Done; or the program's own code |
| 1 | Failed |
| 2 | Usage, or `diagnose` found a problem |
| 66 | No such machine |
| 69 | bhyve cannot be used here |
| 75 | Not running, or not ready |
| 77 | Needs root |
| 125, 126, 127 | The launcher failed; nothing can run the program; not found |
| 128 + n | The program died from signal n |

## Example

```sh
sudo pkg install edk2-bhyve libslirp qemu-tools
sudo meta-linux vm create --yes work
sudo meta-linux vm start --wait work
meta-linux vm exec work -- systemctl status
meta-exec run ./program
```
