//! NetworkManager's view of FreeBSD's network state.

use std::collections::{BTreeMap, BTreeSet};

use meta_common::{CommandError, CommandRunner};
use meta_net::{Interface, NetworkStatus};

pub const NO_OBJECT: &str = "/";

// Values from NetworkManager's D-Bus API reference.
pub const DEVICE_ETHERNET: u32 = 1;
pub const DEVICE_GENERIC: u32 = 14;
pub const DEVICE_LOOPBACK: u32 = 32;
pub const STATE_UNMANAGED: u32 = 10;
pub const STATE_UNAVAILABLE: u32 = 20;
pub const STATE_DISCONNECTED: u32 = 30;
pub const STATE_IP_CONFIG: u32 = 70;
pub const STATE_ACTIVATED: u32 = 100;
pub const ACTIVE_ACTIVATING: u32 = 1;
pub const ACTIVE_ACTIVATED: u32 = 2;

/// One network interface as a NetworkManager device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub index: u32,
    pub interface: Interface,
    pub kind: u32,
    pub state: u32,
    pub driver: String,
    pub gateway4: Option<String>,
    pub gateway6: Option<String>,
    pub nameservers: Vec<String>,
}

impl Device {
    pub fn path(&self) -> String {
        format!("/org/freedesktop/NetworkManager/Devices/{}", self.index)
    }

    pub fn ip4_path(&self) -> String {
        format!("/org/freedesktop/NetworkManager/IP4Config/{}", self.index)
    }

    pub fn ip6_path(&self) -> String {
        format!("/org/freedesktop/NetworkManager/IP6Config/{}", self.index)
    }

    pub fn profile_path(&self) -> String {
        format!("/org/freedesktop/NetworkManager/Settings/{}", self.index)
    }

    pub fn active_path(&self) -> String {
        format!(
            "/org/freedesktop/NetworkManager/ActiveConnection/{}",
            self.index
        )
    }

    /// Ethernet devices have a profile; nothing else is managed yet.
    pub fn managed(&self) -> bool {
        self.kind == DEVICE_ETHERNET
    }

    pub fn active(&self) -> bool {
        self.managed() && matches!(self.state, STATE_IP_CONFIG | STATE_ACTIVATED)
    }

    pub fn profile_id(&self) -> String {
        format!("Wired {}", self.interface.name)
    }

    /// Stable identifier derived from the interface name, in UUID form.
    pub fn profile_uuid(&self) -> String {
        // FNV-1a over the name with two offsets; uniqueness among interface
        // names is all that is needed.
        let hash = |seed: u64| {
            self.interface.name.bytes().fold(seed, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        };
        let (a, b) = (hash(0xcbf2_9ce4_8422_2325), hash(0x9ae1_6a3b_2f90_404f));
        format!(
            "{:08x}-{:04x}-8{:03x}-a{:03x}-{:012x}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0xfff,
            (b >> 48) & 0xfff,
            b & 0xffff_ffff_ffff
        )
    }

    pub fn carrier(&self) -> bool {
        self.interface.status.as_deref() == Some("active")
    }

    /// Addresses of one family with their prefix length.
    pub fn addresses(&self, v6: bool) -> Vec<(String, u32)> {
        self.interface
            .addresses
            .iter()
            .filter(|address| address.contains(':') == v6)
            .filter_map(|address| {
                let (address, prefix) = address.split_once('/')?;
                // libnm does not accept a zone identifier.
                let address = address.split('%').next()?;
                Some((address.to_owned(), prefix.parse().ok()?))
            })
            .collect()
    }
}

/// Turns FreeBSD's state into devices, keeping indexes stable between calls.
#[derive(Debug, Default)]
pub struct Model {
    indexes: BTreeMap<String, u32>,
    next: u32,
    /// Interfaces a client has asked to disconnect.
    pub disconnected: BTreeSet<String>,
    /// Interfaces being connected.
    pub connecting: BTreeSet<String>,
}

impl Model {
    pub fn devices(&mut self, status: &NetworkStatus) -> Vec<Device> {
        status
            .interfaces
            .iter()
            .map(|interface| {
                let index = *self
                    .indexes
                    .entry(interface.name.clone())
                    .or_insert_with(|| {
                        self.next += 1;
                        self.next
                    });
                let gateway = |route: &Option<meta_net::DefaultRoute>| {
                    route
                        .as_ref()
                        .filter(|route| route.interface == interface.name)
                        .map(|route| route.gateway.clone())
                };
                let gateway4 = gateway(&status.default_ipv4);
                let gateway6 = gateway(&status.default_ipv6)
                    .map(|g| g.split('%').next().unwrap_or_default().to_owned());
                let routed = gateway4.is_some() || gateway6.is_some();
                let kind = kind(interface);
                Device {
                    index,
                    kind,
                    state: self.state(interface, kind),
                    driver: interface
                        .name
                        .trim_end_matches(|c: char| c.is_ascii_digit())
                        .to_owned(),
                    // FreeBSD has one resolver configuration; it belongs to the
                    // interface that reaches the network.
                    nameservers: if routed {
                        status.nameservers.clone()
                    } else {
                        Vec::new()
                    },
                    interface: interface.clone(),
                    gateway4,
                    gateway6,
                }
            })
            .collect()
    }

    fn state(&self, interface: &Interface, kind: u32) -> u32 {
        if kind != DEVICE_ETHERNET {
            return STATE_UNMANAGED;
        }
        let has_ipv4 = interface
            .addresses
            .iter()
            .any(|a| !a.contains(':') && !a.starts_with("169.254."));
        if interface.status.as_deref() == Some("no carrier") {
            STATE_UNAVAILABLE
        } else if interface.up && has_ipv4 && !self.disconnected.contains(&interface.name) {
            STATE_ACTIVATED
        } else if self.connecting.contains(&interface.name) {
            STATE_IP_CONFIG
        } else {
            STATE_DISCONNECTED
        }
    }
}

fn kind(interface: &Interface) -> u32 {
    if interface.loopback {
        DEVICE_LOOPBACK
    } else if interface
        .media
        .as_deref()
        .is_some_and(|media| media.starts_with("Ethernet"))
    {
        DEVICE_ETHERNET
    } else {
        // Wireless interfaces included, until there is hardware to test them on.
        DEVICE_GENERIC
    }
}

/// Interface names this program passes to commands.
pub fn valid_interface(name: &str) -> bool {
    (1..=15).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

const SERVICE: &str = "/usr/sbin/service";

/// Releases the address and takes the interface down.
pub fn disconnect(runner: &dyn CommandRunner, device: &Device) -> Result<(), CommandError> {
    let name = device.interface.name.as_str();
    // Not running is fine: the address may be static.
    let _ = runner.run(SERVICE, &["dhclient", "onestop", name]);
    for (address, _) in device.addresses(false) {
        runner.stdout(meta_net::IFCONFIG, &[name, "inet", &address, "delete"])?;
    }
    runner.stdout(meta_net::IFCONFIG, &[name, "down"])?;
    Ok(())
}

/// Brings the interface up and requests an address. Blocks until there is one.
pub fn connect(runner: &dyn CommandRunner, name: &str) -> Result<(), CommandError> {
    runner.stdout(meta_net::IFCONFIG, &[name, "up"])?;
    runner.stdout(SERVICE, &["dhclient", "onerestart", name])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_net::{DefaultRoute, parse_ifconfig};

    fn status() -> NetworkStatus {
        NetworkStatus {
            interfaces: parse_ifconfig(include_str!("../../fixtures/ifconfig-qemu-15.1.txt")),
            default_ipv4: Some(DefaultRoute {
                gateway: "10.0.2.2".into(),
                interface: "vtnet0".into(),
            }),
            default_ipv6: Some(DefaultRoute {
                gateway: "fe80::2%vtnet0".into(),
                interface: "vtnet0".into(),
            }),
            nameservers: vec!["10.0.2.3".into()],
        }
    }

    #[test]
    fn captured_interfaces_become_devices() {
        let devices = Model::default().devices(&status());
        assert_eq!(devices.len(), 2);
        let ethernet = &devices[0];
        assert_eq!(
            (ethernet.kind, ethernet.state),
            (DEVICE_ETHERNET, STATE_ACTIVATED)
        );
        assert_eq!(ethernet.addresses(false), [("10.0.2.15".to_owned(), 24)]);
        assert!(
            ethernet
                .addresses(true)
                .iter()
                .all(|(a, _)| !a.contains('%'))
        );
        assert_eq!(ethernet.gateway6.as_deref(), Some("fe80::2"));
        assert_eq!(ethernet.driver, "vtnet");
        assert!(ethernet.active());
        let loopback = &devices[1];
        assert_eq!(
            (loopback.kind, loopback.state),
            (DEVICE_LOOPBACK, STATE_UNMANAGED)
        );
        assert!(loopback.nameservers.is_empty());
    }

    #[test]
    fn indexes_survive_changes() {
        let mut model = Model::default();
        let before = model.devices(&status())[1].index;
        let mut changed = status();
        changed.interfaces.remove(0);
        assert_eq!(model.devices(&changed)[0].index, before);
        assert_eq!(model.devices(&status())[1].index, before);
    }

    #[test]
    fn states_follow_the_interface() {
        let mut model = Model::default();
        let mut s = status();
        s.interfaces[0].addresses.clear();
        assert_eq!(model.devices(&s)[0].state, STATE_DISCONNECTED);
        model.connecting.insert("vtnet0".into());
        assert_eq!(model.devices(&s)[0].state, STATE_IP_CONFIG);
        s.interfaces[0].status = Some("no carrier".into());
        assert_eq!(model.devices(&s)[0].state, STATE_UNAVAILABLE);
    }

    #[test]
    fn uuids_are_stable_distinct_and_well_formed() {
        let devices = Model::default().devices(&status());
        let (a, b) = (devices[0].profile_uuid(), devices[1].profile_uuid());
        assert_ne!(a, b);
        assert_eq!(a, devices[0].profile_uuid());
        let lengths: Vec<usize> = a.split('-').map(str::len).collect();
        assert_eq!(lengths, [8, 4, 4, 4, 12]);
    }

    #[test]
    fn interface_names_are_restricted() {
        assert!(valid_interface("vtnet0") && valid_interface("wg_home"));
        for name in ["", "-a", "a b", "a;b", "../x", "aaaaaaaaaaaaaaaa", "0x"] {
            assert!(!valid_interface(name), "{name:?}");
        }
    }
}
