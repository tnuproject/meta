//! Building bhyve's arguments.

use crate::config::{Network, VmConfig};

pub const BHYVE: &str = "/usr/sbin/bhyve";
pub const BHYVECTL: &str = "/usr/sbin/bhyvectl";
pub const FIRMWARE: &str = "/usr/local/share/uefi-firmware/BHYVE_UEFI.fd";
pub const FIRMWARE_VARS: &str = "/usr/local/share/uefi-firmware/BHYVE_UEFI_VARS.fd";
/// The address bhyve's user-mode network gives every guest.
const GUEST_ADDRESS: &str = "10.0.2.15";

/// Files of a machine as bhyve sees them; see [`crate::confine`].
const DISK: &str = "/vm/disk.raw";
const SEED: &str = "/vm/seed.iso";
const VARIABLES: &str = "/vm/vars.fd";
const FIRMWARE_INSIDE: &str = "/firmware.fd";

/// How bhyve ended, from bhyve(8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Rebooted,
    PoweredOff,
    Halted,
    TripleFault,
    Error,
}

impl Exit {
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Exit::Rebooted,
            1 => Exit::PoweredOff,
            2 => Exit::Halted,
            3 => Exit::TripleFault,
            _ => Exit::Error,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Exit::Rebooted => "rebooted",
            Exit::PoweredOff => "powered off",
            Exit::Halted => "halted",
            Exit::TripleFault => "crashed (triple fault)",
            Exit::Error => "bhyve reported an error",
        }
    }
}

/// Arguments for a validated configuration.
pub fn arguments(config: &VmConfig) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let mut add = |flag: &str, value: String| {
        args.push(flag.to_owned());
        if !value.is_empty() {
            args.push(value);
        }
    };
    // Yield idle processors, report real ones, and exit on unhandled instructions.
    add("-H", String::new());
    add("-w", String::new());
    add("-c", format!("cpus={}", config.cpus));
    add("-m", format!("{}M", config.memory_mib));
    add("-U", config.uuid.clone());
    add("-s", "0,hostbridge".into());
    add("-s", format!("2,virtio-blk,{DISK}"));
    add("-s", format!("3,ahci-cd,{SEED},ro"));
    add("-s", format!("4,virtio-net,{}", network(config)));
    add("-s", "5,virtio-rnd".into());
    for (index, share) in config.shares.iter().enumerate() {
        let mode = if share.read_only { ",ro" } else { "" };
        add(
            "-s",
            format!(
                "{},virtio-9p,{tag}=/share/{tag}{mode}",
                6 + index,
                tag = share.tag
            ),
        );
    }
    add("-s", "31,lpc".into());
    add("-l", "com1,stdio".into());
    add("-l", format!("bootrom,{FIRMWARE_INSIDE},{VARIABLES}"));
    args.push(config.bhyve_name());
    args
}

/// User-mode networking: no interface, bridge or firewall rule on the host.
fn network(config: &VmConfig) -> String {
    let mut rules = vec![format!(
        "tcp:127.0.0.1:{}-{GUEST_ADDRESS}:22",
        config.ssh_port
    )];
    rules.extend(config.forwards.iter().map(|forward| {
        format!(
            "tcp:127.0.0.1:{}-{GUEST_ADDRESS}:{}",
            forward.host_port, forward.guest_port
        )
    }));
    let open = match config.network {
        Network::Isolated => "",
        Network::Open => ",open",
    };
    format!("slirp{open},hostfwd={}", rules.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::sample;

    #[test]
    fn arguments_are_complete() {
        let args = arguments(&sample());
        let text = args.join(" ");
        for expected in [
            "-c cpus=2",
            "-m 2048M",
            "-s 2,virtio-blk,/vm/disk.raw",
            "-s 4,virtio-net,slirp,hostfwd=tcp:127.0.0.1:22010-10.0.2.15:22;tcp:127.0.0.1:8080-10.0.2.15:80",
            "-s 6,virtio-9p,share=/share/share",
        ] {
            assert!(text.contains(expected), "{expected} in {text}");
        }
        assert_eq!(args.last().unwrap(), "meta-debian");
    }

    #[test]
    fn isolation_is_the_default_and_forwards_stay_on_loopback() {
        let args = arguments(&sample()).join(" ");
        assert!(!args.contains("open"));
        assert!(!args.contains("0.0.0.0"));
        let mut config = sample();
        config.network = Network::Open;
        assert!(arguments(&config).join(" ").contains("slirp,open,hostfwd="));
    }

    #[test]
    fn no_device_of_the_host_is_passed_through() {
        let text = arguments(&sample()).join(" ");
        assert!(!text.contains("passthru") && !text.contains("/dev/"));
    }

    #[test]
    fn exit_codes_follow_the_manual() {
        assert_eq!(Exit::from_code(0), Exit::Rebooted);
        assert_eq!(Exit::from_code(1), Exit::PoweredOff);
        assert_eq!(Exit::from_code(4), Exit::Error);
        assert_eq!(Exit::from_code(-1), Exit::Error);
    }
}
