//! Why something is believed.

use serde::{Deserialize, Serialize};

/// Where a claim comes from, weakest first. The order is used to rank evidence
/// about the same thing; it is never added up into a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A text found in a file, which may never be used.
    Heuristic,
    /// Inferred from the structure of a program or a driver's source.
    StaticAnalysis,
    /// Stated by the program's author or packager.
    Declared,
    /// An entry in META's databases, with its own reference.
    Database,
    /// Upstream documentation.
    Documentation,
    /// The source code of the system providing it.
    SourceCode,
    /// Observed while running.
    Observation,
    /// A recorded test.
    Test,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source: Source,
    pub detail: String,
    /// File, URL or test that can be looked up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl Evidence {
    pub fn new(source: Source, detail: impl Into<String>) -> Self {
        Self {
            source,
            detail: detail.into(),
            reference: None,
        }
    }

    #[must_use]
    pub fn at(mut self, reference: impl Into<String>) -> Self {
        self.reference = Some(reference.into());
        self
    }
}

impl std::fmt::Display for Evidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)?;
        match &self.reference {
            Some(reference) => write!(f, " ({reference})"),
            None => Ok(()),
        }
    }
}
