//! Downgrade protection and compatibility checks.

use meta_common::MetaVersion;
use metaupd_metadata::{Manifest, Sha256Digest};
use serde::{Deserialize, Serialize};

use crate::trust::VerifiedManifest;

/// Highest manifest seen on a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HighWater {
    pub sequence: u64,
    pub digest: Sha256Digest,
}

impl HighWater {
    pub fn of(manifest: &VerifiedManifest) -> Self {
        Self {
            sequence: manifest.manifest().sequence,
            digest: manifest.digest(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SequenceError {
    #[error("sequence {offered} is older than {seen}, which this system has already seen")]
    Rollback { seen: u64, offered: u64 },
    #[error("sequence {0} was seen before with different contents")]
    Equivocation(u64),
}

/// Rejects manifests older than, or conflicting with, what was seen before.
pub fn check_sequence(
    seen: Option<&HighWater>,
    manifest: &VerifiedManifest,
) -> Result<(), SequenceError> {
    let Some(seen) = seen else { return Ok(()) };
    let offered = manifest.manifest().sequence;
    if offered < seen.sequence {
        Err(SequenceError::Rollback {
            seen: seen.sequence,
            offered,
        })
    } else if offered == seen.sequence && manifest.digest() != seen.digest {
        Err(SequenceError::Equivocation(offered))
    } else {
        Ok(())
    }
}

/// Facts about the running system that decide compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub arch: String,
    pub osreldate: Option<u32>,
    pub meta_version: Option<MetaVersion>,
    pub metaupd_version: MetaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Incompatibility {
    #[error("this is not an installed META system")]
    NotMeta,
    #[error("architecture {0} is not supported by this release")]
    Arch(String),
    #[error("kernel version is unknown; FreeBSD is required")]
    UnknownKernel,
    #[error("kernel version {found} is outside the supported range {min}-{max}")]
    Kernel { found: u32, min: u32, max: u32 },
    #[error("update to META {0} first")]
    MetaTooOld(MetaVersion),
    #[error("metaupd {0} or newer is required")]
    MetaupdTooOld(MetaVersion),
    #[error("META {offered} is not newer than the installed {installed}")]
    NotNewer {
        installed: MetaVersion,
        offered: MetaVersion,
    },
}

/// Returns every reason the release cannot be installed on `host`.
pub fn check_compatibility(manifest: &Manifest, host: &Host) -> Vec<Incompatibility> {
    let required = &manifest.compatibility;
    let mut reasons = Vec::new();
    match &host.meta_version {
        None => reasons.push(Incompatibility::NotMeta),
        Some(installed) => {
            if *installed < required.min_meta_version {
                reasons.push(Incompatibility::MetaTooOld(
                    required.min_meta_version.clone(),
                ));
            }
            if manifest.version <= *installed {
                reasons.push(Incompatibility::NotNewer {
                    installed: installed.clone(),
                    offered: manifest.version.clone(),
                });
            }
        }
    }
    if !required.arch.contains(&host.arch) {
        reasons.push(Incompatibility::Arch(host.arch.clone()));
    }
    match host.osreldate {
        None => reasons.push(Incompatibility::UnknownKernel),
        Some(found) if found < required.osreldate.min || found > required.osreldate.max => {
            reasons.push(Incompatibility::Kernel {
                found,
                min: required.osreldate.min,
                max: required.osreldate.max,
            });
        }
        Some(_) => {}
    }
    if host.metaupd_version < required.min_metaupd_version {
        reasons.push(Incompatibility::MetaupdTooOld(
            required.min_metaupd_version.clone(),
        ));
    }
    reasons
}
