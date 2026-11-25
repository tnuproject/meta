//! Release manifests.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use meta_common::MetaVersion;
use serde::{Deserialize, Serialize};

use crate::{AssetName, MetadataError, Sha256Digest, Timestamp};

pub const MANIFEST_SCHEMA: u32 = 1;
pub const MANIFEST_KIND: &str = "meta-release-manifest";
/// Largest artifact a manifest may describe.
pub const MAX_ARTIFACT_SIZE: u64 = 16 * 1024 * 1024 * 1024;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Testing,
    Unstable,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Testing => "testing",
            Channel::Unstable => "unstable",
        }
    }

    /// Whether a subscriber of `self` may receive a release published to `release`.
    pub fn accepts(self, release: Channel) -> bool {
        release <= self
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl FromStr for Channel {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "stable" => Ok(Channel::Stable),
            "testing" => Ok(Channel::Testing),
            "unstable" => Ok(Channel::Unstable),
            _ => Err(format!(
                "unknown channel {text:?}: use stable, testing or unstable"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Debian,
    Arch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Kernel,
    MetaSystem,
    NativePackages,
    Linux2bsd,
    LinuxCompat,
    Linuxkpi,
    Eds,
    Firmware,
    CoreUtilities,
    Desktop,
    Security,
    Edition,
    Documentation,
}

/// How an artifact is applied. There is deliberately no "run a script" method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Apply {
    /// Installed with pkg(8) into a cloned boot environment; atomic on activation.
    PkgInBootEnvironment,
    /// Copied to the EFI system partition, which no boot environment covers.
    EfiSystemPartition,
    /// Written to device firmware; cannot be rolled back.
    DeviceFirmware,
    /// Updates a Linux compatibility root outside the boot environment.
    CompatibilityRoot,
    /// Informational; nothing is installed.
    None,
}

impl Apply {
    /// Whether activating the previous boot environment undoes this change.
    pub fn covered_by_boot_environment(self) -> bool {
        matches!(self, Apply::PkgInBootEnvironment | Apply::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: AssetName,
    pub category: Category,
    pub size: u64,
    pub sha256: Sha256Digest,
    pub apply: Apply,
    #[serde(default)]
    pub requires_reboot: bool,
    /// Restricts the artifact to some editions; empty means every edition.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub editions: BTreeSet<Edition>,
}

/// Inclusive range of supported `__FreeBSD_version` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Osreldate {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compatibility {
    /// FreeBSD `MACHINE_ARCH` values.
    pub arch: BTreeSet<String>,
    /// Oldest installed META version that can update directly.
    pub min_meta_version: MetaVersion,
    /// Kernel versions this release can be applied from.
    pub osreldate: Osreldate,
    /// Oldest metaupd able to process this manifest.
    pub min_metaupd_version: MetaVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub id: String,
    pub summary: String,
    /// Asset holding the instructions; must be listed as an artifact.
    pub document: AssetName,
    pub reversible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub kind: String,
    pub project: String,
    pub version: MetaVersion,
    pub codename: String,
    pub channel: Channel,
    /// Strictly increasing per channel; the basis of rollback protection.
    pub sequence: u64,
    pub created: Timestamp,
    pub expires: Timestamp,
    pub compatibility: Compatibility,
    pub artifacts: Vec<Artifact>,
    /// Asset holding the changelog; must be listed as an artifact.
    pub changelog: AssetName,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrations: Vec<Migration>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, MetadataError> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|source| MetadataError::Json {
                what: "release manifest",
                source,
            })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), MetadataError> {
        let mut problems = Vec::new();
        if self.schema != MANIFEST_SCHEMA {
            problems.push(format!("unsupported schema {}", self.schema));
        }
        if self.kind != MANIFEST_KIND {
            problems.push(format!("unexpected kind {:?}", self.kind));
        }
        if self.project != meta_common::PROJECT {
            problems.push(format!("unexpected project {:?}", self.project));
        }
        if self.codename.is_empty() || !self.codename.bytes().all(|b| b.is_ascii_alphanumeric()) {
            problems.push("codename must be ASCII alphanumeric".into());
        }
        if self.sequence == 0 {
            problems.push("sequence must be at least 1".into());
        }
        if self.expires <= self.created {
            problems.push("expires must be later than created".into());
        }
        if self.channel == Channel::Stable && self.version.pre.is_some() {
            problems.push("pre-release versions cannot be published to the stable channel".into());
        }
        if self.compatibility.arch.is_empty() {
            problems.push("no supported architecture".into());
        }
        if self.compatibility.osreldate.min > self.compatibility.osreldate.max {
            problems.push("osreldate range is empty".into());
        }
        self.validate_artifacts(&mut problems);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(MetadataError::Invalid {
                what: "release manifest",
                problems,
            })
        }
    }

    fn validate_artifacts(&self, problems: &mut Vec<String>) {
        let mut names = BTreeSet::new();
        for artifact in &self.artifacts {
            let name = &artifact.name;
            // Compared without case so a case-insensitive staging filesystem is safe.
            if !names.insert(name.as_str().to_ascii_lowercase()) {
                problems.push(format!("duplicate artifact {name}"));
            }
            if name.as_str().starts_with("meta-manifest.")
                || name.as_str().starts_with("meta-root-")
            {
                problems.push(format!("artifact {name} uses a reserved metadata name"));
            }
            if artifact.size == 0 || artifact.size > MAX_ARTIFACT_SIZE {
                problems.push(format!("artifact {name} has an unacceptable size"));
            }
            if artifact.category == Category::Firmware && artifact.apply == Apply::None {
                problems.push(format!(
                    "firmware artifact {name} must state how it is applied"
                ));
            }
            if artifact.category == Category::Documentation && artifact.apply != Apply::None {
                problems.push(format!("documentation artifact {name} cannot be applied"));
            }
            if artifact.category == Category::Kernel && !artifact.requires_reboot {
                problems.push(format!("kernel artifact {name} must require a reboot"));
            }
        }
        let referenced = std::iter::once(&self.changelog)
            .chain(self.migrations.iter().map(|migration| &migration.document));
        for document in referenced {
            if self.artifact(document).is_none() {
                problems.push(format!("referenced document {document} is not an artifact"));
            }
        }
    }

    pub fn artifact(&self, name: &AssetName) -> Option<&Artifact> {
        self.artifacts
            .iter()
            .find(|artifact| &artifact.name == name)
    }

    /// Artifacts a system of `edition` downloads.
    pub fn artifacts_for(&self, edition: Option<Edition>) -> impl Iterator<Item = &Artifact> {
        self.artifacts.iter().filter(move |artifact| {
            artifact.editions.is_empty() || edition.is_some_and(|e| artifact.editions.contains(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "kind": "meta-release-manifest",
            "project": "META",
            "version": "26.0",
            "codename": "uglydemon",
            "channel": "stable",
            "sequence": 1,
            "created": "2026-01-01T00:00:00Z",
            "expires": "2026-04-01T00:00:00Z",
            "compatibility": {
                "arch": ["amd64"],
                "min_meta_version": "26.0-alpha1",
                "osreldate": {"min": 1501000, "max": 1501999},
                "min_metaupd_version": "26.0-alpha1"
            },
            "artifacts": [{
                "name": "CHANGELOG.md",
                "category": "documentation",
                "size": 10,
                "sha256": "00".repeat(32),
                "apply": "none"
            }],
            "changelog": "CHANGELOG.md"
        })
    }

    fn parse(value: &serde_json::Value) -> Result<Manifest, MetadataError> {
        Manifest::parse(value.to_string().as_bytes())
    }

    fn error_after(change: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value = sample();
        change(&mut value);
        parse(&value).unwrap_err().to_string()
    }

    #[test]
    fn accepts_the_sample() {
        assert_eq!(parse(&sample()).unwrap().version.to_string(), "26.0");
    }

    #[test]
    fn rejects_unknown_fields() {
        assert!(error_after(|v| v["post_install_script"] = "rm".into()).contains("unknown field"));
    }

    #[test]
    fn rejects_unknown_apply_methods() {
        let error = error_after(|v| v["artifacts"][0]["apply"] = "shell-script".into());
        assert!(error.contains("unknown variant"), "{error}");
    }

    #[test]
    fn rejects_path_traversal_in_names() {
        assert!(error_after(|v| v["changelog"] = "../x".into()).contains("unsafe asset name"));
    }

    #[test]
    fn rejects_duplicates_ignoring_case() {
        let error = error_after(|v| {
            let mut copy = v["artifacts"][0].clone();
            copy["name"] = "changelog.MD".into();
            v["artifacts"].as_array_mut().unwrap().push(copy);
        });
        assert!(error.contains("duplicate artifact"), "{error}");
    }

    #[test]
    fn rejects_prerelease_on_stable() {
        assert!(error_after(|v| v["version"] = "26.0-rc1".into()).contains("stable channel"));
    }

    #[test]
    fn rejects_kernel_without_reboot() {
        let error = error_after(|v| {
            v["artifacts"][0]["category"] = "kernel".into();
        });
        assert!(error.contains("must require a reboot"), "{error}");
    }

    #[test]
    fn rejects_reserved_artifact_names() {
        let error = error_after(|v| {
            v["artifacts"][0]["name"] = "meta-manifest.json".into();
            v["changelog"] = "meta-manifest.json".into();
        });
        assert!(error.contains("reserved"), "{error}");
    }

    #[test]
    fn reports_every_problem_at_once() {
        let error = error_after(|v| {
            v["sequence"] = 0.into();
            v["expires"] = v["created"].clone();
        });
        assert!(
            error.contains("sequence") && error.contains("expires"),
            "{error}"
        );
    }

    #[test]
    fn channels_accept_more_stable_releases_only() {
        assert!(Channel::Testing.accepts(Channel::Stable));
        assert!(!Channel::Stable.accepts(Channel::Testing));
        assert!(Channel::Unstable.accepts(Channel::Testing));
    }

    #[test]
    fn edition_filter() {
        let mut value = sample();
        let mut extra = value["artifacts"][0].clone();
        extra["name"] = "deb.txt".into();
        extra["editions"] = serde_json::json!(["debian"]);
        value["artifacts"].as_array_mut().unwrap().push(extra);
        let manifest = parse(&value).unwrap();
        assert_eq!(manifest.artifacts_for(None).count(), 1);
        assert_eq!(manifest.artifacts_for(Some(Edition::Debian)).count(), 2);
        assert_eq!(manifest.artifacts_for(Some(Edition::Arch)).count(), 1);
    }
}
