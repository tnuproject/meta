//! devinfo(8) output: devices the firmware describes.
//!
//! I2C touchpads and touchscreens, batteries and sensors appear on no other bus.

use crate::{Bus, Device, Kind};

/// Parses `devinfo -v` and returns the devices with a firmware identifier that
/// META knows. Lines look like
/// `      iichid0 pnpinfo name=PNP0C50 _HID=ELAN1200 at handle=...`; unattached
/// devices are called `unknown`.
pub fn parse_devinfo(text: &str) -> Vec<Device> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<Device> {
    let line = line.trim();
    let (name, details) = line.split_once(' ')?;
    let field = |key: &str| {
        details
            .split_whitespace()
            .find_map(|word| word.strip_prefix(key))
            .filter(|value| !value.is_empty() && *value != "none" && *value != "0")
            .map(str::to_owned)
    };
    let hid = field("_HID=").or_else(|| field("_CID="))?;
    // A compatible identifier is more telling than a vendor's own.
    let known = [Some(hid.clone()), field("_CID="), field("name=")]
        .into_iter()
        .flatten()
        .find(|id| kind(id) != Kind::Other)?;
    let location = field("handle=").unwrap_or_else(|| hid.clone());
    Some(Device {
        driver: (name != "unknown").then(|| name.to_owned()),
        description: Some(hid),
        hardware_id: Some(known),
        ..Device::new(Bus::Acpi, location)
    })
}

/// Identifiers from the ACPI and Windows hardware specifications.
pub(crate) fn kind(id: &str) -> Kind {
    match id.to_ascii_uppercase().as_str() {
        // HID over I2C: touchpads, touchscreens and pens.
        "PNP0C50" | "ACPI0C50" => Kind::Input,
        "PNP0F13" | "PNP0303" => Kind::Input,
        "PNP0C0A" => Kind::Battery,
        "ACPI0008" => Kind::Sensor,
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "nexus0\n  acpi0\n    pci0 pnpinfo _HID=PNP0A08 _UID=0 _CID=PNP0A03 at handle=\\_SB_.PCI0\n\
        \x20     iichid0 pnpinfo name=PNP0C50 _HID=ELAN1200 _UID=1 _CID=PNP0C50 at handle=\\_SB_.PCI0.I2C1.TPD0\n\
        \x20   unknown pnpinfo _HID=ACPI0008 _UID=0 _CID=none at handle=\\_SB_.ALSD\n\
        \x20   battery0 pnpinfo _HID=PNP0C0A _UID=1 _CID=none at handle=\\_SB_.BAT0\n";

    #[test]
    fn known_identifiers_are_kept() {
        let devices = parse_devinfo(TEXT);
        let summary: Vec<(Kind, Option<&str>)> = devices
            .iter()
            .map(|d| (d.kind(), d.driver.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                (Kind::Input, Some("iichid0")),
                (Kind::Sensor, None),
                (Kind::Battery, Some("battery0")),
            ]
        );
        assert_eq!(devices[0].description.as_deref(), Some("ELAN1200"));
    }

    #[test]
    fn other_output_is_ignored() {
        assert!(parse_devinfo("garbage\n  x\npnpinfo\n").is_empty());
    }
}
