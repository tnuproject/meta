//! metaupd engine: the single backend every interface uses.

pub mod apply;
pub mod engine;
pub mod history;
pub mod plan;
pub mod state;
pub mod store;
pub mod trust_store;

pub use engine::{CheckReport, Engine, EngineError, Offer, StagedRelease};

use meta_common::MetaVersion;

/// Version of this metaupd, compared with `min_metaupd_version` in manifests.
pub const METAUPD_VERSION: &str = "26.0-alpha0";

pub fn metaupd_version() -> MetaVersion {
    METAUPD_VERSION
        .parse()
        .expect("METAUPD_VERSION is covered by a test")
}

#[cfg(test)]
mod tests {
    #[test]
    fn own_version_parses() {
        assert_eq!(super::metaupd_version().to_string(), super::METAUPD_VERSION);
    }
}
