# LinuxKPI

Status: EXPERIMENTAL as a registry; META ports no driver.

LinuxKPI lets Linux driver source be compiled for FreeBSD with changes. It is not a binary
interface.

## Registry

[linuxkpi/registry/15.1.toml](../../linuxkpi/registry/15.1.toml) is generated from FreeBSD's
source: 2982 declared functions, 265 of which call themselves unfinished (`TODO`,
`UNIMPLEMENTED`), and the headers that exist only so that drivers compile.

| State | Meaning |
|---|---|
| declared | There is a function or macro with that name |
| unfinished | Its own source says so |
| missing | No declaration was found |

`declared` says nothing about behaviour.

## Advice

[advice.toml](../../linuxkpi/registry/advice.toml) says what usually helps:
`freebsd-equivalent`, `linuxkpi-implementation`, `driver-adaptation`, `upstream-contribution`
or `unsupported`. Advice is for people who port
drivers; nothing acts on it.

## Versions

A registry describes one release and has to be regenerated for the next.

## Relation to upstream

Fixes belong into FreeBSD. [ath11k](../../linuxkpi/ath11k.md) is the first analysis.
See also the [state of upstream](../compatibility/linuxkpi.md).
