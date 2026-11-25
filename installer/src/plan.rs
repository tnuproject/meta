//! Installation plans.

use serde::Serialize;

use crate::catalog::desktops;
use crate::config::{Bootloader, EditionChoice, Filesystem, InstallConfig, NetworkMode};

pub const FORMAT: &str = "meta-install-plan-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub format: &'static str,
    pub meta_version: &'static str,
    /// Whether meta-install-run supports every choice.
    pub executable: bool,
    pub configuration: InstallConfig,
    pub user_password_set: bool,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    pub description: String,
    /// Irreversibly changes a disk.
    pub destructive: bool,
}

impl Plan {
    pub fn build(config: &InstallConfig) -> Result<Self, Vec<String>> {
        let problems = config.problems();
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Self {
            format: FORMAT,
            meta_version: meta_common::VERSION,
            executable: unsupported(config).is_empty(),
            configuration: config.clone(),
            user_password_set: !config.user.password.is_empty(),
            steps: steps(config),
            warnings: warnings(config),
        })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("plans contain only serializable values")
    }
}

/// Choices that meta-install-run cannot carry out.
pub fn unsupported(config: &InstallConfig) -> Vec<String> {
    let mut reasons = Vec::new();
    if !crate::execute::supported_filesystem(config) {
        reasons.push("installing on UFS".to_owned());
    }
    if config.encrypt {
        reasons.push("disk encryption".to_owned());
    }
    if config.bootloader == Bootloader::GrubChainload {
        reasons.push("GRUB".to_owned());
    }
    reasons
}

fn steps(config: &InstallConfig) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut add = |destructive: bool, description: String| {
        steps.push(Step {
            description,
            destructive,
        });
    };
    let disk = config.disk.as_deref().unwrap_or_default();
    add(
        true,
        format!("erase {disk} and create a GPT with EFI, swap and system partitions"),
    );
    let encryption = if config.encrypt { " on GELI" } else { "" };
    match config.filesystem {
        Filesystem::Zfs => add(
            true,
            format!("create a ZFS pool{encryption} with a boot environment layout on {disk}"),
        ),
        Filesystem::Ufs => add(true, format!("create a UFS filesystem on {disk}")),
    }
    if config.swap_gib > 0 {
        add(false, format!("configure {} GiB of swap", config.swap_gib));
    }
    add(
        false,
        "install the FreeBSD base system and META packages with pkg".into(),
    );
    add(
        false,
        format!(
            "set language {}, keyboard {}, timezone {}",
            config.language, config.keymap, config.timezone
        ),
    );
    add(false, format!("set hostname {}", config.hostname));
    match config.network {
        NetworkMode::Dhcp => add(false, "configure networking with DHCP".into()),
        NetworkMode::Later => add(false, "leave networking unconfigured".into()),
    }
    add(false, "install sudo".into());
    let groups = if config.user.groups.is_empty() {
        String::new()
    } else {
        format!(" in groups {}", config.user.groups.join(", "))
    };
    add(false, format!("create user {}{groups}", config.user.name));
    if config.user.sudo {
        add(
            false,
            format!("allow {} to use sudo with a password", config.user.name),
        );
    }
    add(false, "lock the root password".into());
    if config.linux_compat {
        add(false, "enable Linuxulator".into());
    }
    match config.edition {
        EditionChoice::None => {}
        EditionChoice::Debian => {
            add(
                false,
                "after the first boot: meta-compat create debian".into(),
            );
        }
        EditionChoice::Arch => add(
            false,
            "after the first boot: meta-compat create arch".into(),
        ),
    }
    if let Some(desktop) = desktops()
        .into_iter()
        .find(|d| config.desktop.as_deref() == Some(d.id.as_str()))
    {
        add(
            false,
            format!("install {}: {}", desktop.name, desktop.packages.join(" ")),
        );
    }
    if config.eds_enabled() {
        add(
            false,
            "enable Extended Driver Support for validated hardware".into(),
        );
    }
    match config.bootloader {
        Bootloader::FreebsdLoader => {
            add(
                false,
                "install FreeBSD loader.efi to the EFI system partition".into(),
            );
        }
        Bootloader::GrubChainload => {
            add(
                false,
                "install GRUB chain-loading FreeBSD loader.efi".into(),
            );
        }
    }
    steps
}

fn warnings(config: &InstallConfig) -> Vec<String> {
    let mut warnings: Vec<String> = unsupported(config)
        .into_iter()
        .map(|reason| format!("This version cannot carry out this plan: {reason}."))
        .collect();
    if config.filesystem == Filesystem::Ufs {
        warnings.push("UFS has no boot environments, so metaupd could not roll back.".into());
    }
    if config.encrypt {
        warnings.push("Disk encryption is RESEARCH REQUIRED.".into());
    }
    if config.bootloader == Bootloader::GrubChainload {
        warnings.push("GRUB is RESEARCH REQUIRED; FreeBSD loader is the tested path.".into());
    }
    if config.edition != EditionChoice::None {
        warnings.push("Compatibility editions are PLANNED.".into());
    }
    if config.user.password.is_empty() {
        warnings.push("No user password has been set.".into());
    }
    if !config.user.sudo {
        warnings.push(
            "The root password is locked and the user cannot use sudo, so nobody could administer \n             this system."
                .into(),
        );
    }
    for desktop in desktops() {
        if config.desktop.as_deref() == Some(desktop.id.as_str()) && !desktop.availability.available
        {
            warnings.push(format!(
                "{} packages were not available from {} on {}.",
                desktop.name, desktop.availability.repository, desktop.availability.date
            ));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::valid;

    #[test]
    fn only_supported_plans_are_executable() {
        assert!(Plan::build(&valid()).unwrap().executable);
        let mut config = valid();
        config.encrypt = true;
        let plan = Plan::build(&config).unwrap();
        assert!(!plan.executable);
        assert!(plan.warnings.iter().any(|w| w.contains("cannot carry out")));
    }

    #[test]
    fn destructive_steps_are_marked_and_come_first() {
        let plan = Plan::build(&valid()).unwrap();
        let destructive: Vec<bool> = plan.steps.iter().map(|s| s.destructive).collect();
        assert_eq!(destructive.iter().filter(|d| **d).count(), 2);
        assert!(destructive[0] && destructive[1]);
        assert!(plan.steps[0].description.contains("vtbd0"));
    }

    #[test]
    fn invalid_configuration_has_no_plan() {
        let mut config = valid();
        config.disk = None;
        assert_eq!(Plan::build(&config).unwrap_err(), ["no disk selected"]);
    }

    #[test]
    fn sudo_is_always_installed() {
        let mut config = valid();
        config.user.sudo = false;
        let plan = Plan::build(&config).unwrap();
        assert!(plan.steps.iter().any(|s| s.description == "install sudo"));
        assert!(
            !plan
                .steps
                .iter()
                .any(|s| s.description.contains("to use sudo"))
        );
        assert!(plan.warnings.iter().any(|w| w.contains("nobody could")));
    }

    #[test]
    fn unavailable_desktop_is_reported() {
        let mut config = valid();
        config.desktop = Some("kde".into());
        let plan = Plan::build(&config).unwrap();
        assert!(
            plan.warnings
                .iter()
                .any(|w| w.contains("KDE Plasma packages were not available"))
        );
    }

    #[test]
    fn answer_file_round_trip() {
        let config = valid();
        let text = toml::to_string(&config).unwrap();
        let parsed: InstallConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed, config);
    }
}
