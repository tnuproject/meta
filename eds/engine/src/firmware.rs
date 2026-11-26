//! Firmware that drivers load.
//!
//! FreeBSD's fwget(8) knows which firmware package a device needs, and pkg(8) knows
//! its version, origin and licence and verifies its files. META uses both and keeps
//! no list of its own and distributes no firmware.

use meta_common::{CommandError, CommandRunner};
use serde::Serialize;

use crate::host::PKG;

pub const FWGET: &str = "/usr/sbin/fwget";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirmwareState {
    Installed,
    Missing,
    /// Files differ from the package.
    Damaged,
    /// No repository offers it.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FirmwareReport {
    pub package: String,
    pub state: FirmwareState,
    pub version: Option<String>,
    pub origin: Option<String>,
    /// Licences as the package declares them; empty when it declares none.
    pub licenses: Vec<String>,
    pub repository: Option<String>,
}

/// Firmware packages the detected hardware needs, according to FreeBSD.
pub fn needed(runner: &dyn CommandRunner) -> Result<Vec<String>, CommandError> {
    let text = runner.stdout(FWGET, &["-n", "-q"])?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| valid_package(line))
        .map(str::to_owned)
        .collect())
}

pub fn valid_package(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b))
}

/// Describes one package, installed or not.
pub fn report(runner: &dyn CommandRunner, package: &str) -> Result<FirmwareReport, CommandError> {
    const FORMAT: &str = "%v\t%o\t%L\t%R";
    let mut report = FirmwareReport {
        package: package.to_owned(),
        state: FirmwareState::Unavailable,
        version: None,
        origin: None,
        licenses: Vec::new(),
        repository: None,
    };
    if !valid_package(package) {
        return Ok(report);
    }
    let installed = runner.run(PKG, &["query", FORMAT, package])?;
    if installed.success() && !installed.stdout.trim().is_empty() {
        fill(&mut report, &installed.stdout);
        let check = runner.run(PKG, &["check", "-s", "-q", package])?;
        report.state = if check.success() {
            FirmwareState::Installed
        } else {
            FirmwareState::Damaged
        };
        return Ok(report);
    }
    // -U: answer from the catalogue at hand, without touching the network.
    let remote = runner.run(PKG, &["rquery", "-U", FORMAT, package])?;
    if remote.success() && !remote.stdout.trim().is_empty() {
        fill(&mut report, &remote.stdout);
        report.state = FirmwareState::Missing;
    }
    Ok(report)
}

/// pkg prints one line per licence.
fn fill(report: &mut FirmwareReport, text: &str) {
    for line in text.lines() {
        let mut fields = line.split('\t').map(str::trim);
        let mut next = || fields.next().filter(|f| !f.is_empty()).map(str::to_owned);
        report.version = next().or(report.version.take());
        report.origin = next().or(report.origin.take());
        if let Some(license) = next()
            && !report.licenses.contains(&license)
        {
            report.licenses.push(license);
        }
        report.repository = next().or(report.repository.take());
    }
}

/// Firmware the kernel asked for and did not get, from its message buffer.
pub fn failures(dmesg: &str) -> Vec<String> {
    dmesg
        .lines()
        .filter(|line| {
            let line = line.to_ascii_lowercase();
            line.contains("firmware")
                && [
                    "could not load",
                    "failed to load",
                    "not found",
                    "error -2",
                    "unable to",
                ]
                .iter()
                .any(|text| line.contains(text))
        })
        .map(|line| line.trim().chars().take(200).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;

    const FORMAT: &str = "%v\t%o\t%L\t%R";

    #[test]
    fn missing_firmware_is_described_from_the_catalogue() {
        let runner = FakeRunner::new()
            .with(
                "/usr/sbin/fwget -n -q",
                "wifi-firmware-iwlwifi-kmod-ax210\n\n; rm -rf /\n",
            )
            .with_failure(
                &format!("{PKG} query {FORMAT} wifi-firmware-iwlwifi-kmod-ax210"),
                1,
                "",
            )
            .with(
                &format!("{PKG} rquery -U {FORMAT} wifi-firmware-iwlwifi-kmod-ax210"),
                "20260410\tnet/wifi-firmware-iwlwifi-kmod\tEULA\tFreeBSD-ports\n",
            );
        // Anything that is not a package name is dropped, not passed on.
        let needed = needed(&runner).unwrap();
        assert_eq!(needed, ["wifi-firmware-iwlwifi-kmod-ax210"]);
        let report = report(&runner, &needed[0]).unwrap();
        assert_eq!(report.state, FirmwareState::Missing);
        assert_eq!(report.licenses, ["EULA"]);
        assert_eq!(report.version.as_deref(), Some("20260410"));
    }

    #[test]
    fn damaged_firmware_is_noticed() {
        let runner = FakeRunner::new()
            .with(
                &format!("{PKG} query {FORMAT} fw"),
                "1\to/fw\tA\tr\n1\to/fw\tB\tr\n",
            )
            .with_failure(&format!("{PKG} check -s -q fw"), 1, "checksum mismatch");
        let report = report(&runner, "fw").unwrap();
        assert_eq!(report.state, FirmwareState::Damaged);
        assert_eq!(report.licenses, ["A", "B"]);
    }

    #[test]
    fn hostile_package_names_are_never_run() {
        for name in ["", "-y", "a b", "a;b", "../x", "$(x)"] {
            let report = report(&FakeRunner::new(), name).unwrap();
            assert_eq!(report.state, FirmwareState::Unavailable, "{name:?}");
        }
    }

    #[test]
    fn kernel_messages_about_firmware() {
        let text = "em0: link state changed\niwlwifi0: could not load firmware image 'x'\n\
                    amdgpu: Direct firmware load for amdgpu/y.bin failed with error -2\n\
                    firmware: 'z' version 1: 5 bytes loaded\n";
        assert_eq!(failures(text).len(), 2);
    }
}
