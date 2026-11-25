//! Read-only hardware detection.
//!
//! Uses pciconf(8) and usbconfig(8) from the FreeBSD base system. Nothing here
//! loads drivers or writes to devices.

mod pci;
mod usb;

use meta_common::{CommandError, CommandRunner};
use serde::{Deserialize, Serialize};

pub use pci::parse_pciconf;
pub use usb::parse_usbconfig;

pub const PCICONF: &str = "/usr/sbin/pciconf";
pub const USBCONFIG: &str = "/usr/sbin/usbconfig";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Bus {
    Pci,
    Usb,
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
}

impl Device {
    pub fn kind(&self) -> Kind {
        match self.bus {
            Bus::Pci => pci::kind(self.class),
            Bus::Usb => usb::kind(self.class),
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
    match runner.stdout(PCICONF, &["-l"]) {
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
            collect(&mut inventory, Bus::Usb, parse_usbconfig(&output.stdout));
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
    const PCICONF_OUTPUT: &str = include_str!("../fixtures/pciconf-qemu-15.1.txt");
    const USBCONFIG_OUTPUT: &str = include_str!("../fixtures/usbconfig-qemu-15.1.txt");

    #[test]
    fn detects_captured_devices() {
        let runner = FakeRunner::new()
            .with("/usr/sbin/pciconf -l", PCICONF_OUTPUT)
            .with("/usr/sbin/usbconfig dump_device_desc", USBCONFIG_OUTPUT);
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
            .with("/usr/sbin/pciconf -l", PCICONF_OUTPUT)
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
        assert_eq!(inventory.unavailable.len(), 2);
    }
}
