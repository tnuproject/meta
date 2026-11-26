//! Read-only hardware detection.
//!
//! Uses pciconf(8) and usbconfig(8) from the FreeBSD base system. Nothing here
//! loads drivers or writes to devices.

mod acpi;
mod pci;
mod usb;

use meta_common::{CommandError, CommandRunner};
use serde::{Deserialize, Serialize};

pub use acpi::parse_devinfo;
pub use pci::parse_pciconf;
pub use usb::{parse_interface_classes, parse_usbconfig};

pub const PCICONF: &str = "/usr/sbin/pciconf";
pub const USBCONFIG: &str = "/usr/sbin/usbconfig";
pub const DEVINFO: &str = "/usr/sbin/devinfo";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Pci,
    Usb,
    /// Described by the firmware, including I2C devices.
    Acpi,
}

/// A detected device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Device {
    pub bus: Bus,
    /// `pci0:0:2:0` or `ugen0.2`.
    pub location: String,
    pub vendor: u16,
    pub device: u16,
    /// PCI class code (24 bits) or USB device class (8 bits).
    pub class: u32,
    /// Driver instance attached by the kernel, such as `em0`.
    pub driver: Option<String>,
    pub description: Option<String>,
    /// Subsystem vendor and device, on PCI.
    pub subsystem: Option<(u16, u16)>,
    pub revision: Option<u8>,
    /// What the bus says the device offers, such as `MSI-X` or `PCI-Express`.
    pub capabilities: Vec<String>,
    /// Firmware identifier such as `PNP0C50`, on ACPI.
    pub hardware_id: Option<String>,
    /// USB interface classes; many devices describe themselves only there.
    pub interface_classes: Vec<u8>,
}

impl Device {
    /// A device about which nothing but its location is known.
    pub fn new(bus: Bus, location: impl Into<String>) -> Self {
        Self {
            bus,
            location: location.into(),
            vendor: 0,
            device: 0,
            class: 0,
            driver: None,
            description: None,
            subsystem: None,
            revision: None,
            capabilities: Vec::new(),
            hardware_id: None,
            interface_classes: Vec::new(),
        }
    }

    pub fn kind(&self) -> Kind {
        match self.bus {
            Bus::Pci => pci::kind(self.class),
            Bus::Usb => usb::device_kind(self.class, &self.interface_classes),
            Bus::Acpi => acpi::kind(self.hardware_id.as_deref().unwrap_or_default()),
        }
    }

    /// Whether a specific driver is known to be attached. `vgapci` is a placeholder
    /// the kernel attaches to every display adapter, and usbconfig(8) does not
    /// report drivers.
    pub fn has_specific_driver(&self) -> bool {
        self.driver_name().is_some_and(|name| name != "vgapci")
    }

    /// Driver name without its unit number.
    pub fn driver_name(&self) -> Option<&str> {
        self.driver
            .as_deref()
            .map(|name| name.trim_end_matches(|c: char| c.is_ascii_digit()))
    }
}

/// Broad device category derived from the bus class code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Graphics,
    Ethernet,
    Wireless,
    Bluetooth,
    Audio,
    Storage,
    UsbController,
    Bridge,
    Input,
    Camera,
    Printer,
    SmartCard,
    Hub,
    Sensor,
    Battery,
    Thunderbolt,
    Biometric,
    GameController,
    Other,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inventory {
    pub devices: Vec<Device>,
    /// Buses that could not be read, with the reason.
    pub unavailable: Vec<(Bus, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unexpected {tool} output line {line:?}")]
pub struct ParseError {
    pub tool: &'static str,
    pub line: String,
}

/// Detects devices on every readable bus. A bus that cannot be read is reported
/// rather than treated as empty.
pub fn detect(runner: &dyn CommandRunner) -> Inventory {
    let mut inventory = Inventory::default();
    match runner.stdout(PCICONF, &["-l", "-c"]) {
        Ok(text) => collect(&mut inventory, Bus::Pci, parse_pciconf(&text)),
        Err(error) => inventory.unavailable.push((Bus::Pci, error.to_string())),
    }
    match runner.run(USBCONFIG, &["dump_device_desc"]) {
        // usbconfig reports missing permission on standard output.
        Ok(output) if output.stdout.contains("lack of permissions") => {
            inventory
                .unavailable
                .push((Bus::Usb, "no USB devices, or root is required".into()));
        }
        Ok(output) if output.success() => {
            let mut parsed = parse_usbconfig(&output.stdout);
            // Optional refinement; the devices are valid without it.
            if let (Ok(devices), Ok(text)) = (
                &mut parsed,
                runner.stdout(USBCONFIG, &["dump_curr_config_desc"]),
            ) {
                let classes = parse_interface_classes(&text);
                for device in devices {
                    if let Some(found) = classes.get(&device.location) {
                        device.interface_classes.clone_from(found);
                    }
                }
            }
            collect(&mut inventory, Bus::Usb, parsed);
        }
        Ok(output) => {
            let error = CommandError::Failed {
                program: USBCONFIG.into(),
                code: output.code,
                stderr: output.stderr.trim().into(),
            };
            inventory.unavailable.push((Bus::Usb, error.to_string()));
        }
        Err(error) => inventory.unavailable.push((Bus::Usb, error.to_string())),
    }
    match runner.stdout(DEVINFO, &["-v"]) {
        Ok(text) => collect(&mut inventory, Bus::Acpi, Ok(parse_devinfo(&text))),
        Err(error) => inventory.unavailable.push((Bus::Acpi, error.to_string())),
    }
    inventory
}

fn collect(inventory: &mut Inventory, bus: Bus, parsed: Result<Vec<Device>, ParseError>) {
    match parsed {
        Ok(devices) => inventory.devices.extend(devices),
        Err(error) => inventory.unavailable.push((bus, error.to_string())),
    }
}

/// Parses `0x`-prefixed hexadecimal.
fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;

    // Captured from FreeBSD 15.1-RELEASE in QEMU.
    const PCICONF_OUTPUT: &str = include_str!("../fixtures/pciconf-lc-qemu-15.1.txt");
    const USBCONFIG_OUTPUT: &str = include_str!("../fixtures/usbconfig-qemu-15.1.txt");
    const INTERFACES: &str = include_str!("../fixtures/usbconfig-config-qemu-15.1.txt");
    const DEVINFO: &str = include_str!("../fixtures/devinfo-qemu-15.1.txt");

    #[test]
    fn detects_captured_devices() {
        let runner = FakeRunner::new()
            .with("/usr/sbin/pciconf -l -c", PCICONF_OUTPUT)
            .with("/usr/sbin/usbconfig dump_device_desc", USBCONFIG_OUTPUT)
            .with("/usr/sbin/usbconfig dump_curr_config_desc", INTERFACES)
            .with("/usr/sbin/devinfo -v", DEVINFO);
        let inventory = detect(&runner);
        assert!(
            inventory.unavailable.is_empty(),
            "{:?}",
            inventory.unavailable
        );
        assert_eq!(
            inventory
                .devices
                .iter()
                .filter(|d| d.bus == Bus::Pci)
                .count(),
            9
        );
        assert_eq!(
            inventory
                .devices
                .iter()
                .filter(|d| d.bus == Bus::Usb)
                .count(),
            2
        );
        // The PS/2 controller is an ACPI device; hosts and timers are not listed.
        let acpi: Vec<&Device> = inventory
            .devices
            .iter()
            .filter(|d| d.bus == Bus::Acpi)
            .collect();
        assert_eq!(acpi.len(), 2);
        assert!(acpi.iter().all(|d| d.kind() == Kind::Input));
        let find = |location: &str| {
            let mut devices = inventory.devices.iter();
            devices.find(|d| d.location == location).unwrap()
        };
        assert_eq!(find("ugen0.2").kind(), Kind::Input);
        assert_eq!(find("pci0:0:4:0").capabilities, ["MSI-X"]);
        let kinds: Vec<Kind> = inventory.devices.iter().map(Device::kind).collect();
        for kind in [
            Kind::Graphics,
            Kind::Ethernet,
            Kind::Audio,
            Kind::Storage,
            Kind::Hub,
        ] {
            assert!(kinds.contains(&kind), "{kind:?} missing from {kinds:?}");
        }
    }

    #[test]
    fn missing_permission_is_reported() {
        let runner = FakeRunner::new()
            .with("/usr/sbin/devinfo -v", "")
            .with("/usr/sbin/pciconf -l -c", PCICONF_OUTPUT)
            .with(
                "/usr/sbin/usbconfig dump_device_desc",
                "No device match or lack of permissions.\n",
            );
        let inventory = detect(&runner);
        assert_eq!(inventory.unavailable.len(), 1);
        assert_eq!(inventory.unavailable[0].0, Bus::Usb);
    }

    #[test]
    fn missing_tools_are_reported_per_bus() {
        let inventory = detect(&FakeRunner::new());
        assert!(inventory.devices.is_empty());
        assert_eq!(inventory.unavailable.len(), 3);
    }
}
