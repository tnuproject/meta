//! Host platform identification.

use serde::Serialize;

use crate::command::CommandRunner;

pub const SYSCTL: &str = "/sbin/sysctl";
const FREEBSD_VERSION: &str = "/bin/freebsd-version";
const META_RELEASE: &str = "/etc/meta-release";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    FreeBsd,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub os: Os,
    /// Operating system name as reported by the compiler target.
    pub os_name: &'static str,
    pub arch: String,
    /// Running kernel release, for example `15.1-RELEASE`.
    pub kernel_release: Option<String>,
    /// Running kernel and userland patch levels from freebsd-version(1).
    pub kernel_version: Option<String>,
    pub userland_version: Option<String>,
    /// `__FreeBSD_version` of the running kernel.
    pub osreldate: Option<u32>,
    /// Contents of the META release marker, present only on an installed META system.
    pub meta_release: Option<MetaRelease>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetaRelease {
    pub version: String,
    pub edition: Option<String>,
}

impl Platform {
    pub fn detect(runner: &dyn CommandRunner) -> Self {
        let mut platform = Self {
            os: if cfg!(target_os = "freebsd") {
                Os::FreeBsd
            } else {
                Os::Other
            },
            os_name: std::env::consts::OS,
            arch: std::env::consts::ARCH.to_owned(),
            kernel_release: None,
            kernel_version: None,
            userland_version: None,
            osreldate: None,
            meta_release: None,
        };
        if platform.is_freebsd() {
            platform.fill_freebsd(runner);
            platform.meta_release = std::fs::read_to_string(META_RELEASE)
                .ok()
                .and_then(|text| parse_meta_release(&text));
        }
        platform
    }

    fn fill_freebsd(&mut self, runner: &dyn CommandRunner) {
        if let Some(arch) = sysctl(runner, "hw.machine_arch") {
            self.arch = arch;
        }
        self.kernel_release = sysctl(runner, "kern.osrelease");
        self.osreldate = sysctl(runner, "kern.osreldate").and_then(|v| v.parse().ok());
        if let Ok(text) = runner.stdout(FREEBSD_VERSION, &["-ku"]) {
            let mut lines = text.lines().map(|line| line.trim().to_owned());
            self.kernel_version = lines.next();
            self.userland_version = lines.next();
        }
    }

    pub fn is_freebsd(&self) -> bool {
        self.os == Os::FreeBsd
    }

    /// Explains why `what` cannot run here, or succeeds on FreeBSD.
    pub fn require_freebsd(&self, what: &str) -> Result<(), String> {
        if self.is_freebsd() {
            Ok(())
        } else {
            Err(format!(
                "{what} requires a FreeBSD kernel; this host runs {}",
                self.os_name
            ))
        }
    }
}

/// Reads one sysctl value, or `None` when it does not exist or cannot be read.
pub fn sysctl(runner: &dyn CommandRunner, name: &str) -> Option<String> {
    let value = runner.stdout(SYSCTL, &["-n", name]).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_meta_release(text: &str) -> Option<MetaRelease> {
    let field = |key: &str| {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| name.trim() == key)
            .map(|(_, value)| value.trim().trim_matches('"').to_owned())
    };
    Some(MetaRelease {
        version: field("VERSION")?,
        edition: field("EDITION"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::FakeRunner;

    #[test]
    fn reads_freebsd_details() {
        let runner = FakeRunner::new()
            .with("/sbin/sysctl -n hw.machine_arch", "amd64\n")
            .with("/sbin/sysctl -n kern.osrelease", "15.1-RELEASE\n")
            .with("/sbin/sysctl -n kern.osreldate", "1501000\n")
            .with(
                "/bin/freebsd-version -ku",
                "15.1-RELEASE\n15.1-RELEASE-p1\n",
            );
        let mut platform = Platform::detect(&FakeRunner::new());
        platform.fill_freebsd(&runner);
        assert_eq!(platform.arch, "amd64");
        assert_eq!(platform.osreldate, Some(1_501_000));
        assert_eq!(
            platform.userland_version.as_deref(),
            Some("15.1-RELEASE-p1")
        );
    }

    #[test]
    fn parses_meta_release_marker() {
        let release = parse_meta_release("VERSION=\"26.0\"\nEDITION=debian\n").unwrap();
        assert_eq!(release.version, "26.0");
        assert_eq!(release.edition.as_deref(), Some("debian"));
        assert_eq!(parse_meta_release("garbage"), None);
    }

    #[test]
    fn missing_sysctl_is_none() {
        assert_eq!(sysctl(&FakeRunner::new(), "kern.nope"), None);
    }
}
