//! Installation plans.

use metaupd_metadata::{Apply, Artifact, Edition, Manifest, Timestamp};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub boot_environment: String,
    /// Ordered, human-readable steps.
    pub steps: Vec<Step>,
    pub requires_reboot: bool,
    /// Artifacts that activating the previous boot environment does not undo.
    pub not_rolled_back: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub description: String,
    pub undone_by_rollback: bool,
}

/// Orders the work so everything covered by the boot environment happens, and
/// is validated, before anything that is not.
pub fn build(manifest: &Manifest, edition: Option<Edition>, now: Timestamp) -> Plan {
    let artifacts: Vec<&Artifact> = manifest
        .artifacts_for(edition)
        .filter(|a| a.apply != Apply::None)
        .collect();
    let boot_environment = format!("meta-{}-{}", manifest.version, now.compact());
    let mut steps = Vec::new();
    let mut step = |description: String, undone_by_rollback: bool| {
        steps.push(Step {
            description,
            undone_by_rollback,
        });
    };
    step("verify every staged artifact again".into(), true);
    step(
        format!("clone the running boot environment to {boot_environment}"),
        true,
    );
    step(format!("mount {boot_environment}"), true);
    for artifact in artifacts
        .iter()
        .filter(|a| a.apply == Apply::PkgInBootEnvironment)
    {
        step(
            format!(
                "install {} with pkg inside {boot_environment}",
                artifact.name
            ),
            true,
        );
    }
    step(
        format!("validate {boot_environment}: packages, kernel, loader and fstab"),
        true,
    );
    for (method, text) in [
        (
            Apply::CompatibilityRoot,
            "update the compatibility root from",
        ),
        (
            Apply::EfiSystemPartition,
            "update the EFI system partition from",
        ),
        (Apply::DeviceFirmware, "write device firmware from"),
    ] {
        for artifact in artifacts.iter().filter(|a| a.apply == method) {
            step(format!("{text} {}", artifact.name), false);
        }
    }
    step(
        format!("activate {boot_environment} for one boot only"),
        true,
    );
    step("after a healthy boot, activate it permanently".into(), true);
    Plan {
        boot_environment,
        steps,
        requires_reboot: artifacts.iter().any(|a| a.requires_reboot),
        not_rolled_back: artifacts
            .iter()
            .filter(|a| !a.apply.covered_by_boot_environment())
            .map(|a| a.name.to_string())
            .collect(),
    }
}
