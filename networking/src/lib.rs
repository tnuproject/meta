//! Read-only network status.
//!
//! The seed of META Network: it reports what FreeBSD's own tools say. It does
//! not configure anything.

use meta_common::{CommandError, CommandRunner};
use serde::Serialize;

pub const IFCONFIG: &str = "/sbin/ifconfig";
pub const ROUTE: &str = "/sbin/route";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Interface {
    pub name: String,
    pub up: bool,
    pub running: bool,
    pub loopback: bool,
    pub mtu: Option<u32>,
    pub ether: Option<String>,
    /// Addresses with prefix length.
    pub addresses: Vec<String>,
    pub media: Option<String>,
    pub status: Option<String>,
    pub ssid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DefaultRoute {
    pub gateway: String,
    pub interface: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkStatus {
    pub interfaces: Vec<Interface>,
    pub default_ipv4: Option<DefaultRoute>,
    pub default_ipv6: Option<DefaultRoute>,
    pub nameservers: Vec<String>,
}

pub fn status(runner: &dyn CommandRunner) -> Result<NetworkStatus, CommandError> {
    let interfaces = parse_ifconfig(&runner.stdout(IFCONFIG, &["-a"])?);
    // route(8) fails when there is no default route; that is not an error here.
    let route = |family: &str| {
        let output = runner.run(ROUTE, &["-n", "get", family, "default"]).ok()?;
        if output.success() {
            parse_route(&output.stdout)
        } else {
            None
        }
    };
    let resolv = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    Ok(NetworkStatus {
        interfaces,
        default_ipv4: route("-inet"),
        default_ipv6: route("-inet6"),
        nameservers: parse_resolv_conf(&resolv),
    })
}

/// Parses `ifconfig -a`. Lines it does not understand are skipped because
/// ifconfig(8) output varies with the interface type.
pub fn parse_ifconfig(text: &str) -> Vec<Interface> {
    let mut interfaces: Vec<Interface> = Vec::new();
    for line in text.lines() {
        if !line.starts_with([' ', '\t']) {
            if let Some(interface) = parse_header(line) {
                interfaces.push(interface);
            }
            continue;
        }
        let Some(interface) = interfaces.last_mut() else {
            continue;
        };
        let line = line.trim();
        let mut words = line.split_whitespace();
        match words.next() {
            Some("ether") => interface.ether = words.next().map(str::to_owned),
            Some("inet") => interface.addresses.extend(parse_inet(words)),
            Some("inet6") => interface.addresses.extend(parse_inet6(words)),
            Some("media:") => {
                interface.media = line.strip_prefix("media:").map(|s| s.trim().to_owned());
            }
            Some("status:") => {
                interface.status = line.strip_prefix("status:").map(|s| s.trim().to_owned());
            }
            Some("ssid") => interface.ssid = parse_ssid(line),
            _ => {}
        }
    }
    interfaces
}

fn parse_header(line: &str) -> Option<Interface> {
    let (name, rest) = line.split_once(": flags=")?;
    let flags: Vec<&str> = rest
        .split_once('<')?
        .1
        .split_once('>')?
        .0
        .split(',')
        .collect();
    let mut words = rest.split_whitespace();
    let mtu = words
        .find(|word| *word == "mtu")
        .and_then(|_| words.next()?.parse().ok());
    Some(Interface {
        name: name.to_owned(),
        up: flags.contains(&"UP"),
        running: flags.contains(&"RUNNING"),
        loopback: flags.contains(&"LOOPBACK"),
        mtu,
        ..Interface::default()
    })
}

/// `inet 10.0.2.15 netmask 0xffffff00 broadcast 10.0.2.255`
fn parse_inet<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<String> {
    let address = words.next()?;
    words.find(|word| *word == "netmask")?;
    let mask = u32::from_str_radix(words.next()?.strip_prefix("0x")?, 16).ok()?;
    Some(format!("{address}/{}", mask.count_ones()))
}

/// `inet6 fe80::1%lo0 prefixlen 64 scopeid 0x2`
fn parse_inet6<'a>(mut words: impl Iterator<Item = &'a str>) -> Option<String> {
    let address = words.next()?;
    words.find(|word| *word == "prefixlen")?;
    Some(format!("{address}/{}", words.next()?))
}

/// `ssid "My Network" channel 36 ...` or `ssid simple channel 1 ...`
fn parse_ssid(line: &str) -> Option<String> {
    let rest = line.strip_prefix("ssid ")?;
    let name = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"')?.0,
        None => rest.split_whitespace().next()?,
    };
    (!name.is_empty()).then(|| name.to_owned())
}

pub fn parse_route(text: &str) -> Option<DefaultRoute> {
    let field = |key: &str| {
        text.lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim() == key)
            .map(|(_, value)| value.trim().to_owned())
    };
    Some(DefaultRoute {
        gateway: field("gateway")?,
        interface: field("interface")?,
    })
}

pub fn parse_resolv_conf(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next()? == "nameserver").then(|| words.next().map(str::to_owned))?
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;

    // Captured from FreeBSD 15.1-RELEASE in QEMU.
    const IFCONFIG_OUTPUT: &str = include_str!("../fixtures/ifconfig-qemu-15.1.txt");
    const ROUTE_OUTPUT: &str = include_str!("../fixtures/route-qemu-15.1.txt");

    #[test]
    fn captured_ifconfig() {
        let interfaces = parse_ifconfig(IFCONFIG_OUTPUT);
        assert_eq!(interfaces.len(), 2);
        let vtnet = &interfaces[0];
        assert_eq!(vtnet.name, "vtnet0");
        assert!(vtnet.up && vtnet.running && !vtnet.loopback);
        assert_eq!(vtnet.mtu, Some(1500));
        assert_eq!(vtnet.ether.as_deref(), Some("52:54:00:12:34:56"));
        assert_eq!(vtnet.addresses[0], "10.0.2.15/24");
        assert_eq!(vtnet.addresses.len(), 3);
        assert_eq!(vtnet.status.as_deref(), Some("active"));
        assert!(interfaces[1].loopback);
    }

    #[test]
    fn captured_route() {
        let route = parse_route(ROUTE_OUTPUT).unwrap();
        assert_eq!(
            (route.gateway.as_str(), route.interface.as_str()),
            ("10.0.2.2", "vtnet0")
        );
    }

    #[test]
    fn ssid_forms() {
        assert_eq!(
            parse_ssid("ssid \"My Net\" channel 36").as_deref(),
            Some("My Net")
        );
        assert_eq!(parse_ssid("ssid plain channel 1").as_deref(), Some("plain"));
        assert_eq!(parse_ssid("ssid \"\" channel 1"), None);
    }

    #[test]
    fn missing_default_route_is_not_an_error() {
        let runner = FakeRunner::new()
            .with("/sbin/ifconfig -a", IFCONFIG_OUTPUT)
            .with("/sbin/route -n get -inet default", ROUTE_OUTPUT)
            .with_failure(
                "/sbin/route -n get -inet6 default",
                1,
                "route has not been found",
            );
        let status = status(&runner).unwrap();
        assert!(status.default_ipv4.is_some());
        assert!(status.default_ipv6.is_none());
    }

    #[test]
    fn resolv_conf() {
        let text = "# c\nsearch x\nnameserver 10.0.2.3\nnameserver ::1\nnameserver\n";
        assert_eq!(parse_resolv_conf(text), ["10.0.2.3", "::1"]);
    }
}
