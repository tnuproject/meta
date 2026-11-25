//! Installation choices and their validation.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::catalog::desktops;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Typical,
    Minimal,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Filesystem {
    #[default]
    Zfs,
    Ufs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EditionChoice {
    #[default]
    None,
    Debian,
    Arch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bootloader {
    /// FreeBSD's loader(8), which understands boot environments.
    #[default]
    FreebsdLoader,
    /// GRUB chain-loading FreeBSD's loader. RESEARCH REQUIRED.
    GrubChainload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    #[default]
    Dhcp,
    /// Configure after installation.
    Later,
}

/// A password that is never serialized or printed.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Password(Zeroizing<String>);

impl Password {
    pub fn from_text(text: &str) -> Self {
        let mut password = Self::default();
        text.chars().for_each(|c| password.push(c));
        password
    }

    /// The password itself, for handing to another program's standard input.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    pub fn push(&mut self, c: char) {
        // Reserved up front so growing never leaves copies behind.
        if self.0.capacity() == 0 {
            self.0.reserve(256);
        }
        if self.0.len() + c.len_utf8() <= 256 {
            self.0.push(c);
        }
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }
}

impl fmt::Debug for Password {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.is_empty() {
            "Password(unset)"
        } else {
            "Password(set)"
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct User {
    pub name: String,
    pub full_name: String,
    pub groups: Vec<String>,
    pub sudo: bool,
    #[serde(skip)]
    pub password: Password,
    #[serde(skip)]
    pub password_again: Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct InstallConfig {
    pub mode: Mode,
    pub language: String,
    pub keymap: String,
    pub timezone: String,
    pub hostname: String,
    pub network: NetworkMode,
    pub disk: Option<String>,
    pub filesystem: Filesystem,
    pub encrypt: bool,
    pub swap_gib: u32,
    pub edition: EditionChoice,
    pub user: User,
    /// Desktop id, or `None` for a console-only system.
    pub desktop: Option<String>,
    /// `None` follows the default: on with a desktop, off without.
    pub eds: Option<bool>,
    pub linux_compat: bool,
    pub bootloader: Bootloader,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Typical,
            language: "en_US.UTF-8".into(),
            keymap: "us".into(),
            timezone: "UTC".into(),
            hostname: "meta".into(),
            network: NetworkMode::Dhcp,
            disk: None,
            filesystem: Filesystem::Zfs,
            encrypt: false,
            swap_gib: 2,
            edition: EditionChoice::None,
            user: User {
                sudo: true,
                ..User::default()
            },
            desktop: None,
            eds: None,
            linux_compat: true,
            bootloader: Bootloader::FreebsdLoader,
        }
    }
}

impl InstallConfig {
    /// EDS is on by default with a desktop, off for console-only and minimal
    /// installations, and always follows an explicit choice.
    pub fn eds_enabled(&self) -> bool {
        self.eds
            .unwrap_or(self.desktop.is_some() && self.mode != Mode::Minimal)
    }

    /// Every problem that prevents building a plan.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !valid_hostname(&self.hostname) {
            problems.push(format!("invalid hostname {:?}", self.hostname));
        }
        if !valid_locale(&self.language) {
            problems.push(format!("invalid language {:?}", self.language));
        }
        if !valid_word(&self.keymap) {
            problems.push(format!("invalid keyboard layout {:?}", self.keymap));
        }
        if !valid_timezone(&self.timezone) {
            problems.push(format!("invalid timezone {:?}", self.timezone));
        }
        match &self.disk {
            None => problems.push("no disk selected".into()),
            Some(disk) if !valid_disk(disk) => problems.push(format!("invalid disk {disk:?}")),
            Some(_) => {}
        }
        if self.encrypt && self.filesystem != Filesystem::Zfs {
            problems.push("encryption is only planned for ZFS".into());
        }
        if self.swap_gib > 1024 {
            problems.push("swap is larger than 1024 GiB".into());
        }
        if !valid_username(&self.user.name) {
            problems.push(format!("invalid user name {:?}", self.user.name));
        }
        for group in &self.user.groups {
            if !valid_username(group) {
                problems.push(format!("invalid group {group:?}"));
            }
        }
        if self.user.full_name.contains([':', '\n', '\r']) {
            problems.push("the full name contains a forbidden character".into());
        }
        if self.user.password != self.user.password_again {
            problems.push("the passwords do not match".into());
        }
        if let Some(id) = &self.desktop {
            if !desktops().iter().any(|desktop| desktop.id == *id) {
                problems.push(format!("unknown desktop {id:?}"));
            }
            if self.mode == Mode::Minimal {
                problems.push("a minimal installation has no desktop".into());
            }
        }
        if self.edition != EditionChoice::None && !self.linux_compat {
            problems.push("the Debian and Arch editions need Linux compatibility".into());
        }
        problems
    }
}

fn valid_hostname(name: &str) -> bool {
    (1..=253).contains(&name.len())
        && name.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
}

/// Names pw(8) accepts without surprises. `root` already exists.
fn valid_username(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name != "root"
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

/// A GEOM provider name; never a path.
fn valid_disk(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn valid_word(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn valid_locale(text: &str) -> bool {
    valid_word(text) && (text.ends_with(".UTF-8") || text == "C")
}

fn valid_timezone(text: &str) -> bool {
    text.split('/').all(|part| {
        (1..=64).contains(&part.len())
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+'))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn valid() -> InstallConfig {
        InstallConfig {
            disk: Some("vtbd0".into()),
            user: User {
                name: "alice".into(),
                sudo: true,
                ..User::default()
            },
            ..InstallConfig::default()
        }
    }

    #[test]
    fn valid_configuration_has_no_problems() {
        assert_eq!(valid().problems(), Vec::<String>::new());
    }

    #[test]
    fn eds_defaults_follow_the_desktop() {
        let mut config = valid();
        assert!(!config.eds_enabled());
        config.desktop = Some("xfce".into());
        assert!(config.eds_enabled());
        config.eds = Some(false);
        assert!(!config.eds_enabled());
        config.desktop = None;
        config.eds = Some(true);
        assert!(config.eds_enabled());
    }

    type Setter = fn(&mut InstallConfig, String);

    #[test]
    fn hostile_input_is_rejected() {
        let cases: Vec<(&str, Setter)> = vec![
            ("-rf", |c, v| c.hostname = v),
            ("a b", |c, v| c.hostname = v),
            ("root", |c, v| c.user.name = v),
            ("a;reboot", |c, v| c.user.name = v),
            ("../../etc", |c, v| c.timezone = v),
            ("/dev/ada0", |c, v| c.disk = Some(v)),
            ("ada0; rm", |c, v| c.disk = Some(v)),
            ("x\ny", |c, v| c.keymap = v),
            ("a:b", |c, v| c.user.full_name = v),
            ("cde", |c, v| c.desktop = Some(v)),
            ("a b", |c, v| c.user.groups = vec![v]),
        ];
        for (value, set) in cases {
            let mut config = valid();
            set(&mut config, value.to_owned());
            assert!(
                !config.problems().is_empty(),
                "{value:?} should be rejected"
            );
        }
    }

    #[test]
    fn incompatible_choices_are_rejected() {
        let mut config = valid();
        config.filesystem = Filesystem::Ufs;
        config.encrypt = true;
        assert!(config.problems()[0].contains("encryption"));
        let mut config = valid();
        config.mode = Mode::Minimal;
        config.desktop = Some("xfce".into());
        assert!(config.problems()[0].contains("minimal"));
        let mut config = valid();
        config.edition = EditionChoice::Arch;
        config.linux_compat = false;
        assert!(config.problems()[0].contains("Linux compatibility"));
    }

    #[test]
    fn passwords_never_leave_the_process() {
        let mut config = valid();
        for c in "hunter2".chars() {
            config.user.password.push(c);
            config.user.password_again.push(c);
        }
        assert!(!format!("{config:?}").contains("hunter2"));
        assert!(!serde_json::to_string(&config).unwrap().contains("hunter2"));
        config.user.password_again.pop();
        assert!(config.problems()[0].contains("do not match"));
    }

    #[test]
    fn answer_files_cannot_carry_passwords() {
        let text = "[user]\nname = \"alice\"\npassword = \"x\"\n";
        assert!(toml::from_str::<InstallConfig>(text).is_err());
    }
}
