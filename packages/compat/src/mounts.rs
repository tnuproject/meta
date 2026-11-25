//! Filesystems mounted inside a root.

use meta_common::{CommandError, CommandRunner};
use meta_pkg_policy::Edition;

pub const MOUNT: &str = "/sbin/mount";
pub const UMOUNT: &str = "/sbin/umount";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub kind: &'static str,
    pub source: String,
    /// Path inside the root.
    pub inside: String,
    pub options: Option<&'static str>,
}

/// devfs(8) ruleset of the roots. FreeBSD's own rulesets have small numbers.
const RULESET: &str = "2600";

/// Hides every device except basic ones, terminals, and the directory shared
/// memory is mounted on. FreeBSD's jail ruleset (4) lacks the last one.
pub fn define_ruleset(runner: &dyn CommandRunner) -> Result<(), CommandError> {
    const DEVFS: &str = "/sbin/devfs";
    // Includes rulesets 1 to 3: hide all, unhide basic, unhide login.
    runner.stdout("/usr/sbin/service", &["devfs", "onestart"])?;
    runner.stdout(DEVFS, &["rule", "-s", RULESET, "delset"])?;
    for rule in [
        &["include", "1"][..],
        &["include", "2"],
        &["include", "3"],
        &["path", "shm", "unhide"],
    ] {
        let mut args = vec!["rule", "-s", RULESET, "add"];
        args.extend(rule);
        runner.stdout(DEVFS, &args)?;
    }
    Ok(())
}

/// Everything a running root needs, in mounting order.
pub fn wanted(edition: &Edition) -> Vec<Mount> {
    let fixed = |kind, inside: &str, options| Mount {
        kind,
        source: kind.to_owned(),
        inside: inside.to_owned(),
        options,
    };
    let mut mounts = vec![
        fixed("devfs", "/dev", Some("ruleset=2600")),
        fixed("fdescfs", "/dev/fd", Some("linrdlnk")),
        fixed("tmpfs", "/dev/shm", Some("mode=1777")),
        fixed("linprocfs", "/proc", None),
        fixed("linsysfs", "/sys", None),
    ];
    mounts.extend(edition.shared.iter().map(|shared| Mount {
        kind: "nullfs",
        source: shared.host.clone(),
        inside: shared.inside.clone(),
        options: (!shared.writable).then_some("ro"),
    }));
    mounts
}

/// Mount points strictly below `root`, deepest first, from `mount -p`.
///
/// `mount -p` prints fstab(5) fields, in which spaces are written as `\040`.
pub fn below(root: &str, table: &str) -> Vec<String> {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mut points: Vec<String> = table
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(|point| {
            point
                .replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\134", "\\")
        })
        .filter(|point| point.starts_with(&prefix))
        .collect();
    points.sort_by_key(|point| std::cmp::Reverse(point.matches('/').count()));
    points
}

pub fn current(runner: &dyn CommandRunner, root: &str) -> Result<Vec<String>, CommandError> {
    Ok(below(root, &runner.stdout(MOUNT, &["-p"])?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "zroot/ROOT/default\t/\tzfs\trw\t0 0\n\
        zroot/meta/compat/debian /compat/meta/debian zfs rw 0 0\n\
        devfs /compat/meta/debian/dev devfs rw 0 0\n\
        tmpfs /compat/meta/debian/dev/shm tmpfs rw 0 0\n\
        /home /compat/meta/debian/home nullfs rw 0 0\n\
        x /compat/meta/debian-old/dev devfs rw 0 0\n\
        y /compat/meta/debian/my\\040dir nullfs rw 0 0\n";

    #[test]
    fn deepest_first_without_siblings_or_the_root() {
        let points = below("/compat/meta/debian", TABLE);
        assert_eq!(points[0], "/compat/meta/debian/dev/shm");
        assert_eq!(points.len(), 4);
        assert!(points.contains(&"/compat/meta/debian/my dir".to_owned()));
        assert!(points.iter().all(|p| !p.contains("debian-old")));
    }

    #[test]
    fn home_is_shared_and_devices_are_restricted() {
        for edition in Edition::builtin() {
            let mounts = wanted(&edition);
            assert_eq!(mounts[0].options, Some("ruleset=2600"));
            assert!(
                mounts
                    .iter()
                    .any(|m| m.kind == "nullfs" && m.inside == "/home")
            );
        }
    }
}
