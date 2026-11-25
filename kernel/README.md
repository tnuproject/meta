# Kernel

Status: EXPERIMENTAL.

META uses the FreeBSD kernel. [conf/META](conf/META) is GENERIC with the identity `META`,
so `uname -v` tells the two apart. It adds no options.

Build it with [scripts/build-kernel.sh](../scripts/build-kernel.sh); see
[building](../docs/development/building.md#kernel). Built from 15.1-RELEASE-p7 and booted in
QEMU on 2026-11-25.

## Adding an option

State which META feature needs it, what GENERIC lacks, and what it costs. A loadable module
is preferred to a compiled-in option.

## Open points

| Point | Status |
|---|---|
| Building and booting in CI | PLANNED |
| Kernel package that coexists with FreeBSD's | RESEARCH REQUIRED |
| Whether a separate kernel is worth maintaining at all, as GENERIC already suffices | RESEARCH REQUIRED |
