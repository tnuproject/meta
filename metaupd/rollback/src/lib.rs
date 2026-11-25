//! ZFS boot environments through bectl(8).
//!
//! Listing is read-only. [`activate`] changes the boot environment used at the
//! next boot; it never destroys one.

use meta_common::{CommandError, CommandRunner};
use serde::Serialize;

pub const BECTL: &str = "/sbin/bectl";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BootEnvironment {
    pub name: String,
    /// Mounted as the running root.
    pub active_now: bool,
    /// Used at the next boot.
    pub active_on_reboot: bool,
    /// Used at the next boot only.
    pub active_once: bool,
    pub mountpoint: Option<String>,
    pub space: String,
    pub created: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("boot environments are unavailable: {0}")]
    Unavailable(CommandError),
    #[error("unexpected bectl output line {0:?}")]
    Parse(String),
    #[error("invalid boot environment name {0:?}")]
    InvalidName(String),
    #[error("no boot environment named {0:?}")]
    NotFound(String),
    #[error("{0:?} is already selected for the next boot")]
    AlreadyActive(String),
    #[error("no earlier boot environment to roll back to")]
    NoPrevious,
    #[error(transparent)]
    Command(CommandError),
}

/// Lists boot environments.
pub fn list(runner: &dyn CommandRunner) -> Result<Vec<BootEnvironment>, RollbackError> {
    let output = runner
        .stdout(BECTL, &["list", "-H"])
        .map_err(RollbackError::Unavailable)?;
    parse_list(&output)
}

/// Parses `bectl list -H`: tab-separated name, flags, mountpoint, space, date.
pub fn parse_list(output: &str) -> Result<Vec<BootEnvironment>, RollbackError> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            let [name, flags, mountpoint, space, created] = fields.as_slice() else {
                return Err(RollbackError::Parse(line.to_owned()));
            };
            Ok(BootEnvironment {
                name: (*name).to_owned(),
                active_now: flags.contains('N'),
                active_on_reboot: flags.contains('R'),
                active_once: flags.contains('T'),
                mountpoint: (*mountpoint != "-").then(|| (*mountpoint).to_owned()),
                space: (*space).to_owned(),
                created: (*created).to_owned(),
            })
        })
        .collect()
}

/// Names metaupd accepts: a subset of what ZFS allows, never option-like.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
}

/// Chooses the rollback target: `requested`, or the newest inactive environment.
///
/// `bectl list` prints creation dates as `YYYY-MM-DD HH:MM`, which sort as text.
pub fn choose_target<'a>(
    environments: &'a [BootEnvironment],
    requested: Option<&str>,
) -> Result<&'a BootEnvironment, RollbackError> {
    let target = match requested {
        Some(name) if !valid_name(name) => {
            return Err(RollbackError::InvalidName(name.to_owned()));
        }
        Some(name) => environments
            .iter()
            .find(|be| be.name == name)
            .ok_or_else(|| RollbackError::NotFound(name.to_owned()))?,
        None => environments
            .iter()
            .filter(|be| !be.active_now && !be.active_on_reboot && !be.active_once)
            .filter(|be| valid_name(&be.name))
            .max_by(|a, b| a.created.cmp(&b.created))
            .ok_or(RollbackError::NoPrevious)?,
    };
    if target.active_on_reboot || target.active_once {
        return Err(RollbackError::AlreadyActive(target.name.clone()));
    }
    Ok(target)
}

/// Selects `name` for the next boot; with `once`, for that boot only.
pub fn activate(runner: &dyn CommandRunner, name: &str, once: bool) -> Result<(), RollbackError> {
    if !valid_name(name) {
        return Err(RollbackError::InvalidName(name.to_owned()));
    }
    let args: &[&str] = if once {
        &["activate", "-t", name]
    } else {
        &["activate", name]
    };
    runner
        .stdout(BECTL, args)
        .map(drop)
        .map_err(RollbackError::Command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;

    // Captured from FreeBSD 15.1-RELEASE; see docs/development/testing.md.
    const LIST: &str = include_str!("../fixtures/bectl-list-15.1.txt");

    #[test]
    fn parses_captured_output() {
        let list = parse_list(LIST).unwrap();
        assert!(list.iter().any(|be| be.active_now && be.active_on_reboot));
        assert!(list.iter().all(|be| !be.name.is_empty()));
    }

    fn be(name: &str, flags: &str, created: &str) -> String {
        format!("{name}\t{flags}\t-\t1.2G\t{created}\n")
    }

    #[test]
    fn default_target_is_newest_inactive() {
        let text = be("default", "NR", "2026-03-01 10:00")
            + &be("old", "-", "2026-01-01 10:00")
            + &be("newer", "-", "2026-02-01 10:00");
        let list = parse_list(&text).unwrap();
        assert_eq!(choose_target(&list, None).unwrap().name, "newer");
        assert_eq!(choose_target(&list, Some("old")).unwrap().name, "old");
        assert!(matches!(
            choose_target(&list, Some("default")),
            Err(RollbackError::AlreadyActive(_))
        ));
        assert!(matches!(
            choose_target(&list, Some("gone")),
            Err(RollbackError::NotFound(_))
        ));
    }

    #[test]
    fn single_environment_has_no_target() {
        let list = parse_list(&be("default", "NR", "2026-03-01 10:00")).unwrap();
        assert!(matches!(
            choose_target(&list, None),
            Err(RollbackError::NoPrevious)
        ));
    }

    #[test]
    fn option_like_names_are_refused() {
        for name in ["-t", "", "a b", "a/b", "a@b", "../x"] {
            assert!(!valid_name(name), "{name:?}");
            assert!(activate(&FakeRunner::new(), name, false).is_err());
        }
    }

    #[test]
    fn activation_uses_temporary_flag() {
        let runner = FakeRunner::new().with("/sbin/bectl activate -t old", "");
        activate(&runner, "old", true).unwrap();
        assert!(activate(&runner, "old", false).is_err());
    }

    #[test]
    fn malformed_lines_are_errors() {
        assert!(matches!(
            parse_list("only two\tfields\n"),
            Err(RollbackError::Parse(_))
        ));
    }
}
