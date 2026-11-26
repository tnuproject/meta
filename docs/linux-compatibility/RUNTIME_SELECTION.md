# Runtime selection

Status: EXPERIMENTAL.

## Analysis

| Source | Example | Firm |
|---|---|---|
| ELF header, notes, dynamic section | Architecture, interface, libraries, lowest kernel | Yes |
| Libraries that stand for a feature | `liburing.so.2` | Yes |
| Libraries with fallbacks | `libX11.so.6`, `libsystemd.so.0` | No |
| Imported functions, `syscall` instructions of static programs | `unshare` | No |
| Text | `/sys/fs/cgroup/cgroup.controllers` | No |
| Known programs | `dockerd` | Yes, for Linux executables |
| `PROGRAM.requirements.toml` | `requires = ["kernel:namespaces"]` | Yes |
| A trial that failed | `unshare` was not implemented | Yes |
| A trial that succeeded | The same, but the program carried on | No |

`meta-exec analyze --trial PROGRAM` runs the program under truss(1), as you, and remembers
calls that did not exist, for this file's content, in
`~/.local/state/meta-exec/observations.toml`.

## Choice

Candidates are the host, Linuxulator, each edition that exists and each machine. For each:

1. Obstacles that have nothing to do with requirements: wrong processor, missing libraries,
   Linuxulator not loaded, another user's machine.
2. Every firm need that is missing makes it unsuitable.
3. Firm needs that are unknown or have to be set up make it uncertain.

The least demanding suitable candidate wins; without one, the least demanding uncertain one.
Running instances come before stopped ones. A machine that has to be started first is named,
with the command, and not started.

Facts about an instance override the registry: the control group version a machine was
configured with, its kernel's version, the engines installed in it.

## Explanation

```
$ meta-exec explain ./app
virtual-machine (work) is used.
  needed: kernel:namespaces
  not linuxulator: kernel:namespaces is not available
  not userspace (debian): kernel:namespaces is not available
```

A hint that the chosen runtime cannot serve is mentioned together with the next runtime that
could. `--strict` treats hints as requirements. `--runtime` and `--instance` restrict the
choice; they cannot override a missing requirement.

## Running

| Runtime | How |
|---|---|
| Native, Linuxulator | The program replaces the launcher |
| Userspace | `meta-compat exec`, which needs root and runs the program as the user who called sudo |
| Machine | SSH, after copying the program when it is not in a share |

Streams, arguments, the exit code and SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGUSR1 and SIGUSR2
pass through. `--dry-run` shows what would be done.

A program that fails is not started again elsewhere.
