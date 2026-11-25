//! The twelve installer screens as lists of fields.

use crate::catalog::desktops;
use crate::config::{
    Bootloader, EditionChoice, Filesystem, InstallConfig, Mode, NetworkMode, Password,
};
use crate::disks::Disk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Welcome,
    Language,
    Timezone,
    Network,
    Disk,
    Edition,
    User,
    Desktop,
    Drivers,
    Components,
    Bootloader,
    Review,
}

impl Screen {
    pub const ALL: [Screen; 12] = [
        Screen::Welcome,
        Screen::Language,
        Screen::Timezone,
        Screen::Network,
        Screen::Disk,
        Screen::Edition,
        Screen::User,
        Screen::Desktop,
        Screen::Drivers,
        Screen::Components,
        Screen::Bootloader,
        Screen::Review,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Screen::Welcome => "Welcome",
            Screen::Language => "Language and keyboard",
            Screen::Timezone => "Timezone",
            Screen::Network => "Network",
            Screen::Disk => "Disk and filesystem",
            Screen::Edition => "META edition",
            Screen::User => "User configuration",
            Screen::Desktop => "Desktop environment",
            Screen::Drivers => "Drivers and firmware",
            Screen::Components => "System components",
            Screen::Bootloader => "Bootloader",
            Screen::Review => "Review",
        }
    }

    pub fn fields(self) -> &'static [Field] {
        match self {
            Screen::Welcome => &[Field::Mode],
            Screen::Language => &[Field::Language, Field::Keymap],
            Screen::Timezone => &[Field::Timezone],
            Screen::Network => &[Field::Hostname, Field::Network],
            Screen::Disk => &[Field::Disk, Field::Filesystem, Field::Encrypt, Field::Swap],
            Screen::Edition => &[Field::Edition],
            Screen::User => &[
                Field::UserName,
                Field::FullName,
                Field::Groups,
                Field::Sudo,
                Field::Password,
                Field::PasswordAgain,
            ],
            Screen::Desktop => &[Field::Desktop],
            Screen::Drivers => &[Field::Eds],
            Screen::Components => &[Field::LinuxCompat],
            Screen::Bootloader => &[Field::Bootloader],
            Screen::Review => &[],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Mode,
    Language,
    Keymap,
    Timezone,
    Hostname,
    Network,
    Disk,
    Filesystem,
    Encrypt,
    Swap,
    Edition,
    UserName,
    FullName,
    Groups,
    Sudo,
    Password,
    PasswordAgain,
    Desktop,
    Eds,
    LinuxCompat,
    Bootloader,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Field::Mode => "Installation mode",
            Field::Language => "Language",
            Field::Keymap => "Keyboard layout",
            Field::Timezone => "Timezone",
            Field::Hostname => "Hostname",
            Field::Network => "Network",
            Field::Disk => "Disk",
            Field::Filesystem => "Filesystem",
            Field::Encrypt => "Encryption",
            Field::Swap => "Swap (GiB)",
            Field::Edition => "Edition",
            Field::UserName => "User name",
            Field::FullName => "Full name",
            Field::Groups => "Groups",
            Field::Sudo => "Allow sudo",
            Field::Password => "Password",
            Field::PasswordAgain => "Password again",
            Field::Desktop => "Desktop",
            Field::Eds => "Extended Driver Support",
            Field::LinuxCompat => "Linux compatibility",
            Field::Bootloader => "Bootloader",
        }
    }

    /// Text fields are typed; the others are cycled with the arrow keys.
    pub fn is_text(self) -> bool {
        self.text(&mut InstallConfig::default()).is_some()
            || self.is_password()
            || matches!(self, Field::Groups | Field::Swap)
    }

    pub fn is_password(self) -> bool {
        matches!(self, Field::Password | Field::PasswordAgain)
    }

    fn text(self, config: &mut InstallConfig) -> Option<&mut String> {
        Some(match self {
            Field::Language => &mut config.language,
            Field::Keymap => &mut config.keymap,
            Field::Timezone => &mut config.timezone,
            Field::Hostname => &mut config.hostname,
            Field::UserName => &mut config.user.name,
            Field::FullName => &mut config.user.full_name,
            _ => return None,
        })
    }

    fn password(self, config: &mut InstallConfig) -> Option<&mut Password> {
        match self {
            Field::Password => Some(&mut config.user.password),
            Field::PasswordAgain => Some(&mut config.user.password_again),
            _ => None,
        }
    }

    pub fn value(self, config: &InstallConfig) -> String {
        let yes_no = |value: bool| if value { "yes" } else { "no" }.to_owned();
        match self {
            Field::Mode => match config.mode {
                Mode::Typical => "Typical",
                Mode::Minimal => "Minimal",
                Mode::Custom => "Custom",
            }
            .to_owned(),
            Field::Language => config.language.clone(),
            Field::Keymap => config.keymap.clone(),
            Field::Timezone => config.timezone.clone(),
            Field::Hostname => config.hostname.clone(),
            Field::Network => match config.network {
                NetworkMode::Dhcp => "DHCP",
                NetworkMode::Later => "Configure later",
            }
            .to_owned(),
            Field::Disk => config.disk.clone().unwrap_or_else(|| "none".into()),
            Field::Filesystem => match config.filesystem {
                Filesystem::Zfs => "ZFS",
                Filesystem::Ufs => "UFS (no rollback)",
            }
            .to_owned(),
            Field::Encrypt => yes_no(config.encrypt),
            Field::Swap => config.swap_gib.to_string(),
            Field::Edition => match config.edition {
                EditionChoice::None => "None (native packages only)",
                EditionChoice::Debian => "META Debian Edition (PLANNED)",
                EditionChoice::Arch => "META Arch Edition (PLANNED)",
            }
            .to_owned(),
            Field::UserName => config.user.name.clone(),
            Field::FullName => config.user.full_name.clone(),
            Field::Groups => config.user.groups.join(","),
            Field::Sudo => yes_no(config.user.sudo),
            Field::Password => "*".repeat(config.user.password.len()),
            Field::PasswordAgain => "*".repeat(config.user.password_again.len()),
            Field::Desktop => match &config.desktop {
                None => "None (console only)".to_owned(),
                Some(id) => desktops()
                    .into_iter()
                    .find(|desktop| desktop.id == *id)
                    .map_or_else(|| id.clone(), |desktop| desktop.name),
            },
            Field::Eds => match config.eds {
                None => format!(
                    "Automatic ({})",
                    if config.eds_enabled() { "on" } else { "off" }
                ),
                Some(value) => yes_no(value),
            },
            Field::LinuxCompat => yes_no(config.linux_compat),
            Field::Bootloader => match config.bootloader {
                Bootloader::FreebsdLoader => "FreeBSD loader",
                Bootloader::GrubChainload => "GRUB (RESEARCH REQUIRED)",
            }
            .to_owned(),
        }
    }

    /// Selects the next or previous choice.
    pub fn cycle(self, config: &mut InstallConfig, disks: &[Disk], forward: bool) {
        match self {
            Field::Mode => {
                let all = [Mode::Typical, Mode::Minimal, Mode::Custom];
                config.mode = step(&all, &config.mode, forward);
                if config.mode == Mode::Minimal {
                    config.desktop = None;
                }
            }
            Field::Network => {
                let all = [NetworkMode::Dhcp, NetworkMode::Later];
                config.network = step(&all, &config.network, forward);
            }
            Field::Disk => {
                let mut all = vec![None];
                all.extend(disks.iter().map(|disk| Some(disk.name.clone())));
                config.disk = step(&all, &config.disk, forward);
            }
            Field::Filesystem => {
                let all = [Filesystem::Zfs, Filesystem::Ufs];
                config.filesystem = step(&all, &config.filesystem, forward);
            }
            Field::Edition => {
                let all = [
                    EditionChoice::None,
                    EditionChoice::Debian,
                    EditionChoice::Arch,
                ];
                config.edition = step(&all, &config.edition, forward);
            }
            Field::Desktop => {
                let mut all = vec![None];
                if config.mode != Mode::Minimal {
                    all.extend(desktops().into_iter().map(|desktop| Some(desktop.id)));
                }
                config.desktop = step(&all, &config.desktop, forward);
            }
            Field::Eds => {
                config.eds = step(&[None, Some(true), Some(false)], &config.eds, forward);
            }
            Field::Bootloader => {
                let all = [Bootloader::FreebsdLoader, Bootloader::GrubChainload];
                config.bootloader = step(&all, &config.bootloader, forward);
            }
            Field::Encrypt => config.encrypt = !config.encrypt,
            Field::Sudo => config.user.sudo = !config.user.sudo,
            Field::LinuxCompat => config.linux_compat = !config.linux_compat,
            _ => {}
        }
    }

    pub fn insert(self, config: &mut InstallConfig, c: char) {
        if c.is_control() {
            return;
        }
        if let Some(password) = self.password(config) {
            password.push(c);
        } else if let Some(text) = self.text(config) {
            if text.len() < 128 {
                text.push(c);
            }
        } else if self == Field::Swap
            && let Some(digit) = c.to_digit(10)
        {
            config.swap_gib = config
                .swap_gib
                .saturating_mul(10)
                .saturating_add(digit)
                .min(9999);
        } else if self == Field::Groups {
            let mut text = config.user.groups.join(",");
            text.push(c);
            config.user.groups = parse_groups(&text);
        }
    }

    pub fn backspace(self, config: &mut InstallConfig) {
        if let Some(password) = self.password(config) {
            password.pop();
        } else if let Some(text) = self.text(config) {
            text.pop();
        } else if self == Field::Swap {
            config.swap_gib /= 10;
        } else if self == Field::Groups {
            let mut text = config.user.groups.join(",");
            text.pop();
            config.user.groups = parse_groups(&text);
        }
    }
}

/// Splits on commas. A trailing comma is kept as an empty last element so it
/// survives while typing; validation rejects it if it is left there.
fn parse_groups(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split(',').map(str::to_owned).collect()
    }
}

fn step<T: PartialEq + Clone>(all: &[T], current: &T, forward: bool) -> T {
    let index = all.iter().position(|item| item == current).unwrap_or(0);
    let next = if forward {
        index + 1
    } else {
        index + all.len() - 1
    };
    all[next % all.len()].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycling_wraps_in_both_directions() {
        let mut config = InstallConfig::default();
        Field::Mode.cycle(&mut config, &[], false);
        assert_eq!(config.mode, Mode::Custom);
        Field::Mode.cycle(&mut config, &[], true);
        assert_eq!(config.mode, Mode::Typical);
    }

    #[test]
    fn minimal_mode_offers_no_desktop() {
        let mut config = InstallConfig {
            desktop: Some("xfce".into()),
            ..Default::default()
        };
        Field::Mode.cycle(&mut config, &[], true);
        assert_eq!((config.mode, config.desktop.clone()), (Mode::Minimal, None));
        Field::Desktop.cycle(&mut config, &[], true);
        assert_eq!(config.desktop, None);
    }

    #[test]
    fn group_editing() {
        let mut config = InstallConfig::default();
        for c in "wheel,video".chars() {
            Field::Groups.insert(&mut config, c);
        }
        assert_eq!(config.user.groups, ["wheel", "video"]);
        for _ in 0..11 {
            Field::Groups.backspace(&mut config);
        }
        assert!(config.user.groups.is_empty());
    }

    #[test]
    fn swap_editing_is_bounded() {
        let mut config = InstallConfig {
            swap_gib: 0,
            ..Default::default()
        };
        for c in "999999x".chars() {
            Field::Swap.insert(&mut config, c);
        }
        assert_eq!(config.swap_gib, 9999);
        Field::Swap.backspace(&mut config);
        assert_eq!(config.swap_gib, 999);
    }

    #[test]
    fn every_field_is_on_exactly_one_screen() {
        let count: usize = Screen::ALL.iter().map(|screen| screen.fields().len()).sum();
        assert_eq!(count, 21);
    }
}
