# Predicting portability

Status: EXPERIMENTAL.

`meta-eds predict DIR` reads a Linux driver's source and compares the kernel functions and
headers it uses with the LinuxKPI registry. It is meant for people deciding whether to port a
driver.

| Output | Meaning |
|---|---|
| verdict | EXPERIMENTAL, INCOMPATIBLE or INSUFFICIENT_EVIDENCE. Never PROBABLE or CONFIRMED |
| missing, unfinished | Interfaces, by number of uses, with advice |
| headers | Not in LinuxKPI |
| firmware, device identifiers, architectures | Found in the source |
| graph | `--json`; driver, interfaces, firmware, with evidence |

INCOMPATIBLE is given only when the advice for an interface that is used says that FreeBSD
itself has to be extended.

## Checked against known drivers

FreeBSD releng/15.1, 2026-11-25:

| Driver | State in FreeBSD | Prediction |
|---|---|---|
| ath11k | In the tree, does not work yet | INCOMPATIBLE: MHI and QMI need work |
| rtw88 | Works | INSUFFICIENT_EVIDENCE |
| iwlwifi | Works | INSUFFICIENT_EVIDENCE |

Working drivers are not recognized as such: they contain code for kernels and buses that is
not built, and names made by macros. The result is a reading list, not a probability.

## What it never does

Compile, load, patch or install. No prediction changes a verdict about hardware.
