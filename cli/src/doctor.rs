//! `meta doctor`.

use std::path::Path;
use std::process::ExitCode;

use linux2bsd::{Linuxulator, discover};
use meta_common::platform::sysctl;
use meta_common::{CommandRunner, Platform};
use meta_pkg_policy::{Edition, check_isolation};

/// Oldest FreeBSD kernel META is developed against (15.0-RELEASE).
const MINIMUM_OSRELDATE: u32 = 1_500_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Info,
    Warning,
    Failure,
}

struct Finding {
    level: Level,
    check: &'static str,
    detail: String,
}

/// Runs every check. Fails only when something is actually broken.
pub fn run(runner: &dyn CommandRunner) -> ExitCode {
    let platform = Platform::detect(runner);
    let mut findings = vec![check_platform(&platform), check_editions()];
    if platform.is_freebsd() {
        findings.push(check_root_filesystem(runner));
        findings.push(check_boot_environments(runner));
        findings.push(check_secure_level(runner));
        findings.extend(check_linux(runner));
        findings.push(check_sudo());
        findings.push(check_trust());
        findings.push(check_update_state(
            Path::new("/"),
            Path::new("/var/db/metaupd"),
        ));
    }
    for finding in &findings {
        let label = match finding.level {
            Level::Ok => "ok",
            Level::Info => "info",
            Level::Warning => "warning",
            Level::Failure => "FAILED",
        };
        println!("{label:8} {:18} {}", finding.check, finding.detail);
    }
    if findings
        .iter()
        .any(|finding| finding.level == Level::Failure)
    {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn finding(level: Level, check: &'static str, detail: impl Into<String>) -> Finding {
    Finding {
        level,
        check,
        detail: detail.into(),
    }
}

fn check_platform(platform: &Platform) -> Finding {
    const CHECK: &str = "platform";
    if !platform.is_freebsd() {
        return finding(
            Level::Info,
            CHECK,
            format!(
                "{} development host; FreeBSD checks skipped",
                platform.os_name
            ),
        );
    }
    match platform.osreldate {
        Some(version) if version >= MINIMUM_OSRELDATE => {
            let release = platform
                .kernel_release
                .as_deref()
                .unwrap_or("unknown release");
            finding(
                Level::Ok,
                CHECK,
                format!("FreeBSD {release} on {}", platform.arch),
            )
        }
        Some(version) => finding(
            Level::Failure,
            CHECK,
            format!("kernel {version} is older than the minimum {MINIMUM_OSRELDATE}"),
        ),
        None => finding(Level::Failure, CHECK, "cannot determine the kernel version"),
    }
}

fn check_editions() -> Finding {
    match check_isolation(&Edition::builtin()) {
        Ok(()) => finding(Level::Ok, "editions", "compatibility roots are separate"),
        Err(problems) => finding(Level::Failure, "editions", problems.join("; ")),
    }
}

fn check_root_filesystem(runner: &dyn CommandRunner) -> Finding {
    const CHECK: &str = "root filesystem";
    match runner.stdout("/sbin/mount", &["-p"]) {
        Ok(text) => match root_filesystem(&text) {
            Some("zfs") => finding(Level::Ok, CHECK, "ZFS"),
            Some(other) => finding(
                Level::Warning,
                CHECK,
                format!("{other}; boot environments and rollback need ZFS"),
            ),
            None => finding(Level::Failure, CHECK, "cannot find the root mount"),
        },
        Err(error) => finding(Level::Failure, CHECK, error.to_string()),
    }
}

/// Filesystem type of `/` from `mount -p`, which prints fstab(5) fields.
fn root_filesystem(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.nth(1)? == "/").then(|| fields.next())?
    })
}

fn check_boot_environments(runner: &dyn CommandRunner) -> Finding {
    const CHECK: &str = "boot environments";
    match runner.run("/sbin/bectl", &["check"]) {
        Ok(output) if output.success() => finding(Level::Ok, CHECK, "available"),
        Ok(_) => finding(Level::Warning, CHECK, "not available on this system"),
        Err(error) => finding(Level::Warning, CHECK, error.to_string()),
    }
}

fn check_secure_level(runner: &dyn CommandRunner) -> Finding {
    const CHECK: &str = "securelevel";
    match sysctl(runner, "kern.securelevel") {
        Some(level) => finding(Level::Info, CHECK, level),
        None => finding(Level::Warning, CHECK, "cannot be read"),
    }
}

fn check_linux(runner: &dyn CommandRunner) -> Vec<Finding> {
    let linuxulator = Linuxulator::detect(runner);
    let runtimes = discover(Path::new("/"), &linuxulator.emul_path);
    let state = match &linuxulator.osrelease {
        Some(release) => finding(
            Level::Ok,
            "linuxulator",
            format!("loaded, reports Linux {release}"),
        ),
        None => finding(
            Level::Info,
            "linuxulator",
            "not loaded (sysrc linux_enable=YES)",
        ),
    };
    let userlands = if runtimes.is_empty() {
        finding(Level::Info, "linux userland", "none installed")
    } else {
        let names: Vec<String> = runtimes
            .iter()
            .map(|runtime| runtime.root.display().to_string())
            .collect();
        finding(Level::Ok, "linux userland", names.join(", "))
    };
    vec![state, userlands]
}

fn check_sudo() -> Finding {
    if Path::new("/usr/local/bin/sudo").exists() {
        finding(Level::Ok, "sudo", "installed")
    } else {
        finding(Level::Info, "sudo", "not installed")
    }
}

fn check_trust() -> Finding {
    if Path::new("/usr/local/etc/meta/trust/root.json").exists() {
        finding(Level::Ok, "update trust", "root metadata installed")
    } else {
        finding(
            Level::Info,
            "update trust",
            "no root metadata; metaupd will refuse every update",
        )
    }
}

/// Update state inside the boot environment would be reverted by a rollback,
/// taking downgrade protection and the update history back with it.
#[cfg(unix)]
fn check_update_state(root: &Path, state: &Path) -> Finding {
    use std::os::unix::fs::MetadataExt;
    const CHECK: &str = "update state";
    let device = |path: &Path| std::fs::metadata(path).map(|m| m.dev());
    match (device(root), device(state)) {
        (Ok(root), Ok(state)) if root != state => {
            finding(Level::Ok, CHECK, "kept outside the boot environment")
        }
        (Ok(_), Ok(_)) => finding(
            Level::Warning,
            CHECK,
            format!(
                "{} is inside the boot environment; rollback would revert it",
                state.display()
            ),
        ),
        _ => finding(Level::Info, CHECK, "metaupd has not been used"),
    }
}

#[cfg(not(unix))]
fn check_update_state(_root: &Path, _state: &Path) -> Finding {
    finding(
        Level::Info,
        "update state",
        "not checked on this operating system",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn state_on_the_root_filesystem_is_a_warning() {
        let dir = std::env::temp_dir();
        assert_eq!(check_update_state(&dir, &dir).level, Level::Warning);
        let missing = dir.join("meta-doctor-missing");
        assert_eq!(check_update_state(&dir, &missing).level, Level::Info);
    }

    #[test]
    fn root_filesystem_from_mount_output() {
        let text = "zroot/ROOT/default\t/\tzfs\trw,noatime\t0 0\ndevfs\t/dev\tdevfs\trw\t0 0\n";
        assert_eq!(root_filesystem(text), Some("zfs"));
        assert_eq!(root_filesystem("devfs /dev devfs rw 0 0\n"), None);
    }
}
