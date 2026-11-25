//! Release metadata formats for metaupd.
//!
//! These types describe untrusted input until `metaupd-verify` has checked the
//! signatures over the exact bytes they were parsed from. Parsing is strict:
//! unknown fields, unknown enum values and unsafe file names are errors.

pub mod asset;
pub mod manifest;
pub mod root;
pub mod timestamp;

pub use asset::AssetName;
pub use manifest::{
    Apply, Artifact, Category, Channel, Compatibility, Edition, Manifest, Migration, Osreldate,
};
pub use root::{PublicKey, Role, RoleSpec, RootMetadata};
pub use timestamp::Timestamp;

/// Upper bound for any metadata document, enforced before parsing.
pub const MAX_METADATA_SIZE: u64 = 4 * 1024 * 1024;

pub const MANIFEST_ASSET: &str = "meta-manifest.json";
pub const MANIFEST_SIGNATURE_ASSET: &str = "meta-manifest.sig.json";

/// Asset names for version `version` of the root metadata and its signatures.
pub fn root_assets(version: u32) -> (String, String) {
    (
        format!("meta-root-{version}.json"),
        format!("meta-root-{version}.sig.json"),
    )
}

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("malformed {what}: {source}")]
    Json {
        what: &'static str,
        source: serde_json::Error,
    },
    #[error("invalid {what}: {}", problems.join("; "))]
    Invalid {
        what: &'static str,
        problems: Vec<String>,
    },
}

/// SHA-256 digest with a canonical lowercase hexadecimal representation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Digest(pub [u8; 32]);

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl std::fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sha256Digest({self})")
    }
}

impl std::str::FromStr for Sha256Digest {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        decode_lower_hex(text)
            .map(Self)
            .ok_or_else(|| "expected 64 lowercase hex digits".into())
    }
}

impl serde::Serialize for Sha256Digest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Sha256Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Decodes 32 bytes of hex, rejecting uppercase so encodings are unique.
pub(crate) fn decode_lower_hex(text: &str) -> Option<[u8; 32]> {
    if text.bytes().any(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(text, &mut out).ok()?;
    Some(out)
}
