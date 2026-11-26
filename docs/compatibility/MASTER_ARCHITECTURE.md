# Compatibility architecture

Status: EXPERIMENTAL.

META stays FreeBSD: its kernel, its userland, its package manager. Compatibility is added
around it in three parts that share one model.

| Part | Question it answers | Code | Tool |
|---|---|---|---|
| MDCF, hardware driver compatibility framework | Can this hardware be used, and by which driver? | `eds/` | `meta-eds` |
| MLCE, Linux compatibility environment | Where can Linux programs run? | `linux/vm`, `linux/cli`, `packages/compat` | `meta-linux` |
| MAES, adaptive execution | Where should this program run, and why? | `linux/analyzer`, `linux/maes` | `meta-exec` |
| Shared model | What is needed, what is provided, how is that known? | `compatibility/model` | |

No Linux driver is loaded into FreeBSD's kernel as a binary, and no Linux kernel replaces
it. Neither every Linux program nor every Linux driver can work.

```
            hardware                              program
               |                                     |
        discovery (read-only)                analyzer (read-only)
               |                                     |
      requirements + evidence               requirements + evidence
               \                                     /
                \------------ shared model ---------/
                /                                   \
      evaluator: verdict                     MAES: decision
               |                                     |
      plan -> confirmation          native | Linuxulator | userspace | machine
               |                                     |
      pkg, kldload, loader.conf        exec | exec | jail | bhyve + Linux kernel
```

## Levels

| Level | Kernel | Userland | Use |
|---|---|---|---|
| Native | FreeBSD | FreeBSD | FreeBSD programs |
| 1 Linuxulator | FreeBSD | Linux files on the host | Ordinary Linux programs |
| 2 Userspace | FreeBSD | Debian or Arch in a jail | Programs needing a distribution's libraries |
| 3 Virtual machine | Linux | Debian or Arch | systemd, namespaces, cgroups, kernel modules, containers |

A level is used only when the ones above lack something the program is known to need.

## Dependencies

```
meta-model <- meta-mdcf <- meta-eds
meta-model <- meta-analyzer <- meta-maes <- meta-linux, meta-exec
linux2bsd, meta-compat -> meta-maes        meta-linux-vm -> meta-maes
```

## Audit

What existed before and was kept:

| Component | Now |
|---|---|
| linux2BSD | Level 1 and 2 identification; used by the analyzer, unchanged |
| meta-compat | Level 2; used by MAES. Its mount helpers also serve MLCE |
| EDS detection and database | Extended into MDCF. `meta-eds enable` and `disable` without a name still set the policy |
| LinuxKPI gap tool | Kept for analyses by hand; the registry and predictor are new |
| metaupd, installer, META Network, kernel build | Not touched |

## Decisions

| Decision | Reason |
|---|---|
| Evidence is ranked, never added to a score | A percentage would suggest a precision nobody measured |
| Hints never exclude a runtime | Text in a file does not show that it is used |
| Failed programs are not started again elsewhere | They may already have changed something |
| bhyve runs as the owner, in a jail | bhyve is what a hostile guest attacks; as root it would be worth attacking |
| User-mode networking | No bridge, interface or firewall rule on the host |
| SSH as the channel into guests | Authenticated and encrypted; bhyve has no vsock |
| Images pinned by digest in the repository | A changed image is a reviewed change |
| Shares only from `nosuid` datasets | See [SECURITY](SECURITY.md) |
| No D-Bus service yet | See below |

### D-Bus

A service could let desktops show hardware and machines. It would run as root and need
polkit actions for every change, as [metaupd's](../updates/dbus-api.md) does. The tools have a
JSON form that a desktop can read without it, and starting machines is the only operation
that a desktop needs privileges for. Status: PLANNED, after the tools are stable.
