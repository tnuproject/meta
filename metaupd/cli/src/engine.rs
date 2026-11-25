//! Checking, downloading and planning.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use meta_common::{CommandRunner, MetaVersion, Platform};
use metaupd_github::{Release, ReleaseSource, SourceError};
use metaupd_metadata::{
    Channel, Edition, MANIFEST_ASSET, MANIFEST_SIGNATURE_ASSET, MAX_METADATA_SIZE, Timestamp,
    root_assets,
};
use metaupd_rollback::{BootEnvironment, RollbackError};
use metaupd_verify::{
    HashMismatch, HighWater, Host, Incompatibility, SequenceError, SignatureFile, TrustError,
    TrustedRoot, VerifiedManifest, check_compatibility, check_sequence, verify_file,
};

use crate::history::{self, Action};
use crate::plan::{self, Plan};
use crate::state::State;
use crate::store::{
    Lock, Paths, StoreError, ensure_private_dir, io_at, read_limited, write_atomic,
};
use crate::trust_store::{self, TrustStoreError};

/// Releases examined per check.
const MAX_RELEASES: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    TrustStore(#[from] TrustStoreError),
    #[error(transparent)]
    Trust(#[from] TrustError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error(transparent)]
    Hash(#[from] HashMismatch),
    #[error(transparent)]
    Sequence(#[from] SequenceError),
    #[error(transparent)]
    Rollback(#[from] RollbackError),
    #[error(transparent)]
    Apply(#[from] crate::apply::ApplyError),
    #[error("no update is available")]
    NoUpdate,
    #[error("META {0} has not been downloaded; run `metaupd download`")]
    NotStaged(String),
    #[error("invalid version {0:?}")]
    InvalidVersion(String),
    #[error("this release cannot be installed here: {}", join(.0))]
    Incompatible(Vec<Incompatibility>),
    #[error("{0}")]
    Unsupported(String),
}

fn join(reasons: &[Incompatibility]) -> String {
    reasons
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// A verified release that could be downloaded.
#[derive(Debug, Clone)]
pub struct Offer {
    pub release: Release,
    pub manifest: VerifiedManifest,
    manifest_bytes: Vec<u8>,
    signature_bytes: Vec<u8>,
    pub incompatibilities: Vec<Incompatibility>,
}

#[derive(Debug, Default)]
pub struct CheckReport {
    pub offer: Option<Offer>,
    /// Releases that failed verification, with the reason.
    pub rejected: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

/// A fully downloaded release, verified again from disk.
#[derive(Debug)]
pub struct StagedRelease {
    pub manifest: VerifiedManifest,
    pub directory: PathBuf,
    pub incompatibilities: Vec<Incompatibility>,
}

pub struct Engine<'a> {
    pub paths: Paths,
    pub source: &'a dyn ReleaseSource,
    pub runner: &'a dyn CommandRunner,
    pub now: Timestamp,
    pub host: Host,
    pub edition: Option<Edition>,
    /// Refuse to install when the state would be reverted by a rollback. Always
    /// set outside tests.
    pub require_separate_state: bool,
}

impl std::fmt::Debug for Engine<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("paths", &self.paths)
            .field("host", &self.host)
            .finish()
    }
}

/// Derives update-relevant facts from the running platform.
pub fn host_from_platform(platform: &Platform) -> (Host, Option<Edition>) {
    let release = platform.meta_release.as_ref();
    let host = Host {
        arch: platform.arch.clone(),
        osreldate: platform.osreldate,
        meta_version: release.and_then(|r| r.version.parse::<MetaVersion>().ok()),
        metaupd_version: crate::metaupd_version(),
    };
    let edition = match release.and_then(|r| r.edition.as_deref()) {
        Some("debian") => Some(Edition::Debian),
        Some("arch") => Some(Edition::Arch),
        _ => None,
    };
    (host, edition)
}

impl Engine<'_> {
    /// Looks for the newest acceptable release and records what was seen.
    pub fn check(&self) -> Result<CheckReport, EngineError> {
        let _lock = Lock::acquire(&self.paths.state_dir)?;
        self.check_locked()
    }

    fn check_locked(&self) -> Result<CheckReport, EngineError> {
        let mut state = State::load(&self.paths)?;
        let mut root = trust_store::load(&self.paths, state.root_version)?;
        let releases = self.source.releases()?;
        let mut report = CheckReport::default();

        root = self.rotate_root(root, &releases, &mut report)?;
        state.root_version = root.version();

        let mut offers = Vec::new();
        let candidates = releases
            .into_iter()
            .filter(|release| release.asset(MANIFEST_ASSET).is_ok())
            .take(MAX_RELEASES);
        for release in candidates {
            match self.verify_release(&root, &release) {
                Ok(offer) => offers.push(offer),
                Err(error) => report.rejected.push((release.tag, error.to_string())),
            }
        }
        offers.retain(|offer| state.channel.accepts(offer.manifest.manifest().channel));

        let newest = self.enforce_sequences(&mut offers, &state, &mut report);
        state.high_water.extend(newest);
        state.last_check = Some(self.now);
        state.save(&self.paths)?;

        // A system that is already current has nothing to do, so only newer
        // versions are offered. Unknown installed versions see everything.
        if let Some(installed) = &self.host.meta_version {
            offers.retain(|offer| offer.manifest.manifest().version > *installed);
        }
        report.offer = offers.into_iter().max_by(|a, b| {
            let (a, b) = (a.manifest.manifest(), b.manifest.manifest());
            a.version.cmp(&b.version).then(a.sequence.cmp(&b.sequence))
        });
        Ok(report)
    }

    /// Follows root rotations published with any release.
    fn rotate_root(
        &self,
        mut root: TrustedRoot,
        releases: &[Release],
        report: &mut CheckReport,
    ) -> Result<TrustedRoot, EngineError> {
        loop {
            let (root_name, signature_name) = root_assets(root.version() + 1);
            let Some(release) = releases
                .iter()
                .find(|r| r.asset(&root_name).is_ok() && r.asset(&signature_name).is_ok())
            else {
                return Ok(root);
            };
            let bytes = self.source.fetch(release, &root_name, MAX_METADATA_SIZE)?;
            let signatures = self
                .source
                .fetch(release, &signature_name, MAX_METADATA_SIZE)?;
            match trust_store::advance(&self.paths, &root, &bytes, &signatures) {
                Ok(next) => {
                    let detail = format!("root metadata {} to {}", root.version(), next.version());
                    history::append(&self.paths, self.now, Action::RootRotation, true, detail)?;
                    root = next;
                }
                Err(TrustStoreError::Trust(error)) => {
                    report
                        .warnings
                        .push(format!("ignored {root_name}: {error}"));
                    return Ok(root);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn verify_release(&self, root: &TrustedRoot, release: &Release) -> Result<Offer, EngineError> {
        let manifest_bytes = self
            .source
            .fetch(release, MANIFEST_ASSET, MAX_METADATA_SIZE)?;
        let signature_bytes =
            self.source
                .fetch(release, MANIFEST_SIGNATURE_ASSET, MAX_METADATA_SIZE)?;
        let signatures = SignatureFile::parse(&signature_bytes).map_err(TrustError::from)?;
        let manifest = root.verify_manifest(&manifest_bytes, &signatures, self.now)?;
        let incompatibilities = check_compatibility(manifest.manifest(), &self.host);
        Ok(Offer {
            release: release.clone(),
            manifest,
            manifest_bytes,
            signature_bytes,
            incompatibilities,
        })
    }

    /// Drops releases that fail rollback protection and returns the new
    /// high-water marks.
    fn enforce_sequences(
        &self,
        offers: &mut Vec<Offer>,
        state: &State,
        report: &mut CheckReport,
    ) -> BTreeMap<Channel, HighWater> {
        // Two different manifests with one sequence number in the same listing
        // are equivocation too; neither can be trusted.
        let mut by_sequence: BTreeMap<(Channel, u64), Vec<HighWater>> = BTreeMap::new();
        for offer in offers.iter() {
            let manifest = offer.manifest.manifest();
            let marks = by_sequence
                .entry((manifest.channel, manifest.sequence))
                .or_default();
            let mark = HighWater::of(&offer.manifest);
            if !marks.contains(&mark) {
                marks.push(mark);
            }
        }
        offers.retain(|offer| {
            let manifest = offer.manifest.manifest();
            let duplicated = by_sequence
                .get(&(manifest.channel, manifest.sequence))
                .is_some_and(|marks| marks.len() > 1);
            let result = if duplicated {
                Err(SequenceError::Equivocation(manifest.sequence))
            } else {
                check_sequence(state.high_water.get(&manifest.channel), &offer.manifest)
            };
            match result {
                Ok(()) => true,
                // Older releases stay listed on GitHub; that alone is normal.
                Err(SequenceError::Rollback { .. }) => false,
                Err(error) => {
                    report
                        .rejected
                        .push((offer.release.tag.clone(), error.to_string()));
                    false
                }
            }
        });
        for (channel, seen) in &state.high_water {
            let offered = offers
                .iter()
                .any(|o| o.manifest.manifest().channel == *channel);
            if state.channel.accepts(*channel) && !offered {
                report.warnings.push(format!(
                    "{} previously offered sequence {} on {channel} but no longer does; \
                     updates may be being withheld",
                    self.source.describe(),
                    seen.sequence
                ));
            }
        }
        let mut newest: BTreeMap<Channel, HighWater> = BTreeMap::new();
        for offer in offers.iter() {
            let mark = HighWater::of(&offer.manifest);
            let entry = newest
                .entry(offer.manifest.manifest().channel)
                .or_insert(mark);
            if mark.sequence > entry.sequence {
                *entry = mark;
            }
        }
        newest
    }

    /// Downloads the offered release into the staging area.
    pub fn download(&self) -> Result<(Offer, PathBuf), EngineError> {
        let _lock = Lock::acquire(&self.paths.state_dir)?;
        let offer = self.check_locked()?.offer.ok_or(EngineError::NoUpdate)?;
        let version = offer.manifest.manifest().version.to_string();
        let result = self.stage(&offer);
        let detail = match &result {
            Ok(_) => format!("META {version}"),
            Err(error) => format!("META {version}: {error}"),
        };
        history::append(
            &self.paths,
            self.now,
            Action::Download,
            result.is_ok(),
            detail,
        )?;
        Ok((offer, result?))
    }

    fn stage(&self, offer: &Offer) -> Result<PathBuf, EngineError> {
        let manifest = offer.manifest.manifest();
        let directory = self.paths.staging().join(manifest.version.to_string());
        ensure_private_dir(&self.paths.staging())?;
        ensure_private_dir(&directory)?;
        for artifact in manifest.artifacts_for(self.edition) {
            let path = directory.join(artifact.name.as_str());
            if verify_file(&path, artifact).is_ok() {
                continue;
            }
            let partial = directory.join(format!("{}.partial", artifact.name));
            // A partial file that is already too long cannot be resumed.
            if fs::metadata(&partial).is_ok_and(|m| m.len() >= artifact.size) {
                fs::remove_file(&partial).map_err(io_at(&partial))?;
            }
            self.source.download(
                &offer.release,
                artifact.name.as_str(),
                &partial,
                artifact.size,
            )?;
            if let Err(error) = verify_file(&partial, artifact) {
                // Never resume from data that failed verification.
                fs::remove_file(&partial).map_err(io_at(&partial))?;
                return Err(error.into());
            }
            fs::rename(&partial, &path).map_err(io_at(&path))?;
        }
        // Written last, so their presence means every artifact was complete.
        write_atomic(
            &directory.join(MANIFEST_SIGNATURE_ASSET),
            &offer.signature_bytes,
        )?;
        write_atomic(&directory.join(MANIFEST_ASSET), &offer.manifest_bytes)?;
        Ok(directory)
    }

    /// Verifies a staged release from scratch.
    pub fn staged(&self, version: &str) -> Result<StagedRelease, EngineError> {
        let parsed: MetaVersion = version
            .parse()
            .map_err(|_| EngineError::InvalidVersion(version.to_owned()))?;
        let state = State::load(&self.paths)?;
        let root = trust_store::load(&self.paths, state.root_version)?;
        let directory = self.paths.staging().join(parsed.to_string());
        let not_staged = || EngineError::NotStaged(version.to_owned());
        let bytes = read_limited(&directory.join(MANIFEST_ASSET), MAX_METADATA_SIZE)?
            .ok_or_else(not_staged)?;
        let signatures =
            read_limited(&directory.join(MANIFEST_SIGNATURE_ASSET), MAX_METADATA_SIZE)?
                .ok_or_else(not_staged)?;
        let signatures = SignatureFile::parse(&signatures).map_err(TrustError::from)?;
        let manifest = root.verify_manifest(&bytes, &signatures, self.now)?;
        check_sequence(
            state.high_water.get(&manifest.manifest().channel),
            &manifest,
        )?;
        if manifest.manifest().version != parsed {
            return Err(not_staged());
        }
        for artifact in manifest.manifest().artifacts_for(self.edition) {
            verify_file(&directory.join(artifact.name.as_str()), artifact)?;
        }
        let incompatibilities = check_compatibility(manifest.manifest(), &self.host);
        Ok(StagedRelease {
            manifest,
            directory,
            incompatibilities,
        })
    }

    /// Versions with a staging directory, newest first.
    pub fn staged_versions(&self) -> Vec<MetaVersion> {
        let Ok(entries) = fs::read_dir(self.paths.staging()) else {
            return Vec::new();
        };
        let mut versions: Vec<MetaVersion> = entries
            .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
            .collect();
        versions.sort();
        versions.reverse();
        versions
    }

    /// Plans the installation of a staged release.
    pub fn plan(&self, staged: &StagedRelease) -> Plan {
        plan::build(staged.manifest.manifest(), self.edition, self.now)
    }

    pub fn channel(&self) -> Result<Channel, EngineError> {
        Ok(State::load(&self.paths)?.channel)
    }

    pub fn set_channel(&self, channel: Channel) -> Result<(), EngineError> {
        let _lock = Lock::acquire(&self.paths.state_dir)?;
        let mut state = State::load(&self.paths)?;
        let detail = format!("{} to {channel}", state.channel);
        state.channel = channel;
        state.save(&self.paths)?;
        history::append(&self.paths, self.now, Action::Channel, true, detail)?;
        Ok(())
    }

    pub fn boot_environments(&self) -> Result<Vec<BootEnvironment>, EngineError> {
        Ok(metaupd_rollback::list(self.runner)?)
    }

    /// Selects an existing boot environment for the next boot.
    pub fn rollback(&self, name: &str, once: bool) -> Result<(), EngineError> {
        let _lock = Lock::acquire(&self.paths.state_dir)?;
        let result = metaupd_rollback::activate(self.runner, name, once);
        let detail = match &result {
            Ok(()) if once => format!("{name} for one boot"),
            Ok(()) => name.to_owned(),
            Err(error) => format!("{name}: {error}"),
        };
        history::append(
            &self.paths,
            self.now,
            Action::Rollback,
            result.is_ok(),
            detail,
        )?;
        Ok(result?)
    }
}
