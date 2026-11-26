# Driver compatibility model

## Categories

| Category | Meaning |
|---|---|
| NATIVE_FREEBSD | A driver written for FreeBSD |
| LINUXKPI_COMPATIBLE | Linux's driver, ported to LinuxKPI by someone and shipped |
| FIRMWARE_ONLY | A driver exists; firmware is missing |
| USERSPACE_INTEGRATION | A program drives it, such as webcamd |
| EXTERNAL_DEPENDENCY | A driver outside FreeBSD and its packages |
| PARTIALLY_SUPPORTED | Some functions are missing |
| EXPERIMENTAL | In development upstream |
| UNSUPPORTED | Nothing is known to drive it |

A driver is not LINUXKPI_COMPATIBLE because it compiles.

## Verdicts

| Verdict | Requires |
|---|---|
| CONFIRMED | META tested this device with this driver on this major release |
| PROBABLE | The kernel attached the driver, or the device is listed upstream and nothing contradicts it |
| EXPERIMENTAL | Upstream says so, or the support is partial |
| INCOMPATIBLE | A check failed: architecture, release, missing kernel option, conflict |
| INSUFFICIENT_EVIDENCE | Only the vendor or kind matches, or a fact is missing |

A failed check outweighs everything. Unknown facts lower a verdict and never raise it. The
percentage shown is the share of checks with a definite answer, not a likelihood.

## Checks

Hardware identifier, architecture, release (`kern.osreldate` range), kernel options, LinuxKPI
interfaces, packages, modules, firmware, conflicts with loaded modules, attached driver.

## Entries

| Field | Meaning |
|---|---|
| `id`, `name`, `category`, `support`, `validation`, `source` | As in [EDS](../hardware/eds.md) |
| `driver`, `modules`, `packages` | Driver and where it comes from |
| `arch`, `osreldate` | Where it applies |
| `firmware` | Packages |
| `linuxkpi`, `symbols`, `kernel_config` | Kernel requirements |
| `userspace` | Programs involved |
| `conflicts` | Modules that exclude it |
| `limitations` | Shown with every verdict |
| `tested_on` | Release, machine, date, result; required for `validated` |

Schema 2. Files with another schema are refused.
