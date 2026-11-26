//! What a program or a driver needs.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::evidence::{Evidence, Source};

/// A requirement with a stable textual form, such as `kernel:cgroup-v2`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Requirement {
    /// CPU architecture, in FreeBSD's naming.
    Arch(String),
    /// Operating system interface: `freebsd`, `linux`.
    Abi(String),
    /// A shared library by its soname.
    Library(String),
    /// A symbol version of a library: `libc.so.6 GLIBC_2.38`.
    SymbolVersion(String),
    /// A Linux system call.
    Syscall(String),
    /// A kernel feature: `namespaces`, `cgroup-v1`, `cgroup-v2`, `modules`, ...
    Kernel(String),
    /// Lowest Linux kernel version, `6.1`.
    KernelVersion(u32, u32),
    /// A service environment: `systemd`.
    Init(String),
    /// A container engine.
    Container(String),
    /// Device access: `gpu`, `kvm`, `fuse`, `tun`.
    Device(String),
    /// A desktop facility: `x11`, `wayland`, `audio`.
    Desktop(String),
    /// A LinuxKPI interface a driver uses.
    LinuxKpi(String),
    /// A firmware file or package.
    Firmware(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid requirement {0:?}")]
pub struct RequirementError(String);

impl Requirement {
    /// Kind and value of the textual form.
    fn parts(&self) -> (&'static str, String) {
        match self {
            Requirement::Arch(v) => ("arch", v.clone()),
            Requirement::Abi(v) => ("abi", v.clone()),
            Requirement::Library(v) => ("library", v.clone()),
            Requirement::SymbolVersion(v) => ("symbol-version", v.clone()),
            Requirement::Syscall(v) => ("syscall", v.clone()),
            Requirement::Kernel(v) => ("kernel", v.clone()),
            Requirement::KernelVersion(major, minor) => {
                ("kernel-version", format!("{major}.{minor}"))
            }
            Requirement::Init(v) => ("init", v.clone()),
            Requirement::Container(v) => ("container", v.clone()),
            Requirement::Device(v) => ("device", v.clone()),
            Requirement::Desktop(v) => ("desktop", v.clone()),
            Requirement::LinuxKpi(v) => ("linuxkpi", v.clone()),
            Requirement::Firmware(v) => ("firmware", v.clone()),
        }
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, value) = self.parts();
        write!(f, "{kind}:{value}")
    }
}

impl FromStr for Requirement {
    type Err = RequirementError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || RequirementError(text.chars().take(100).collect());
        let (kind, value) = text.split_once(':').ok_or_else(error)?;
        let printable = !value.is_empty()
            && value.len() <= 200
            && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ');
        if !printable {
            return Err(error());
        }
        let owned = value.to_owned();
        Ok(match kind {
            "arch" => Requirement::Arch(owned),
            "abi" => Requirement::Abi(owned),
            "library" => Requirement::Library(owned),
            "symbol-version" => Requirement::SymbolVersion(owned),
            "syscall" => Requirement::Syscall(owned),
            "kernel" => Requirement::Kernel(owned),
            "kernel-version" => {
                let (major, minor) = value.split_once('.').ok_or_else(error)?;
                Requirement::KernelVersion(
                    major.parse().map_err(|_| error())?,
                    minor.parse().map_err(|_| error())?,
                )
            }
            "init" => Requirement::Init(owned),
            "container" => Requirement::Container(owned),
            "device" => Requirement::Device(owned),
            "desktop" => Requirement::Desktop(owned),
            "linuxkpi" => Requirement::LinuxKpi(owned),
            "firmware" => Requirement::Firmware(owned),
            _ => return Err(error()),
        })
    }
}

impl TryFrom<String> for Requirement {
    type Error = RequirementError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Requirement> for String {
    fn from(value: Requirement) -> Self {
        value.to_string()
    }
}

/// A requirement together with the reasons for believing in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Need {
    pub requirement: Requirement,
    pub evidence: Vec<Evidence>,
}

impl Need {
    pub fn new(requirement: Requirement, evidence: Evidence) -> Self {
        Self {
            requirement,
            evidence: vec![evidence],
        }
    }

    /// The strongest source behind this need.
    pub fn strength(&self) -> Source {
        self.evidence
            .iter()
            .map(|e| e.source)
            .max()
            .unwrap_or(Source::Heuristic)
    }

    /// Whether the need should decide anything. A text found in a file only
    /// hints that a program might use something.
    pub fn is_firm(&self) -> bool {
        self.strength() > Source::Heuristic
    }

    /// Adds a need to a list, merging evidence for the same requirement.
    pub fn add(needs: &mut Vec<Need>, requirement: Requirement, evidence: Evidence) {
        match needs
            .iter_mut()
            .find(|need| need.requirement == requirement)
        {
            Some(need) if !need.evidence.contains(&evidence) => need.evidence.push(evidence),
            Some(_) => {}
            None => needs.push(Need::new(requirement, evidence)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textual_form_round_trips() {
        for text in [
            "arch:amd64",
            "symbol-version:libc.so.6 GLIBC_2.38",
            "syscall:io_uring_setup",
            "kernel:cgroup-v2",
            "kernel-version:6.1",
            "linuxkpi:mhi_sync_power_up",
        ] {
            let requirement: Requirement = text.parse().unwrap();
            assert_eq!(requirement.to_string(), text);
            let json = serde_json::to_string(&requirement).unwrap();
            assert_eq!(json, format!("\"{text}\""));
        }
    }

    #[test]
    fn malformed_requirements_are_rejected() {
        for text in [
            "",
            "kernel",
            "kernel:",
            "nope:x",
            "kernel-version:6",
            "syscall:a\nb",
        ] {
            assert!(text.parse::<Requirement>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn evidence_for_one_requirement_is_merged_and_ranked() {
        let mut needs = Vec::new();
        let requirement = Requirement::Kernel("namespaces".into());
        Need::add(
            &mut needs,
            requirement.clone(),
            Evidence::new(Source::Heuristic, "text"),
        );
        assert!(!needs[0].is_firm());
        Need::add(
            &mut needs,
            requirement,
            Evidence::new(Source::StaticAnalysis, "imports unshare"),
        );
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].strength(), Source::StaticAnalysis);
        assert!(needs[0].is_firm());
    }
}
