//! Root metadata: which keys may sign what.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{MetadataError, Sha256Digest, Timestamp, decode_lower_hex};

pub const ROOT_SCHEMA: u32 = 1;
pub const ROOT_KIND: &str = "meta-root";

/// A signing role. Each role has its own signature domain, so a signature made
/// for one kind of document is never valid for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Root,
    Release,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Root => "root",
            Role::Release => "release",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An Ed25519 public key in lowercase hexadecimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PublicKey(pub [u8; 32]);

impl TryFrom<String> for PublicKey {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        decode_lower_hex(&value)
            .map(Self)
            .ok_or_else(|| "expected an Ed25519 public key as 64 lowercase hex digits".into())
    }
}

impl From<PublicKey> for String {
    fn from(value: PublicKey) -> Self {
        hex::encode(value.0)
    }
}

/// Key identifier: the SHA-256 digest of the raw public key.
pub type KeyId = Sha256Digest;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSpec {
    pub threshold: u32,
    pub keyids: BTreeSet<KeyId>,
}

/// The trust anchor. Version N+1 must be signed by a threshold of the root keys
/// of version N and by a threshold of its own root keys; that is how keys are
/// rotated, and removing a key from the next version is how it is revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootMetadata {
    pub schema: u32,
    pub kind: String,
    pub project: String,
    pub version: u32,
    pub created: Timestamp,
    pub expires: Timestamp,
    pub keys: BTreeMap<KeyId, PublicKey>,
    pub roles: BTreeMap<Role, RoleSpec>,
}

impl RootMetadata {
    pub fn parse(bytes: &[u8]) -> Result<Self, MetadataError> {
        let root: Self = serde_json::from_slice(bytes).map_err(|source| MetadataError::Json {
            what: "root metadata",
            source,
        })?;
        root.validate()?;
        Ok(root)
    }

    /// Structural checks. Key identifiers are checked against key material by
    /// `metaupd-verify`, which owns the hash implementation.
    pub fn validate(&self) -> Result<(), MetadataError> {
        let mut problems = Vec::new();
        if self.schema != ROOT_SCHEMA {
            problems.push(format!("unsupported schema {}", self.schema));
        }
        if self.kind != ROOT_KIND {
            problems.push(format!("unexpected kind {:?}", self.kind));
        }
        if self.project != meta_common::PROJECT {
            problems.push(format!("unexpected project {:?}", self.project));
        }
        if self.version == 0 {
            problems.push("version must be at least 1".into());
        }
        if self.expires <= self.created {
            problems.push("expires must be later than created".into());
        }
        for role in [Role::Root, Role::Release] {
            match self.roles.get(&role) {
                None => problems.push(format!("role {role} is missing")),
                Some(spec) => {
                    if spec.threshold == 0 {
                        problems.push(format!("role {role} has threshold 0"));
                    }
                    if (spec.threshold as usize) > spec.keyids.len() {
                        problems.push(format!("role {role} threshold exceeds its key count"));
                    }
                    for id in spec.keyids.iter().filter(|id| !self.keys.contains_key(id)) {
                        problems.push(format!("role {role} names unknown key {id}"));
                    }
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(MetadataError::Invalid {
                what: "root metadata",
                problems,
            })
        }
    }
}
