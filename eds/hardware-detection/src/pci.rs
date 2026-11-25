//! pciconf(8) output.

use crate::{Bus, Device, Kind, ParseError, hex};

/// Parses `pciconf -l`, for example
/// `em0@pci0:0:3:0:  class=0x020000 rev=0x03 hdr=0x00 vendor=0x8086 device=0x100e ...`.
pub fn parse_pciconf(text: &str) -> Result<Vec<Device>, ParseError> {
    text.lines()
        // `pciconf -lv` adds indented detail lines.
        .filter(|line| !line.is_empty() && !line.starts_with([' ', '\t']))
        .map(|line| {
            parse_line(line).ok_or_else(|| ParseError {
                tool: "pciconf",
                line: line.chars().take(200).collect(),
            })
        })
        .collect()
}

fn parse_line(line: &str) -> Option<Device> {
    let (name, rest) = line.split_once('@')?;
    let mut fields = rest.split_whitespace();
    let location = fields.next()?.trim_end_matches(':').to_owned();
    let (mut class, mut vendor, mut device) = (None, None, None);
    for field in fields {
        match field.split_once('=') {
            Some(("class", value)) => class = hex(value),
            Some(("vendor", value)) => vendor = hex(value),
            Some(("device", value)) => device = hex(value),
            _ => {}
        }
    }
    Some(Device {
        bus: Bus::Pci,
        location,
        vendor: u16::try_from(vendor?).ok()?,
        device: u16::try_from(device?).ok()?,
        class: class?,
        // pciconf names unattached devices `noneN`.
        driver: (!is_unattached(name)).then(|| name.to_owned()),
        description: None,
    })
}

fn is_unattached(name: &str) -> bool {
    name.strip_prefix("none")
        .is_some_and(|unit| unit.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn kind(class: u32) -> Kind {
    match (class >> 16, (class >> 8) & 0xff) {
        (0x01, _) => Kind::Storage,
        (0x02, 0x00) => Kind::Ethernet,
        // Wi-Fi adapters report "other network controller" or the wireless class.
        (0x02, 0x80) => Kind::Wireless,
        (0x03, _) => Kind::Graphics,
        (0x04, 0x01 | 0x03) => Kind::Audio,
        (0x06, _) => Kind::Bridge,
        (0x09, _) => Kind::Input,
        (0x0c, 0x03) => Kind::UsbController,
        (0x0d, 0x11) => Kind::Bluetooth,
        (0x0d, _) => Kind::Wireless,
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attached_and_unattached_devices() {
        let text = "em0@pci0:0:3:0:\tclass=0x020000 rev=0x03 hdr=0x00 vendor=0x8086 device=0x100e subvendor=0x1af4 subdevice=0x1100\n    vendor     = 'Intel'\nnone12@pci0:2:0:0:\tclass=0x028000 rev=0x1a hdr=0x00 vendor=0x8086 device=0x2723 subvendor=0x8086 subdevice=0x0084\n";
        let devices = parse_pciconf(text).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].driver_name(), Some("em"));
        assert_eq!(devices[0].location, "pci0:0:3:0");
        assert_eq!(devices[1].driver, None);
        assert_eq!(devices[1].kind(), Kind::Wireless);
        assert_eq!((devices[1].vendor, devices[1].device), (0x8086, 0x2723));
    }

    #[test]
    fn malformed_lines_are_errors() {
        assert!(parse_pciconf("nonsense\n").is_err());
        assert!(parse_pciconf("em0@pci0:0:3:0: class=0x020000 vendor=8086 device=0x1\n").is_err());
    }
}
