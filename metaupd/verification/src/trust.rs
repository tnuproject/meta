//! The trust anchor and verification against it.

use std::collections::BTreeSet;

use metaupd_metadata::root::KeyId;
use metaupd_metadata::{Manifest, MetadataError, Role, RootMetadata, Sha256Digest, Timestamp};

use crate::hash::sha256;
use crate::signature::{SignatureFile, key_id, signature_is_valid, signing_message};

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error(transparent)]
    Metadata(#[from] MetadataError),
    #[error("{role} signature threshold not met: {valid} of {needed} trusted signatures")]
    Threshold {
        role: Role,
        valid: usize,
        needed: u32,
    },
    #[error("signature file is for role {found}, expected {expected}")]
    WrongRole { expected: Role, found: Role },
    #[error("key {0} does not match its identifier")]
    KeyIdMismatch(KeyId),
    #[error("root rotation must go from version {from} to {}, found {found}", from + 1)]
    RootVersion { from: u32, found: u32 },
    #[error("root metadata version {version} expired at {expires}")]
    RootExpired { version: u32, expires: Timestamp },
    #[error("release metadata expired at {expires}; the source may be withholding updates")]
    ManifestExpired { expires: Timestamp },
    #[error("release metadata is dated {created}, in the future; check the system clock")]
    ManifestFromFuture { created: Timestamp },
}

/// Root metadata that has been accepted as a trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedRoot {
    metadata: RootMetadata,
}

/// A manifest whose signatures, structure and dates have been checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedManifest {
    manifest: Manifest,
    digest: Sha256Digest,
}

impl VerifiedManifest {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Digest of the exact signed bytes.
    pub fn digest(&self) -> Sha256Digest {
        self.digest
    }
}

impl TrustedRoot {
    /// Accepts root metadata obtained out of band (installed with the system).
    ///
    /// The document must still be self-consistent and signed by a threshold of
    /// its own root keys, which catches corruption and hand edits.
    pub fn from_anchor(bytes: &[u8], signatures: &SignatureFile) -> Result<Self, TrustError> {
        let metadata = parse_root(bytes)?;
        check_threshold(&metadata, Role::Root, bytes, signatures)?;
        Ok(Self { metadata })
    }

    pub fn metadata(&self) -> &RootMetadata {
        &self.metadata
    }

    pub fn version(&self) -> u32 {
        self.metadata.version
    }

    /// Moves trust to the next root version.
    ///
    /// Expiry is not checked here: a client that was offline for a long time
    /// must be able to walk through expired intermediate versions. It is
    /// checked when the final root is used, by [`Self::verify_manifest`].
    pub fn rotate(&self, bytes: &[u8], signatures: &SignatureFile) -> Result<Self, TrustError> {
        let next = parse_root(bytes)?;
        if Some(next.version) != self.metadata.version.checked_add(1) {
            return Err(TrustError::RootVersion {
                from: self.metadata.version,
                found: next.version,
            });
        }
        check_threshold(&self.metadata, Role::Root, bytes, signatures)?;
        check_threshold(&next, Role::Root, bytes, signatures)?;
        Ok(Self { metadata: next })
    }

    /// Verifies a release manifest. Signatures are checked before parsing.
    pub fn verify_manifest(
        &self,
        bytes: &[u8],
        signatures: &SignatureFile,
        now: Timestamp,
    ) -> Result<VerifiedManifest, TrustError> {
        if now >= self.metadata.expires {
            return Err(TrustError::RootExpired {
                version: self.metadata.version,
                expires: self.metadata.expires,
            });
        }
        check_threshold(&self.metadata, Role::Release, bytes, signatures)?;
        let manifest = Manifest::parse(bytes)?;
        if now >= manifest.expires {
            return Err(TrustError::ManifestExpired {
                expires: manifest.expires,
            });
        }
        // Allows a day of clock skew.
        if manifest.created > now.plus_days(1) {
            return Err(TrustError::ManifestFromFuture {
                created: manifest.created,
            });
        }
        Ok(VerifiedManifest {
            manifest,
            digest: sha256(bytes),
        })
    }
}

fn parse_root(bytes: &[u8]) -> Result<RootMetadata, TrustError> {
    let metadata = RootMetadata::parse(bytes)?;
    for (id, key) in &metadata.keys {
        if key_id(key) != *id {
            return Err(TrustError::KeyIdMismatch(*id));
        }
    }
    Ok(metadata)
}

fn check_threshold(
    root: &RootMetadata,
    role: Role,
    payload: &[u8],
    signatures: &SignatureFile,
) -> Result<(), TrustError> {
    if signatures.role != role {
        return Err(TrustError::WrongRole {
            expected: role,
            found: signatures.role,
        });
    }
    let Some(spec) = root.roles.get(&role) else {
        return Err(TrustError::Threshold {
            role,
            valid: 0,
            needed: 1,
        });
    };
    let message = signing_message(role, payload);
    // A set, so one key signing repeatedly counts once. Distinct identifiers
    // imply distinct keys because identifiers are digests of the key.
    let valid: BTreeSet<KeyId> = signatures
        .signatures
        .iter()
        .filter(|entry| spec.keyids.contains(&entry.keyid))
        .filter(|entry| {
            root.keys
                .get(&entry.keyid)
                .is_some_and(|key| signature_is_valid(key, &message, &entry.sig))
        })
        .map(|entry| entry.keyid)
        .collect();
    if valid.len() >= spec.threshold as usize && spec.threshold > 0 {
        Ok(())
    } else {
        Err(TrustError::Threshold {
            role,
            valid: valid.len(),
            needed: spec.threshold,
        })
    }
}
