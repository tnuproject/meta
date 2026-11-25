//! Installation plans.

use serde::Serialize;

use crate::catalog::desktops;
use crate::config::{Bootloader, EditionChoice, Filesystem, InstallConfig, NetworkMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub format: &'static str,
    pub meta_version: &'static str,
    /// Always false: this version cannot carry a plan out.
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
            format: "meta-install-plan-v1",
            meta_version: meta_common::VERSION,
            executable: false,
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
        add(
            false,
            "enable Linuxulator and install a Linux userland".into(),
        );
    }
    match config.edition {
        EditionChoice::None => {}
        EditionChoice::Debian => add(false, "create the Debian compatibility root".into()),
        EditionChoice::Arch => add(false, "create the Arch compatibility root".into()),
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
    let mut warnings =
        vec!["This version only plans installations; it cannot perform them.".to_owned()];
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
    fn plans_are_never_executable() {
        assert!(!Plan::build(&valid()).unwrap().executable);
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
