//! Loading and rotating the trust anchor.
//!
//! The anchor is installed with the system. Later root versions are stored in
//! the state directory and re-verified from the anchor on every load, so those
//! files are evidence rather than something to trust.

use metaupd_metadata::{MAX_METADATA_SIZE, root_assets};
use metaupd_verify::{SignatureFile, TrustError, TrustedRoot};

use crate::store::{
    Paths, StoreError, check_not_shared, ensure_private_dir, read_limited, write_atomic,
};

/// Bounds the rotation walk.
const MAX_ROTATIONS: u32 = 1024;

#[derive(Debug, thiserror::Error)]
pub enum TrustStoreError {
    #[error(
        "no trusted META root metadata is installed in {0}; updates cannot be verified. \
         No official META signing keys have been published yet"
    )]
    NoAnchor(String),
    #[error(
        "stored root metadata is at version {found}, but version {expected} was trusted before"
    )]
    ChainTruncated { expected: u32, found: u32 },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Trust(#[from] TrustError),
}

/// Loads the newest trusted root, which must be at least version `minimum`.
pub fn load(paths: &Paths, minimum: u32) -> Result<TrustedRoot, TrustStoreError> {
    let missing = || TrustStoreError::NoAnchor(paths.trust_dir.display().to_string());
    let root_path = paths.trust_dir.join("root.json");
    let signature_path = paths.trust_dir.join("root.sig.json");
    let bytes = read_limited(&root_path, MAX_METADATA_SIZE)?.ok_or_else(missing)?;
    let signatures = read_limited(&signature_path, MAX_METADATA_SIZE)?.ok_or_else(missing)?;
    for path in [paths.trust_dir.as_path(), &root_path, &signature_path] {
        check_not_shared(path)?;
    }
    let mut root = TrustedRoot::from_anchor(&bytes, &parse_signatures(&signatures)?)?;
    for _ in 0..MAX_ROTATIONS {
        let (root_name, signature_name) = root_assets(root.version() + 1);
        let directory = paths.root_chain();
        let next = read_limited(&directory.join(root_name), MAX_METADATA_SIZE)?;
        let signatures = read_limited(&directory.join(signature_name), MAX_METADATA_SIZE)?;
        let (Some(next), Some(signatures)) = (next, signatures) else {
            break;
        };
        root = root.rotate(&next, &parse_signatures(&signatures)?)?;
    }
    if root.version() < minimum {
        return Err(TrustStoreError::ChainTruncated {
            expected: minimum,
            found: root.version(),
        });
    }
    Ok(root)
}

/// Verifies the next root version and stores it.
pub fn advance(
    paths: &Paths,
    current: &TrustedRoot,
    bytes: &[u8],
    signatures: &[u8],
) -> Result<TrustedRoot, TrustStoreError> {
    let next = current.rotate(bytes, &parse_signatures(signatures)?)?;
    let directory = paths.root_chain();
    ensure_private_dir(&directory)?;
    let (root_name, signature_name) = root_assets(next.version());
    write_atomic(&directory.join(root_name), bytes)?;
    write_atomic(&directory.join(signature_name), signatures)?;
    Ok(next)
}

fn parse_signatures(bytes: &[u8]) -> Result<SignatureFile, TrustError> {
    Ok(SignatureFile::parse(bytes)?)
}
