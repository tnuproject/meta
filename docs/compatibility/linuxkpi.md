# LinuxKPI

Status: RESEARCH REQUIRED.

LinuxKPI is FreeBSD's implementation of parts of the Linux kernel's internal interfaces.
It lets selected Linux drivers be compiled for FreeBSD with modest changes. It is not a
binary interface: Linux kernel modules cannot be loaded, and a Linux driver works only
after someone ports and maintains it.

META does not maintain a fork of LinuxKPI. Its work is research, testing on real hardware
and upstream contributions.

## State of upstream

From FreeBSD status reports, checked 2026-11-25. META has tested none of this.

| Area | Upstream state | Source |
|---|---|---|
| GPU (DRM/KMS) | Drivers from Linux 6.12 finished for 15.1; 6.13 and 6.14 in review | [2026 Q2](https://www.freebsd.org/status/report-2026-04-2026-06/drm-drivers/) |
| Wi-Fi | iwlwifi, rtw88, rtw89 updated to Linux 6.19 sources; mt76 in progress; ath10k, ath11k, ath12k to do | [2026 Q1](https://www.freebsd.org/status/report-2026-01-2026-03/lkpi-wireless/) |
| Wi-Fi suspend and resume | Blocked by LinuxKPI issues being worked on | [May 2026](https://freebsdfoundation.github.io/proj-laptop/monthly-updates/2026-05) |

## Research questions

| Subsystem | Question | Status |
|---|---|---|
| DRM/KMS | Which drm-kmod versions work with each FreeBSD point release META supports? | RESEARCH REQUIRED |
| Wi-Fi | Throughput, roaming and power saving per driver | RESEARCH REQUIRED |
| PCI, DMA | Which interfaces do wanted drivers use that LinuxKPI lacks? | RESEARCH REQUIRED |
| USB | Is LinuxKPI USB used by any wanted driver, or is webcamd enough? | RESEARCH REQUIRED |
| Firmware | Packaging and licences; loading before the root is mounted | RESEARCH REQUIRED |
| Power management | Suspend paths of DRM and Wi-Fi drivers | RESEARCH REQUIRED |
| Audio | DSP microphones (SOF, ACP) have no FreeBSD driver | RESEARCH REQUIRED |

## First analysis

[linuxkpi/ath11k.md](../../linuxkpi/ath11k.md) finds that Qualcomm's Wi-Fi 6 driver lacks almost
nothing to compile, and that MHI and QMI are the real work. The tool works on any driver in
FreeBSD's tree.

## Method

1. Choose a driver family from hardware reports.
2. List the kernel interfaces it uses; mark what LinuxKPI has, stubs or lacks.
3. Estimate the work, and ask upstream before starting.
4. Test on named machines, recording results in the [EDS database](../hardware/eds.md).

## Licensing

Many Linux drivers are GPL, and some are dual-licensed. GPL drivers can be distributed as
separate modules, as drm-kmod is, but not merged into META's own code.

## Categories

The EDS database keeps these apart: native drivers, LinuxKPI drivers, firmware-only,
userspace integrations such as webcamd, and unsupported hardware.
