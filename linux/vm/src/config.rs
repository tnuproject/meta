//! Machine configuration and its validation.

use serde::{Deserialize, Serialize};

pub const MAX_CPUS: u32 = 64;
pub const MIN_MEMORY_MIB: u32 = 512;
pub const MIN_DISK_GIB: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    #[error("invalid configuration: {}", .0.join("; "))]
    Invalid(Vec<String>),
    #[error("there is no machine named {0:?}")]
    Missing(String),
    #[error("a machine named {0:?} already exists")]
    Exists(String),
    #[error("{0} is running")]
    Running(String),
    #[error("{0} is not running; start it with: meta-linux vm start {0}")]
    NotRunning(String),
    #[error("{0} did not become ready: {1}")]
    NotReady(String, String),
    #[error("bhyve cannot be used here: {0}")]
    Unsupported(String),
    #[error("cannot share {path}: {reason}")]
    Share { path: String, reason: String },
    #[error("image {image}: {reason}")]
    Image { image: String, reason: String },
    #[error("{0}")]
    Guest(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Command(#[from] meta_common::CommandError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Network {
    /// Only connections META makes into the guest.
    #[default]
    Isolated,
    /// The guest may also connect out. It can then reach services the host offers
    /// on its loopback interface.
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cgroups {
    #[default]
    V2,
    V1,
}

/// A port on the host's loopback interface that leads into the guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forward {
    pub host_port: u16,
    pub guest_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Share {
    /// Name in the guest; mounted at `/mnt/host/<tag>`.
    pub tag: String,
    /// Directory on the host.
    pub host: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmConfig {
    pub name: String,
    /// Identifier in the image list.
    pub image: String,
    pub cpus: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    #[serde(default)]
    pub network: Network,
    #[serde(default)]
    pub cgroups: Cgroups,
    /// Host user the machine belongs to. The same user exists in the guest.
    pub owner: String,
    pub owner_uid: u32,
    pub uuid: String,
    /// Port on the host's loopback interface that leads to the guest's SSH.
    pub ssh_port: u16,
    #[serde(default)]
    pub forwards: Vec<Forward>,
    #[serde(default)]
    pub shares: Vec<Share>,
}

pub fn valid_name(name: &str) -> bool {
    (1..=24).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn valid_user(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name != "root"
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

fn valid_uuid(text: &str) -> bool {
    let lengths: Vec<usize> = text.split('-').map(str::len).collect();
    lengths == [8, 4, 4, 4, 12] && text.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
}

/// bhyve's options are separated by commas and shares by `=`, and some settings take
/// no escapes, so a path with such characters cannot be expressed safely.
fn valid_share_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 200
        && !path.contains("/../")
        && !path.ends_with("/..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
}

impl VmConfig {
    /// Name bhyve knows the machine by.
    pub fn bhyve_name(&self) -> String {
        format!("meta-{}", self.name)
    }

    pub fn validate(&self) -> Result<(), VmError> {
        let mut problems = Vec::new();
        if !valid_name(&self.name) {
            problems.push(format!("invalid name {:?}", self.name));
        }
        if !valid_name(&self.image) {
            problems.push(format!("invalid image {:?}", self.image));
        }
        if !(1..=MAX_CPUS).contains(&self.cpus) {
            problems.push(format!("cpus must be between 1 and {MAX_CPUS}"));
        }
        if self.memory_mib < MIN_MEMORY_MIB {
            problems.push(format!("memory must be at least {MIN_MEMORY_MIB} MiB"));
        }
        if !(MIN_DISK_GIB..=16384).contains(&self.disk_gib) {
            problems.push(format!("disk must be at least {MIN_DISK_GIB} GiB"));
        }
        if !valid_user(&self.owner) || self.owner_uid == 0 {
            problems.push("a machine must belong to an ordinary user".into());
        }
        if !valid_uuid(&self.uuid) {
            problems.push("invalid uuid".into());
        }
        let mut ports = vec![self.ssh_port];
        for forward in &self.forwards {
            if ports.contains(&forward.host_port) {
                problems.push(format!("port {} is used twice", forward.host_port));
            }
            if forward.guest_port == 0 {
                problems.push("guest port 0 is not valid".into());
            }
            ports.push(forward.host_port);
        }
        // Lower ports would need privileges that the helper gives up.
        for port in ports.iter().filter(|port| **port < 1024) {
            problems.push(format!("port {port} is reserved"));
        }
        let mut tags: Vec<&str> = Vec::new();
        for share in &self.shares {
            if !valid_name(&share.tag) || tags.contains(&share.tag.as_str()) {
                problems.push(format!("invalid share name {:?}", share.tag));
            }
            if !valid_share_path(&share.host) {
                problems.push(format!("share path {:?} cannot be used", share.host));
            }
            tags.push(&share.tag);
        }
        if self.shares.len() > 4 {
            problems.push("at most 4 shares".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(VmError::Invalid(problems))
        }
    }

    /// Where a host path is visible in the guest.
    pub fn guest_path(&self, host_path: &str) -> Option<String> {
        self.shares.iter().find_map(|share| {
            let rest = host_path.strip_prefix(share.host.trim_end_matches('/'))?;
            let inside = rest.is_empty() || rest.starts_with('/');
            inside.then(|| format!("/mnt/host/{}{rest}", share.tag))
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> VmConfig {
        VmConfig {
            name: "debian".into(),
            image: "debian-12".into(),
            cpus: 2,
            memory_mib: 2048,
            disk_gib: 20,
            network: Network::Isolated,
            cgroups: Cgroups::V2,
            owner: "alice".into(),
            owner_uid: 1001,
            uuid: "00000000-1111-2222-3333-444444444444".into(),
            ssh_port: 22010,
            forwards: vec![Forward {
                host_port: 8080,
                guest_port: 80,
            }],
            shares: vec![Share {
                tag: "share".into(),
                host: "/var/meta-linux/share/debian".into(),
                read_only: false,
            }],
        }
    }

    fn rejected(change: impl FnOnce(&mut VmConfig)) {
        let mut config = sample();
        change(&mut config);
        assert!(config.validate().is_err(), "{config:?}");
    }

    #[test]
    fn sample_is_valid_and_round_trips() {
        let config = sample();
        config.validate().unwrap();
        let text = toml::to_string(&config).unwrap();
        assert_eq!(toml::from_str::<VmConfig>(&text).unwrap(), config);
    }

    #[test]
    fn hostile_values_are_rejected() {
        for name in ["", "-x", "a b", "a,b", "../x", "A", "a/b", "a;b"] {
            rejected(|c| c.name = name.into());
            rejected(|c| c.image = name.into());
            rejected(|c| c.shares[0].tag = name.into());
        }
        for path in [
            "relative", "/a,ro", "/a=b", "/a b", "/a/../b", "/a/..", "/a\nb", "/é",
        ] {
            rejected(|c| c.shares[0].host = path.into());
        }
        for owner in ["root", "", "Alice", "a;b", "-a"] {
            rejected(|c| c.owner = owner.into());
        }
        rejected(|c| c.owner_uid = 0);
        rejected(|c| c.uuid = "x".into());
    }

    #[test]
    fn limits_and_ports_are_checked() {
        rejected(|c| c.cpus = 0);
        rejected(|c| c.cpus = 65);
        rejected(|c| c.memory_mib = 100);
        rejected(|c| c.disk_gib = 1);
        rejected(|c| c.ssh_port = 22);
        rejected(|c| c.forwards[0].host_port = 22010);
        rejected(|c| c.forwards[0].host_port = 80);
    }

    #[test]
    fn unknown_settings_are_rejected() {
        let text = toml::to_string(&sample()).unwrap() + "\nextra_args = \"-x\"\n";
        assert!(toml::from_str::<VmConfig>(&text).is_err());
    }

    #[test]
    fn host_paths_translate_only_inside_shares() {
        let config = sample();
        assert_eq!(
            config
                .guest_path("/var/meta-linux/share/debian/a/b")
                .as_deref(),
            Some("/mnt/host/share/a/b")
        );
        assert_eq!(
            config.guest_path("/var/meta-linux/share/debian").as_deref(),
            Some("/mnt/host/share")
        );
        assert_eq!(config.guest_path("/var/meta-linux/share/debian2/x"), None);
        assert_eq!(config.guest_path("/etc/passwd"), None);
    }
}
