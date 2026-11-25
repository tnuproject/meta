//! Compatibility database: known limitations and workarounds.

use serde::{Deserialize, Serialize};

use crate::elf::Binary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Workaround,
    Blocking,
}

/// Where a claim comes from. Claims without a source are not accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "kebab-case")]
pub enum Evidence {
    /// Stated by upstream documentation.
    Documentation { url: String },
    /// Observed by the META project.
    Tested { on: String, date: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advice {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
    pub evidence: Evidence,
    #[serde(default)]
    pub matches: Matches,
}

/// An entry applies when any listed program or library matches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Matches {
    /// Exact file names.
    pub programs: Vec<String>,
    /// Prefixes of needed library names.
    pub libraries: Vec<String>,
    /// Any AppImage.
    pub appimage: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
    pub schema: u32,
    #[serde(default)]
    pub entry: Vec<Advice>,
}

impl Database {
    pub fn builtin() -> Self {
        Self::parse(include_str!("../data/compatibility.toml"))
            .expect("the embedded database is validated by the test suite")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let database: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        if database.schema != 1 {
            return Err(format!("unsupported schema {}", database.schema));
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &database.entry {
            if !ids.insert(&entry.id) {
                return Err(format!("duplicate entry {}", entry.id));
            }
            let m = &entry.matches;
            if m.programs.is_empty() && m.libraries.is_empty() && !m.appimage {
                return Err(format!("entry {} matches nothing", entry.id));
            }
        }
        Ok(database)
    }

    pub fn advice_for(&self, program: &str, binary: &Binary) -> Vec<&Advice> {
        self.entry
            .iter()
            .filter(|entry| {
                let m = &entry.matches;
                m.programs.iter().any(|name| name == program)
                    || m.libraries
                        .iter()
                        .any(|prefix| binary.needed.iter().any(|lib| lib.starts_with(prefix)))
                    || (m.appimage && binary.appimage.is_some())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_database_is_valid() {
        assert!(!Database::builtin().entry.is_empty());
    }

    #[test]
    fn entries_need_evidence_and_a_match() {
        let without_evidence = "schema = 1\n[[entry]]\nid = \"x\"\nseverity = \"info\"\nsummary = \"s\"\n[entry.matches]\nappimage = true\n";
        assert!(Database::parse(without_evidence).is_err());
        let without_match = "schema = 1\n[[entry]]\nid = \"x\"\nseverity = \"info\"\nsummary = \"s\"\nevidence = { kind = \"documentation\", url = \"u\" }\n";
        assert!(
            Database::parse(without_match)
                .unwrap_err()
                .contains("matches nothing")
        );
    }
}
