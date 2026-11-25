//! Release discovery and transport.
//!
//! Nothing here establishes trust. Everything a source returns is untrusted
//! until `metaupd-verify` has checked it.

mod directory;
mod fetch;
mod github;

use std::path::Path;

pub use directory::DirectorySource;
pub use fetch::{CommandFetcher, Fetcher};
pub use github::{GithubSource, OFFICIAL_REPOSITORY};
use metaupd_metadata::AssetName;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("{0}")]
    Transport(String),
    #[error("release {release} has no asset {asset}")]
    MissingAsset { release: String, asset: String },
    #[error("{what} is larger than {limit} bytes")]
    TooLarge { what: String, limit: u64 },
    #[error("unexpected response: {0}")]
    Malformed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A published release as described by an untrusted source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: AssetName,
    /// Locator understood by the source that produced it.
    pub location: String,
}

impl Release {
    pub fn asset(&self, name: &str) -> Result<&Asset, SourceError> {
        self.assets
            .iter()
            .find(|asset| asset.name.as_str() == name)
            .ok_or_else(|| SourceError::MissingAsset {
                release: self.tag.clone(),
                asset: name.to_owned(),
            })
    }
}

pub trait ReleaseSource {
    /// Human-readable description for diagnostics.
    fn describe(&self) -> String;

    /// Published releases, newest first as far as the source knows.
    fn releases(&self) -> Result<Vec<Release>, SourceError>;

    /// Fetches a small document into memory.
    fn fetch(&self, release: &Release, asset: &str, limit: u64) -> Result<Vec<u8>, SourceError>;

    /// Downloads an asset to `destination`, resuming a partial file if present.
    fn download(
        &self,
        release: &Release,
        asset: &str,
        destination: &Path,
        limit: u64,
    ) -> Result<(), SourceError>;
}
