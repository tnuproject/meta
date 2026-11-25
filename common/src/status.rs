//! Feature status labels and the component registry.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The only status labels META uses, in code and in documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "IMPLEMENTED")]
    Implemented,
    #[serde(rename = "EXPERIMENTAL")]
    Experimental,
    #[serde(rename = "IN DEVELOPMENT")]
    InDevelopment,
    #[serde(rename = "PLANNED")]
    Planned,
    #[serde(rename = "RESEARCH REQUIRED")]
    ResearchRequired,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Status::Implemented => "IMPLEMENTED",
            Status::Experimental => "EXPERIMENTAL",
            Status::InDevelopment => "IN DEVELOPMENT",
            Status::Planned => "PLANNED",
            Status::ResearchRequired => "RESEARCH REQUIRED",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: String,
    pub name: String,
    pub status: Status,
    pub summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    component: Vec<Component>,
}

/// Components listed in the repository's `status.toml`, embedded at build time.
pub fn components() -> Vec<Component> {
    let registry: Registry = toml::from_str(include_str!("../../status.toml"))
        .expect("status.toml is validated by the test suite");
    registry.component
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_parses_with_unique_ids() {
        let components = components();
        assert!(!components.is_empty());
        let mut ids: Vec<_> = components.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate component id");
    }

    #[test]
    fn serde_and_display_labels_agree() {
        for status in [
            Status::Implemented,
            Status::Experimental,
            Status::InDevelopment,
            Status::Planned,
            Status::ResearchRequired,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{status}\""));
        }
    }
}
