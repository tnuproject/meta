//! Verification of signed metaupd metadata.
//!
//! The types here are evidence: a [`VerifiedManifest`] can only be obtained by
//! checking a threshold of signatures against a [`TrustedRoot`], so code that
//! downloads or applies artifacts takes one and cannot skip verification.

mod hash;
mod policy;
mod signature;
mod trust;

pub use hash::{HashMismatch, HashingReader, sha256, verify_file};
pub use policy::{
    HighWater, Host, Incompatibility, SequenceError, check_compatibility, check_sequence,
};
pub use signature::{SignatureEntry, SignatureFile, key_id, signing_message};
pub use trust::{TrustError, TrustedRoot, VerifiedManifest};
