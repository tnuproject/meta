# Extended Driver Support

Status: EXPERIMENTAL. Detection and the database work. Installing drivers does not exist.

## Policy

- Active by default when a desktop is selected; inactive for console-only and minimal
  installations. Always overridable.
- Installs only components marked `validated`. There are three, all virtual hardware, and
  none needs a package.
- Never promises universal support.

## Commands

| Command | Behaviour | Status |
|---|---|---|
| `meta-eds status` | Shows policy and database size | EXPERIMENTAL |
| `meta-eds scan` | Detects hardware, shows matching entries | EXPERIMENTAL |
| `meta-eds list`, `info` | Shows the database | EXPERIMENTAL |
| `meta-eds enable`, `disable` | Records the policy, nothing else | EXPERIMENTAL |
| `meta-eds update` | Exits with 69; updates will come through metaupd | PLANNED |

## Detection

PCI devices come from `pciconf -l`; USB devices from `usbconfig dump_device_desc`, which
needs root. Detection is read-only. A bus that cannot be read is reported, not treated as
empty.

Limits: devices behind I2C, ACPI, SDIO and Thunderbolt are not enumerated, which includes
most laptop touchpads. USB devices that declare their class per interface appear as
`other`.

## Database

TOML files in [eds/driver-database/data](../../eds/driver-database/data). Fields:

| Field | Meaning |
|---|---|
| `id`, `name`, `category` | Identification |
| `support` | `native`, `linuxkpi`, `firmware-only`, `userspace`, `unsupported` |
| `validation` | `validated` by META, or `documented` upstream |
| `driver` | Kernel driver name |
| `modules`, `packages` | What would be needed |
| `source` | Upstream documentation, required |
| `tested_on` | Release, machine, date and result; required for `validated` |
| `matches` | Bus, vendor, devices, kind |

Loading fails when an entry has no source, claims validation without a test, or lists
packages for unsupported hardware.

### Lookup

When the kernel attached a specific driver, only entries for that driver apply. When it
did not, entries are shown as candidates.

## Coverage

| Category | State of the database | Status |
|---|---|---|
| GPU: AMD, Intel, NVIDIA | Documented, by vendor | EXPERIMENTAL |
| Wi-Fi: Intel, Realtek | Documented | EXPERIMENTAL |
| Ethernet: Intel, Realtek, VirtIO | VirtIO validated | EXPERIMENTAL |
| Bluetooth | Documented for USB | EXPERIMENTAL |
| Fingerprint | libfprint documented, no device list | RESEARCH REQUIRED |
| Cameras | USB video class through webcamd | EXPERIMENTAL |
| Camera sensors (MIPI) | None known to work | RESEARCH REQUIRED |
| Audio | HDA and USB documented | EXPERIMENTAL |
| DSP microphones | None known to work | RESEARCH REQUIRED |
| Printers, scanners | CUPS and SANE documented | EXPERIMENTAL |
| Touchpads, touchscreens | Documented, not detected | RESEARCH REQUIRED |
| Thunderbolt, docks | Not covered | RESEARCH REQUIRED |
| Sensors, power management | Not covered | RESEARCH REQUIRED |
| Brightness | Documented | EXPERIMENTAL |

## Until drivers are installed

With an X11 desktop the installer configures the firmware's framebuffer; see
[installer](../installation/installer.md#what-it-does).

## Installing, when it exists

1. Detect, and select validated entries only.
2. Show packages, modules and licences; ask.
3. Create a boot environment.
4. Install with pkg into it; add modules to `kld_list`.
5. Activate for one boot; keep it only if the device attached.

A display driver that hangs at boot is then undone by a reset.
