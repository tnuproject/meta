# Extended Driver Support

Status: EXPERIMENTAL. Everything that looks works; operations on components were tried with one
virtual device.

EDS is the policy and the tool; [MDCF](../drivers/MDCF_ARCHITECTURE.md) is what it is built on.

## Policy

- Active by default when a desktop is selected; inactive for console-only and minimal
  installations. Always overridable.
- Installs nothing by itself. Components marked `validated` need a confirmation, all others a
  second one.
- Never promises universal support.

## Commands

See [MDCF](../drivers/MDCF_ARCHITECTURE.md#commands). `meta-eds enable` and `disable` without a
name record the policy; `meta-eds update` exits with 69, because drivers are updated with the
system.

## Detection

| Bus | Source | root |
|---|---|---|
| PCI | `pciconf -l -c` | No |
| USB | `usbconfig dump_device_desc`, `dump_curr_config_desc` | Yes |
| ACPI | `devinfo -v`: touch devices on I2C, batteries, light sensors, PS/2 | No |

Detection is read-only. A bus that cannot be read is reported, not treated as empty. Composite
USB devices are known by their interfaces.

Limits: SDIO and Thunderbolt devices are not enumerated, and I2C devices only as far as ACPI
describes them.

## Database

TOML files in [eds/driver-database/data](../../eds/driver-database/data), described in
[the model](../drivers/MDCF_COMPATIBILITY_MODEL.md#entries). Loading fails when an entry has no
source, claims validation without a test, or lists packages for unsupported hardware.

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
| Touchpads, touchscreens | Detected through ACPI, documented | EXPERIMENTAL |
| Thunderbolt, docks | Not covered | RESEARCH REQUIRED |
| Sensors, batteries | Detected, no entries | EXPERIMENTAL |
| Power management | Not covered | RESEARCH REQUIRED |
| Brightness | Documented | EXPERIMENTAL |

## Until drivers are installed

With an X11 desktop the installer configures the firmware's framebuffer; see
[installer](../installation/installer.md#what-it-does).

## Display drivers

A display driver that hangs at boot cannot be undone by a program. Before enabling one, create
a boot environment with `bectl create`; after a hang, choose it in the loader.
