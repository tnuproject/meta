//! usbconfig(8) output.

use crate::{Bus, Device, Kind, ParseError, hex};

/// Parses `usbconfig dump_device_desc`.
pub fn parse_usbconfig(text: &str) -> Result<Vec<Device>, ParseError> {
    let mut devices: Vec<Device> = Vec::new();
    for line in text.lines() {
        if line.starts_with("ugen") {
            devices.push(parse_header(line).ok_or_else(|| ParseError {
                tool: "usbconfig",
                line: line.chars().take(200).collect(),
            })?);
        } else if let (Some(device), Some((key, value))) =
            (devices.last_mut(), line.split_once('='))
        {
            // Values look like `0x0009  <HUB>`.
            let number = value.split_whitespace().next().and_then(hex);
            match (key.trim(), number) {
                ("idVendor", Some(n)) => device.vendor = n as u16,
                ("idProduct", Some(n)) => device.device = n as u16,
                ("bDeviceClass", Some(n)) => device.class = n & 0xff,
                _ => {}
            }
        }
    }
    Ok(devices)
}

/// `ugen0.2: <QEMU Tablet Adomax> at usbus0, cfg=0 md=HOST ...`
fn parse_header(line: &str) -> Option<Device> {
    let (location, rest) = line.split_once(": ")?;
    let description = rest.split_once('<')?.1.rsplit_once('>')?.0;
    Some(Device {
        bus: Bus::Usb,
        location: location.to_owned(),
        vendor: 0,
        device: 0,
        class: 0,
        // usbconfig does not report which kernel driver is attached.
        driver: None,
        description: Some(description.to_owned()),
    })
}

/// Many devices report class 0 and describe themselves per interface, which
/// this command does not show; those are `Other`.
pub(crate) fn kind(class: u32) -> Kind {
    match class {
        0x01 => Kind::Audio,
        0x03 => Kind::Input,
        0x07 => Kind::Printer,
        0x08 => Kind::Storage,
        0x09 => Kind::Hub,
        0x0b => Kind::SmartCard,
        0x0e => Kind::Camera,
        0xe0 => Kind::Bluetooth,
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_descriptors() {
        let devices = parse_usbconfig(include_str!("../fixtures/usbconfig-qemu-15.1.txt")).unwrap();
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].kind(), Kind::Hub);
        assert_eq!((devices[1].vendor, devices[1].device), (0x0627, 0x0001));
        assert!(
            devices[1]
                .description
                .as_deref()
                .unwrap()
                .starts_with("QEMU Tablet")
        );
    }

    #[test]
    fn malformed_header_is_an_error() {
        assert!(parse_usbconfig("ugen0.1 no description\n").is_err());
    }
}
