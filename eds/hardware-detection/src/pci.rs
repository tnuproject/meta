//! pciconf(8) output.

use crate::{Bus, Device, Kind, ParseError, hex};

/// Parses `pciconf -l -c`: device lines such as
/// `em0@pci0:0:3:0:  class=0x020000 rev=0x03 hdr=0x00 vendor=0x8086 device=0x100e ...`
/// followed by indented capability lines such as
/// `    cap 05[d0] = MSI supports 1 message, 64 bit`.
pub fn parse_pciconf(text: &str) -> Result<Vec<Device>, ParseError> {
    let mut devices: Vec<Device> = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        if line.starts_with([' ', '\t']) {
            if let (Some(device), Some(capability)) = (devices.last_mut(), parse_capability(line)) {
                device.capabilities.push(capability);
            }
            continue;
        }
        devices.push(parse_line(line).ok_or_else(|| ParseError {
            tool: "pciconf",
            line: line.chars().take(200).collect(),
        })?);
    }
    Ok(devices)
}

fn parse_capability(line: &str) -> Option<String> {
    let (kind, text) = line.trim().split_once(" = ")?;
    let is_capability = kind.starts_with("cap ") || kind.starts_with("ecap ");
    // The first words name it; the rest are settings.
    is_capability.then(|| {
        text.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    })
}

fn parse_line(line: &str) -> Option<Device> {
    let (name, rest) = line.split_once('@')?;
    let mut fields = rest.split_whitespace();
    let location = fields.next()?.trim_end_matches(':').to_owned();
    let (mut class, mut vendor, mut device) = (None, None, None);
    let (mut subvendor, mut subdevice, mut revision) = (None, None, None);
    for field in fields {
        match field.split_once('=') {
            Some(("class", value)) => class = hex(value),
            Some(("vendor", value)) => vendor = hex(value),
            Some(("device", value)) => device = hex(value),
            Some(("subvendor", value)) => subvendor = hex(value),
            Some(("subdevice", value)) => subdevice = hex(value),
            Some(("rev", value)) => revision = hex(value),
            _ => {}
        }
    }
    let short = |value: Option<u32>| value.and_then(|v| u16::try_from(v).ok());
    Some(Device {
        vendor: short(vendor)?,
        device: short(device)?,
        class: class?,
        // pciconf names unattached devices `noneN`.
        driver: (!is_unattached(name)).then(|| name.to_owned()),
        subsystem: short(subvendor).zip(short(subdevice)),
        revision: revision.and_then(|v| u8::try_from(v).ok()),
        ..Device::new(Bus::Pci, location)
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
        // USB4 host interfaces, which also carry Thunderbolt.
        (0x0c, 0x03) if class & 0xff == 0x40 => Kind::Thunderbolt,
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
    fn subsystem_revision_and_capabilities() {
        let text = "em0@pci0:0:3:0:\tclass=0x020000 rev=0x03 hdr=0x00 vendor=0x8086 device=0x100e subvendor=0x1af4 subdevice=0x1100\n    cap 05[d0] = MSI supports 1 message, 64 bit\n    ecap 0001[100] = AER 2 0 fatal\n    bar   [10] = type Memory\n";
        let devices = parse_pciconf(text).unwrap();
        assert_eq!(devices[0].subsystem, Some((0x1af4, 0x1100)));
        assert_eq!(devices[0].revision, Some(3));
        assert_eq!(devices[0].capabilities, ["MSI", "AER"]);
    }

    #[test]
    fn malformed_lines_are_errors() {
        assert!(parse_pciconf("nonsense\n").is_err());
        assert!(parse_pciconf("em0@pci0:0:3:0: class=0x020000 vendor=8086 device=0x1\n").is_err());
    }
}
