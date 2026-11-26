//! What each runtime provides.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::evidence::{Evidence, Source};
use crate::requirement::Requirement;

/// Where a program can run, least demanding first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    Native,
    Linuxulator,
    /// A managed Linux userland. Its kernel is still FreeBSD's Linuxulator.
    Userspace,
    /// A virtual machine with a Linux kernel of its own.
    VirtualMachine,
}

impl RuntimeKind {
    pub const ALL: [RuntimeKind; 4] = [
        RuntimeKind::Native,
        RuntimeKind::Linuxulator,
        RuntimeKind::Userspace,
        RuntimeKind::VirtualMachine,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Native => "native",
            RuntimeKind::Linuxulator => "linuxulator",
            RuntimeKind::Userspace => "userspace",
            RuntimeKind::VirtualMachine => "virtual-machine",
        }
    }

    /// Whether Linux programs here use FreeBSD's kernel.
    pub fn uses_linuxulator(self) -> bool {
        matches!(self, RuntimeKind::Linuxulator | RuntimeKind::Userspace)
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provision {
    Provided,
    /// Present with limits described in the evidence.
    Partial,
    /// Depends on how the runtime is configured.
    Configurable,
    Missing,
    /// Nobody has recorded whether it works.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub requirement: Requirement,
    pub summary: String,
    pub provision: BTreeMap<RuntimeKind, Provision>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InterfaceFile {
    schema: u32,
    #[serde(default)]
    interface: Vec<Interface>,
}

/// Linux system calls of one FreeBSD release.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemCalls {
    pub schema: u32,
    pub release: String,
    pub architecture: String,
    pub implemented: BTreeMap<String, u32>,
    pub placeholder: BTreeMap<String, u32>,
}

#[derive(Debug, Clone)]
pub struct Registry {
    pub interfaces: Vec<Interface>,
    pub system_calls: SystemCalls,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {reason}")]
pub struct RegistryError {
    file: &'static str,
    reason: String,
}

impl Registry {
    /// The registry shipped with this build.
    pub fn builtin() -> Self {
        Self::parse(
            include_str!("../../data/interfaces.toml"),
            include_str!("../../data/linuxulator-15.1.toml"),
        )
        .expect("the embedded registry is validated by the test suite")
    }

    pub fn parse(interfaces: &str, system_calls: &str) -> Result<Self, RegistryError> {
        let error = |file| move |reason: String| RegistryError { file, reason };
        let interfaces: InterfaceFile =
            toml::from_str(interfaces).map_err(|e| error("interfaces.toml")(e.to_string()))?;
        let system_calls: SystemCalls =
            toml::from_str(system_calls).map_err(|e| error("linuxulator")(e.to_string()))?;
        for schema in [interfaces.schema, system_calls.schema] {
            if schema != 1 {
                return Err(error("registry")(format!("unsupported schema {schema}")));
            }
        }
        for interface in &interfaces.interface {
            if interface.evidence.iter().all(|e| e.reference.is_none()) {
                let reason = format!("{} has no reference", interface.requirement);
                return Err(error("interfaces.toml")(reason));
            }
        }
        if let Some(both) = system_calls
            .implemented
            .keys()
            .find(|n| system_calls.placeholder.contains_key(*n))
        {
            return Err(error("linuxulator")(format!("{both} is listed twice")));
        }
        Ok(Self {
            interfaces: interfaces.interface,
            system_calls,
        })
    }

    /// Whether `runtime` provides `requirement`, and why that is believed.
    pub fn provision(
        &self,
        runtime: RuntimeKind,
        requirement: &Requirement,
    ) -> (Provision, Vec<Evidence>) {
        if let Some(interface) = self
            .interfaces
            .iter()
            .find(|i| i.requirement == *requirement)
        {
            let provision = interface
                .provision
                .get(&runtime)
                .copied()
                .unwrap_or(Provision::Unknown);
            return (provision, interface.evidence.clone());
        }
        match requirement {
            Requirement::Syscall(name) if runtime.uses_linuxulator() => self.linuxulator(name),
            Requirement::Syscall(_) if runtime == RuntimeKind::VirtualMachine => (
                // A real kernel; whether it is recent enough is a version question.
                Provision::Provided,
                vec![Evidence::new(
                    Source::Documentation,
                    "provided by the guest's Linux kernel",
                )],
            ),
            Requirement::Syscall(_) => (
                Provision::Missing,
                vec![Evidence::new(
                    Source::Documentation,
                    "FreeBSD programs use FreeBSD calls",
                )],
            ),
            _ => (Provision::Unknown, Vec::new()),
        }
    }

    fn linuxulator(&self, name: &str) -> (Provision, Vec<Evidence>) {
        let calls = &self.system_calls;
        let reference = format!("FreeBSD {} sys/compat/linux", calls.release);
        if calls.placeholder.contains_key(name) {
            let detail = format!("{name} is a placeholder that returns ENOSYS");
            (
                Provision::Missing,
                vec![Evidence::new(Source::SourceCode, detail).at(reference)],
            )
        } else if calls.implemented.contains_key(name) {
            // Present does not mean complete, which only testing can show.
            let detail = format!("{name} has an implementation");
            (
                Provision::Partial,
                vec![Evidence::new(Source::SourceCode, detail).at(reference)],
            )
        } else {
            let detail = format!("{name} is not in the system call table");
            (
                Provision::Missing,
                vec![Evidence::new(Source::SourceCode, detail).at(reference)],
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel(name: &str) -> Requirement {
        Requirement::Kernel(name.into())
    }

    #[test]
    fn builtin_registry_is_valid() {
        let registry = Registry::builtin();
        assert!(registry.system_calls.implemented.len() > 200);
        assert!(registry.system_calls.placeholder.contains_key("unshare"));
    }

    #[test]
    fn kernel_features_need_a_real_kernel() {
        let registry = Registry::builtin();
        for feature in ["namespaces", "cgroup-v1", "cgroup-v2", "modules"] {
            for runtime in [RuntimeKind::Linuxulator, RuntimeKind::Userspace] {
                let (provision, evidence) = registry.provision(runtime, &kernel(feature));
                assert_eq!(provision, Provision::Missing, "{feature} on {runtime}");
                assert!(!evidence.is_empty());
            }
            let (provision, _) = registry.provision(RuntimeKind::VirtualMachine, &kernel(feature));
            assert!(
                matches!(provision, Provision::Provided | Provision::Configurable),
                "{feature}"
            );
        }
    }

    #[test]
    fn system_calls_follow_the_source() {
        let registry = Registry::builtin();
        let call = |name: &str| Requirement::Syscall(name.into());
        for name in ["io_uring_setup", "setns", "bpf", "no_such_call"] {
            let (provision, _) = registry.provision(RuntimeKind::Linuxulator, &call(name));
            assert_eq!(provision, Provision::Missing, "{name}");
        }
        let (provision, _) = registry.provision(RuntimeKind::Userspace, &call("statx"));
        assert_eq!(provision, Provision::Partial);
        let (provision, _) = registry.provision(RuntimeKind::VirtualMachine, &call("bpf"));
        assert_eq!(provision, Provision::Provided);
    }

    #[test]
    fn interfaces_need_references() {
        let text = "schema = 1\n[[interface]]\nrequirement = \"kernel:x\"\nsummary = \"s\"\n\
                    provision = {}\nevidence = [{ source = \"documentation\", detail = \"d\" }]\n";
        let calls =
            "schema = 1\nrelease = \"r\"\narchitecture = \"amd64\"\n[implemented]\n[placeholder]\n";
        assert!(
            Registry::parse(text, calls)
                .unwrap_err()
                .to_string()
                .contains("no reference")
        );
    }
}
