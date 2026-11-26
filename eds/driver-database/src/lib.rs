//! EDS hardware compatibility database.
//!
//! The database says what is known about hardware and how it is known. It does
//! not promise support: only entries with a recorded META test are
//! [`Validation::Validated`], and only those may ever be installed.

use std::collections::BTreeSet;

use meta_eds_detect::{Bus, Device, Kind};
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 2;

/// Where driver support comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Written for FreeBSD.
    Native,
    /// A Linux driver built on LinuxKPI.
    Linuxkpi,
    /// The driver exists; only firmware has to be added.
    FirmwareOnly,
    /// Handled by a userspace program.
    Userspace,
    /// Needs something that neither FreeBSD nor META provides.
    ExternalDependency,
    PartiallySupported,
    Experimental,
    /// Known not to work.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Validation {
    /// Tested by the META project; `tested_on` records where.
    Validated,
    /// Claimed by upstream documentation, not tested by META.
    Documented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Gpu,
    Wireless,
    Ethernet,
    Bluetooth,
    Biometrics,
    Camera,
    Audio,
    Storage,
    Input,
    Peripheral,
    Platform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub support: Support,
    pub validation: Validation,
    /// Kernel driver name as pciconf(8) shows it.
    #[serde(default)]
    pub driver: Option<String>,
    /// Kernel modules to load, when not loaded automatically.
    #[serde(default)]
    pub modules: Vec<String>,
    /// FreeBSD packages that provide the component.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Upstream documentation for the claim.
    pub source: String,
    /// Architectures it exists for; empty means no restriction is recorded.
    #[serde(default)]
    pub arch: Vec<String>,
    /// `__FreeBSD_version` range it is known for.
    #[serde(default)]
    pub osreldate: Option<Range>,
    /// Firmware packages or modules.
    #[serde(default)]
    pub firmware: Vec<String>,
    /// LinuxKPI interfaces that decide whether it can work.
    #[serde(default)]
    pub linuxkpi: Vec<String>,
    /// Kernel symbols it needs beyond the stable interfaces.
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Kernel options that have to be set.
    #[serde(default)]
    pub kernel_config: Vec<String>,
    /// Programs or services it needs in userspace.
    #[serde(default)]
    pub userspace: Vec<String>,
    /// Modules it cannot be used with.
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub limitations: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tested_on: Vec<Tested>,
    #[serde(default)]
    pub matches: Vec<Match>,
}

impl Entry {
    /// The category in the compatibility model.
    pub fn category_label(&self) -> &'static str {
        match self.support {
            Support::Native => "NATIVE_FREEBSD",
            Support::Linuxkpi => "LINUXKPI_COMPATIBLE",
            Support::FirmwareOnly => "FIRMWARE_ONLY",
            Support::Userspace => "USERSPACE_INTEGRATION",
            Support::ExternalDependency => "EXTERNAL_DEPENDENCY",
            Support::PartiallySupported => "PARTIALLY_SUPPORTED",
            Support::Experimental => "EXPERIMENTAL",
            Support::Unsupported => "UNSUPPORTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tested {
    pub release: String,
    pub machine: String,
    pub date: String,
    pub result: String,
}

/// Every field that is present must match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    pub bus: Bus,
    #[serde(default)]
    pub vendor: Option<u16>,
    #[serde(default)]
    pub devices: Vec<u16>,
    #[serde(default)]
    pub kind: Option<Kind>,
}

impl Match {
    pub fn applies_to(&self, device: &Device) -> bool {
        self.bus == device.bus
            && self.vendor.is_none_or(|vendor| vendor == device.vendor)
            && (self.devices.is_empty() || self.devices.contains(&device.device))
            && self.kind.is_none_or(|kind| kind == device.kind())
    }

    fn specificity(&self) -> usize {
        usize::from(self.vendor.is_some())
            + usize::from(self.kind.is_some())
            + 2 * usize::from(!self.devices.is_empty())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Database {
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    #[serde(default)]
    entry: Vec<Entry>,
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("{file}: {reason}")]
    Parse { file: String, reason: String },
    #[error("entry {id}: {reason}")]
    Invalid { id: String, reason: &'static str },
}

const BUILTIN: &[(&str, &str)] = &[
    ("audio.toml", include_str!("../data/audio.toml")),
    ("graphics.toml", include_str!("../data/graphics.toml")),
    ("network.toml", include_str!("../data/network.toml")),
    ("peripherals.toml", include_str!("../data/peripherals.toml")),
    ("platform.toml", include_str!("../data/platform.toml")),
];

impl Database {
    /// The database shipped with this build.
    pub fn builtin() -> Self {
        Self::parse(BUILTIN).expect("the embedded database is validated by the test suite")
    }

    pub fn parse(files: &[(&str, &str)]) -> Result<Self, DatabaseError> {
        let mut entries = Vec::new();
        for (name, text) in files {
            let parse_error = |reason: String| DatabaseError::Parse {
                file: (*name).to_owned(),
                reason,
            };
            let file: File = toml::from_str(text).map_err(|e| parse_error(e.to_string()))?;
            if file.schema != SCHEMA {
                return Err(parse_error(format!("unsupported schema {}", file.schema)));
            }
            entries.extend(file.entry);
        }
        let mut ids = BTreeSet::new();
        for entry in &entries {
            let invalid = |reason| DatabaseError::Invalid {
                id: entry.id.clone(),
                reason,
            };
            if !ids.insert(entry.id.as_str()) {
                return Err(invalid("duplicate id"));
            }
            if !entry.source.starts_with("https://") {
                return Err(invalid("source must be an https URL"));
            }
            if (entry.validation == Validation::Validated) == entry.tested_on.is_empty() {
                return Err(invalid(
                    "validated entries, and only those, must list tests",
                ));
            }
            if entry.support == Support::Unsupported
                && !(entry.packages.is_empty() && entry.modules.is_empty())
            {
                return Err(invalid(
                    "unsupported hardware cannot name packages or modules",
                ));
            }
            if entry.osreldate.is_some_and(|range| range.min > range.max) {
                return Err(invalid("the kernel version range is empty"));
            }
            if entry
                .matches
                .iter()
                .any(|m| m.vendor.is_none() && !m.devices.is_empty())
            {
                return Err(invalid("device identifiers need a vendor"));
            }
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Entries for a device, most specific first.
    ///
    /// When the kernel attached a specific driver, entries naming another driver
    /// do not apply. When it did not, every entry returned is only a candidate.
    pub fn lookup(&self, device: &Device) -> Vec<&Entry> {
        let mut found: Vec<(bool, usize, &Entry)> = self
            .entries
            .iter()
            .filter(|entry| {
                entry.driver.is_none()
                    || !device.has_specific_driver()
                    || entry.driver.as_deref() == device.driver_name()
            })
            .filter_map(|entry| {
                let specificity = entry
                    .matches
                    .iter()
                    .filter(|m| m.applies_to(device))
                    .map(Match::specificity)
                    .max()?;
                let attached =
                    entry.driver.is_some() && entry.driver.as_deref() == device.driver_name();
                Some((attached, specificity, entry))
            })
            .collect();
        found.sort_by(|a, b| {
            (b.0, b.1)
                .cmp(&(a.0, a.1))
                .then_with(|| a.2.id.cmp(&b.2.id))
        });
        found.into_iter().map(|(_, _, entry)| entry).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pci(vendor: u16, device: u16, class: u32, driver: Option<&str>) -> Device {
        Device {
            vendor,
            device,
            class,
            driver: driver.map(str::to_owned),
            ..Device::new(Bus::Pci, "pci0:0:0:0")
        }
    }

    #[test]
    fn builtin_database_is_valid() {
        assert!(Database::builtin().entries().len() > 10);
    }

    #[test]
    fn graphics_lookup_by_vendor() {
        let database = Database::builtin();
        let amd = database.lookup(&pci(0x1002, 0x73bf, 0x030000, Some("vgapci0")));
        assert_eq!(amd[0].id, "gpu-amd");
        let intel = database.lookup(&pci(0x8086, 0x9a49, 0x030000, Some("vgapci0")));
        assert_eq!(intel[0].id, "gpu-intel");
        // An Intel network card is not graphics.
        assert!(
            database
                .lookup(&pci(0x8086, 0x100e, 0x020000, Some("em0")))
                .iter()
                .all(|entry| entry.category != Category::Gpu)
        );
    }

    #[test]
    fn attached_driver_ranks_first() {
        let database = Database::builtin();
        let found = database.lookup(&pci(0x8086, 0x2723, 0x028000, Some("iwlwifi0")));
        assert_eq!(found[0].id, "wifi-intel-iwlwifi");
        let found = database.lookup(&pci(0x8086, 0x2723, 0x028000, Some("iwx0")));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "wifi-intel-iwx");
        // Unattached: every candidate is listed.
        assert!(database.lookup(&pci(0x8086, 0x2723, 0x028000, None)).len() > 1);
        // Attached to a driver the database does not know.
        assert!(
            database
                .lookup(&pci(0x8086, 0x1572, 0x020000, Some("ixl0")))
                .is_empty()
        );
    }

    #[test]
    fn unknown_hardware_has_no_entries() {
        assert!(
            Database::builtin()
                .lookup(&pci(0xdead, 0xbeef, 0xff0000, None))
                .is_empty()
        );
    }

    fn entry(extra: &str) -> Result<Database, DatabaseError> {
        let text = format!(
            "schema = 2\n[[entry]]\nid = \"x\"\nname = \"X\"\ncategory = \"gpu\"\n\
             support = \"native\"\nvalidation = \"documented\"\nsource = \"https://example.org\"\n{extra}"
        );
        Database::parse(&[("x.toml", &text)])
    }

    #[test]
    fn rules_are_enforced() {
        assert!(entry("").is_ok());
        assert!(entry("surprise = 1\n").is_err());
        let tested = "[[entry.tested_on]]\nrelease = \"r\"\nmachine = \"m\"\ndate = \"d\"\nresult = \"ok\"\n";
        assert!(
            entry(tested)
                .unwrap_err()
                .to_string()
                .contains("must list tests")
        );
        let devices = "[[entry.matches]]\nbus = \"pci\"\ndevices = [1]\n";
        assert!(
            entry(devices)
                .unwrap_err()
                .to_string()
                .contains("need a vendor")
        );
    }
}
