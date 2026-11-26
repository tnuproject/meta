# linux2BSD

Status: EXPERIMENTAL.

linux2BSD makes Linux programs as transparent as FreeBSD allows. It decides whether a
program can run, where its libraries come from, and what is missing when it cannot.

[meta-exec](../linux-compatibility/RUNTIME_SELECTION.md) builds on it, and also considers machines
with a Linux kernel.

## What it is not

Linux and FreeBSD programs for one CPU use the same instructions. The differences are
system calls, the ABI, libraries and kernel interfaces. FreeBSD's Linuxulator implements
Linux system calls inside the kernel; linux2BSD never emulates them and contains no
translation code. It does not run programs for another CPU.

## How a Linux program starts on FreeBSD

1. The kernel recognizes the program as Linux by its ELF marks.
2. It asks for the program's loader, such as `/lib64/ld-linux-x86-64.so.2`.
3. Path lookups by Linux processes try `compat.linux.emul_path`, `/compat/linux` by
   default, before the real root.
4. The Linux loader finds the libraries there.

When Linuxulator is enabled and a userland is installed, `./program` already works.
linux2BSD adds the checks around it.

## Pipeline

| Step | Module | Status |
|---|---|---|
| ELF and ABI detection | `elf.rs` | EXPERIMENTAL |
| Linuxulator state | `runtime.rs` | EXPERIMENTAL |
| Userland discovery | `runtime.rs` | EXPERIMENTAL |
| Library and symbol version resolution | `resolve.rs` | EXPERIMENTAL |
| Compatibility database | `database.rs` | EXPERIMENTAL |
| Decision and diagnostics | `decision.rs` | EXPERIMENTAL |
| Running from the emulation path | `main.rs` | EXPERIMENTAL |
| Running from another root, in a jail | `main.rs`, [meta-compat](../packaging/editions.md) | EXPERIMENTAL |
| Installing missing packages | | PLANNED |
| Automatic start from the shell | | RESEARCH REQUIRED |

### Detection

Evidence is collected from the OS/ABI byte, the ABI notes, the loader path and, for
libraries, dependencies. The report lists what the conclusion rests on. A program with no
marks, which static Linux programs can be, is reported as unknown rather than guessed.

### Resolution

Symbolic links and `..` are resolved as they would be inside the root, so an absolute
link in a Debian root does not lead to the host. The search follows `DT_RUNPATH`,
`$ORIGIN`, `ld.so.conf` and the default directories. Libraries for another architecture
do not count. Symbol versions are compared, because "the library is too old" is the most
common failure across distributions.

Not read: `ld.so.cache`, `LD_LIBRARY_PATH`, `glibc-hwcaps` directories. Individual symbols
are not compared. Libraries a program opens itself with `dlopen` cannot be seen.

### Decision

| Decision | Meaning |
|---|---|
| native | A FreeBSD program |
| linuxulator | Runs against the emulation path |
| other_root | Complete only in an edition root; started in its jail as the calling user |
| not_runnable | With a list of problems, each with a fix where one is known |

"Without known problems" is not a promise that a program works.

## Transparency without configuration

For programs complete in the emulation path, the kernel already provides it. For others,
three mechanisms are candidates. RESEARCH REQUIRED:

- `binmiscctl(8)`, which starts an interpreter for matching files. It works for whole
  file types and may conflict with the kernel's own ELF handling of Linux programs.
- A shell hook for failed commands, which covers only interactive use.
- Wrappers generated at package installation, which is how ports do it.

Wrappers are the most predictable and the likely first step.

## Compatibility database

[linux2bsd/data/compatibility.toml](../../linux2bsd/data/compatibility.toml). Each entry
names programs or libraries, a severity, and its evidence: a document or a dated test.
Entries without evidence do not load.

## Results

Observed on FreeBSD 15.1-RELEASE-p7, amd64, in QEMU, 2026-11-25. Linuxulator reported
Linux 5.15.0.

| Case | Prediction | Reality |
|---|---|---|
| Static program, Linuxulator off | cannot run | `Exec format error` |
| Static program, Linuxulator on | runs | ran, exit status passed through |
| Dynamic glibc program, no userland | cannot run | |
| Same, with `linux_base-rl9` | runs | ran |
| Ubuntu 24.04 `curl` | `libcurl.so.4` missing | loader reported the same library |
| Program using `strlcpy`, built on Ubuntu 24.04 | `GLIBC_2.38` not provided | loader reported the same version |
| Debian 12 `curl`, installed in a Debian root | needs the Debian root | ran there under chroot |
| Ubuntu 24.04 `curl` with that Debian root | needs the Debian root | **wrong**: missing symbol `curl_global_trace` |

The last case shows a limit. Debian's `libcurl` provides the symbol version the program
asks for, but not every symbol in it. Versions are compared; individual symbols are not.

An earlier version accepted FreeBSD's own `libcurl` from `/usr/local/lib` for that program.
FreeBSD libraries are now rejected, and system directories are searched only inside the root.

`linux_base-rl9` refuses to install until the Linux modules are loaded, so enabling comes
first:

```sh
sysrc linux_enable=YES
service linux start
pkg install linux_base-rl9
```

From an edition root, the program must be visible there: inside the root, or below `/home` or
`/tmp`. Entering a jail needs root, so `sudo linux2bsd run`; the program itself runs as the
user who called sudo. Tested with Debian and Arch programs by `tests/vm/suite.sh m3`.

## Known limits of Linuxulator

The [handbook](https://docs.freebsd.org/en/books/handbook/linuxemu/) states that systemd
does not work. Anything that needs Linux namespaces, cgroups or kernel modules should be
expected to fail, which includes Linux container engines. Track security advisories for
Linuxulator itself, such as FreeBSD-SA-26:30.linux on set-user-ID programs.
