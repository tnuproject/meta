//! What LinuxKPI offers, from FreeBSD's source.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    area: String,
    #[serde(default)]
    functions: Vec<String>,
    #[serde(default)]
    unfinished: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlaceholderHeaders {
    names: BTreeSet<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    release: String,
    header: BTreeMap<String, Header>,
    placeholder_headers: PlaceholderHeaders,
}

/// What to do about an interface LinuxKPI does not provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    /// FreeBSD has the same thing under another name.
    FreebsdEquivalent,
    /// A safe implementation belongs in LinuxKPI.
    LinuxkpiImplementation,
    /// The driver can avoid it.
    DriverAdaptation,
    /// Large enough to need FreeBSD developers.
    UpstreamContribution,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advice {
    /// Exact name, or a prefix ending in `*`.
    pub interface: String,
    pub strategy: Strategy,
    pub detail: String,
    pub reference: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdviceFile {
    schema: u32,
    #[serde(default)]
    advice: Vec<Advice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum InterfaceState {
    /// Declared, with no note that it is incomplete. Behaviour is not implied.
    Declared {
        header: String,
        area: String,
    },
    /// Declared, but its own source says that it is unfinished.
    Unfinished {
        header: String,
        area: String,
    },
    Missing,
}

#[derive(Debug, Clone)]
pub struct KpiRegistry {
    pub release: String,
    names: BTreeMap<String, InterfaceState>,
    placeholder_headers: BTreeSet<String>,
    headers: BTreeSet<String>,
    advice: Vec<Advice>,
}

impl KpiRegistry {
    pub fn builtin() -> Self {
        Self::parse(
            include_str!("../../../linuxkpi/registry/15.1.toml"),
            include_str!("../../../linuxkpi/registry/advice.toml"),
        )
        .expect("the embedded registry is validated by the test suite")
    }

    pub fn parse(registry: &str, advice: &str) -> Result<Self, String> {
        let file: File = toml::from_str(registry).map_err(|e| e.to_string())?;
        let advice: AdviceFile = toml::from_str(advice).map_err(|e| e.to_string())?;
        if file.schema != 1 || advice.schema != 1 {
            return Err("unsupported schema".into());
        }
        let mut names = BTreeMap::new();
        for (header, contents) in &file.header {
            let (header, area) = (header.clone(), contents.area.clone());
            for name in &contents.functions {
                names
                    .entry(name.clone())
                    .or_insert_with(|| InterfaceState::Declared {
                        header: header.clone(),
                        area: area.clone(),
                    });
            }
            // Unfinished anywhere is unfinished.
            for name in &contents.unfinished {
                names.insert(
                    name.clone(),
                    InterfaceState::Unfinished {
                        header: header.clone(),
                        area: area.clone(),
                    },
                );
            }
        }
        Ok(Self {
            release: file.release,
            names,
            headers: file.header.into_keys().collect(),
            placeholder_headers: file.placeholder_headers.names,
            advice: advice.advice,
        })
    }

    pub fn state(&self, name: &str) -> InterfaceState {
        self.names
            .get(name)
            .cloned()
            .unwrap_or(InterfaceState::Missing)
    }

    /// `Some(false)`: the header exists only so that drivers compile.
    pub fn header(&self, name: &str) -> Option<bool> {
        if self.headers.contains(name) {
            Some(true)
        } else if self.placeholder_headers.contains(name) {
            Some(false)
        } else {
            None
        }
    }

    pub fn advice(&self, name: &str) -> Option<&Advice> {
        self.advice
            .iter()
            .find(|advice| match advice.interface.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == advice.interface,
            })
    }

    /// Declared and unfinished interfaces per area.
    pub fn areas(&self) -> BTreeMap<String, (usize, usize)> {
        let mut areas: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for state in self.names.values() {
            match state {
                InterfaceState::Declared { area, .. } => {
                    areas.entry(area.clone()).or_default().0 += 1;
                }
                InterfaceState::Unfinished { area, .. } => {
                    areas.entry(area.clone()).or_default().1 += 1;
                }
                InterfaceState::Missing => {}
            }
        }
        areas
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_registry_reflects_the_source() {
        let registry = KpiRegistry::builtin();
        assert!(matches!(
            registry.state("kmalloc"),
            InterfaceState::Declared { .. }
        ));
        assert!(matches!(
            registry.state("mhi_sync_power_up"),
            InterfaceState::Unfinished { .. }
        ));
        assert_eq!(registry.state("iommu_map"), InterfaceState::Missing);
        assert!(registry.areas().len() > 8);
    }

    #[test]
    fn advice_matches_names_and_prefixes() {
        let registry = KpiRegistry::builtin();
        assert_eq!(
            registry.advice("mhi_sync_power_up").map(|a| a.strategy),
            Some(Strategy::UpstreamContribution)
        );
        assert!(registry.advice("no_such_thing").is_none());
    }

    #[test]
    fn unfinished_wins_over_declared() {
        let text = "schema = 1\nrelease = \"r\"\n[header.\"a.h\"]\narea = \"x\"\nfunctions = [\"f\"]\n\
                    [header.\"b.h\"]\narea = \"x\"\nfunctions = []\nunfinished = [\"f\"]\n\
                    [placeholder_headers]\nnames = [\"c.h\"]\n";
        let registry = KpiRegistry::parse(text, "schema = 1\n").unwrap();
        assert!(matches!(
            registry.state("f"),
            InterfaceState::Unfinished { .. }
        ));
        assert_eq!(registry.header("a.h"), Some(true));
        assert_eq!(registry.header("c.h"), Some(false));
        assert_eq!(registry.header("d.h"), None);
    }
}
