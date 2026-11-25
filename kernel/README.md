# Kernel

Status: EXPERIMENTAL.

META uses the FreeBSD kernel. [conf/META](conf/META) is GENERIC with the identity `META`,
so `uname -v` tells the two apart. It adds no options.

The source is pinned to a commit in [SOURCE](SOURCE). `scripts/fetch-kernel-source.sh` fetches
it, [scripts/build-kernel.sh](../scripts/build-kernel.sh) builds it, and the `meta-kernel`
package installs it beside FreeBSD's as `/boot/kernel.META`. `tests/vm/suite.sh m1` does all of
this and boots the result. Last run on 2026-11-25, in QEMU.

## Adding an option

State which META feature needs it, what GENERIC lacks, and what it costs. A loadable module
is preferred to a compiled-in option.

## Open points

| Point | Status |
|---|---|
| Building and booting in CI | IN DEVELOPMENT |
| Moving the pin to every security release | PLANNED |
| Whether a separate kernel is worth maintaining at all, as GENERIC already suffices | RESEARCH REQUIRED |
