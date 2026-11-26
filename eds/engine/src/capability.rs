//! What hardware can do here and now.
//!
//! Presence and operation are kept apart: a camera on the bus is [`Present`], and
//! only a device node a program could open makes it [`Operational`].
//!
//! [`Present`]: CapabilityState::Present
//! [`Operational`]: CapabilityState::Operational

use std::path::Path;

use meta_common::CommandRunner;
use meta_common::platform::sysctl;
use meta_eds_detect::{Inventory, Kind};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityState {
    /// The system offers an interface that programs can use.
    Operational,
    /// The hardware is there, but nothing usable is.
    Present,
    Absent,
    /// Cannot be determined without trying, or is not looked at yet.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Capability {
    pub id: &'static str,
    pub state: CapabilityState,
    /// What was looked at.
    pub basis: String,
}

/// Queries every capability. `root` is `/` except in tests.
pub fn query(inventory: &Inventory, runner: &dyn CommandRunner, root: &Path) -> Vec<Capability> {
    let has = |kind: Kind| inventory.devices.iter().any(|d| d.kind() == kind);
    let node = |path: &str| root.join(path.trim_start_matches('/')).exists();
    let state = |operational: bool, present: bool| match (operational, present) {
        (true, _) => CapabilityState::Operational,
        (false, true) => CapabilityState::Present,
        (false, false) => CapabilityState::Absent,
    };
    let sndstat = std::fs::read_to_string(root.join("dev/sndstat")).unwrap_or_default();
    let pcm = sndstat.lines().any(|line| line.starts_with("pcm"));
    let microphone = sndstat
        .lines()
        .any(|l| l.starts_with("pcm") && l.contains("rec"));
    let wlan = sysctl(runner, "net.wlan.devices");
    let inputs = input_names(runner);
    let touch = inputs
        .iter()
        .any(|n| n.contains("touchscreen") || n.contains("touch screen"));
    let touchpad = inputs
        .iter()
        .any(|n| n.contains("touchpad") || n.contains("trackpad"));
    let sleep = sysctl(runner, "hw.acpi.supported_sleep_state");
    let batteries: u32 = sysctl(runner, "hw.acpi.battery.units")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let backlight = root
        .join("dev/backlight")
        .read_dir()
        .is_ok_and(|mut d| d.next().is_some());

    let mut found = Vec::new();
    let mut add = |id, state, basis: &str| {
        found.push(Capability {
            id,
            state,
            basis: basis.to_owned(),
        });
    };
    add(
        "display",
        state(
            node("/dev/dri/card0") || node("/dev/ttyv0"),
            has(Kind::Graphics),
        ),
        "kernel mode setting or a console",
    );
    add(
        "gpu-acceleration",
        // A render node is what programs open; whether they render correctly is
        // not visible from here.
        state(node("/dev/dri/renderD128"), has(Kind::Graphics)),
        "/dev/dri/renderD128",
    );
    for id in ["video-decoding", "video-encoding"] {
        let possible = if node("/dev/dri/renderD128") {
            CapabilityState::Unknown
        } else {
            state(false, has(Kind::Graphics))
        };
        add(
            id,
            possible,
            "needs a render node and a codec driver; codecs are not queried",
        );
    }
    add("audio-output", state(pcm, has(Kind::Audio)), "/dev/sndstat");
    add(
        "audio-input",
        state(microphone, has(Kind::Audio)),
        "/dev/sndstat",
    );
    add(
        "wifi",
        state(wlan.is_some(), has(Kind::Wireless)),
        "net.wlan.devices",
    );
    add(
        "bluetooth",
        state(
            node("/dev/ubt0") || node("/dev/ubt0.0"),
            has(Kind::Bluetooth),
        ),
        "/dev/ubt0",
    );
    add(
        "camera",
        state(node("/dev/video0"), has(Kind::Camera)),
        "/dev/video0",
    );
    add("touchscreen", state(touch, false), "kern.evdev.input");
    add("touchpad", state(touchpad, false), "kern.evdev.input");
    add(
        "printer",
        state(
            node("/dev/ulpt0") || node("/dev/unlpt0"),
            has(Kind::Printer),
        ),
        "/dev/ulpt0",
    );
    add(
        "suspend",
        suspend(sleep.as_deref()),
        "hw.acpi.supported_sleep_state",
    );
    add(
        "battery",
        state(batteries > 0, false),
        "hw.acpi.battery.units",
    );
    add("brightness", state(backlight, false), "/dev/backlight");
    found
}

/// The firmware offering a sleep state does not mean drivers survive it.
fn suspend(states: Option<&str>) -> CapabilityState {
    match states {
        Some(list) if list.split_whitespace().any(|s| s == "S3" || s == "s2idle") => {
            CapabilityState::Unknown
        }
        Some(_) => CapabilityState::Absent,
        None => CapabilityState::Unknown,
    }
}

fn input_names(runner: &dyn CommandRunner) -> Vec<String> {
    let Ok(text) = runner.stdout(meta_common::platform::SYSCTL, &["kern.evdev.input"]) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.split_once(".name: "))
        .map(|(_, name)| name.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;
    use meta_eds_detect::{Bus, Device};

    fn inventory() -> Inventory {
        let device = |bus, class| Device {
            class,
            ..Device::new(bus, "x")
        };
        Inventory {
            devices: vec![
                device(Bus::Pci, 0x030000),
                device(Bus::Pci, 0x040300),
                device(Bus::Usb, 0x0e),
            ],
            unavailable: Vec::new(),
        }
    }

    fn get(list: &[Capability], id: &str) -> CapabilityState {
        list.iter().find(|c| c.id == id).unwrap().state
    }

    #[test]
    fn presence_is_not_operation() {
        let dir = tempfile::tempdir().unwrap();
        let found = query(&inventory(), &FakeRunner::new(), dir.path());
        for id in ["gpu-acceleration", "audio-output", "camera"] {
            assert_eq!(get(&found, id), CapabilityState::Present, "{id}");
        }
        assert_eq!(get(&found, "wifi"), CapabilityState::Absent);
    }

    #[test]
    fn device_nodes_make_capabilities_operational() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("dev/dri")).unwrap();
        std::fs::write(dir.path().join("dev/dri/renderD128"), "").unwrap();
        std::fs::write(
            dir.path().join("dev/sndstat"),
            "pcm0: <X> (play/rec) default\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("dev/video0"), "").unwrap();
        let runner = FakeRunner::new()
            .with(
                "/sbin/sysctl kern.evdev.input",
                "kern.evdev.input.0.name: ELAN Touchpad\n",
            )
            .with(
                "/sbin/sysctl -n hw.acpi.supported_sleep_state",
                "S3 S4 S5\n",
            );
        let found = query(&inventory(), &runner, dir.path());
        for id in [
            "gpu-acceleration",
            "audio-output",
            "audio-input",
            "camera",
            "touchpad",
        ] {
            assert_eq!(get(&found, id), CapabilityState::Operational, "{id}");
        }
        // Offered by the firmware is not the same as working.
        assert_eq!(get(&found, "suspend"), CapabilityState::Unknown);
        assert_eq!(get(&found, "video-decoding"), CapabilityState::Unknown);
    }
}
