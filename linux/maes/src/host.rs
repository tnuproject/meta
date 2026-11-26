//! What this host offers.

use std::path::Path;

use linux2bsd::database::Severity;
use linux2bsd::elf::{Abi, Binary, Libc};
use linux2bsd::{Database, Linuxulator, Runtime};
use meta_analyzer::{Kind, Profile};
use meta_common::{CommandRunner, Platform};
use meta_compat::{Root, State};
use meta_linux_vm::{Cgroups, Manager, VmConfig, VmState};
use meta_model::{Provision, RuntimeKind, Source};
use serde::Serialize;

use crate::decide::Candidate;

#[derive(Debug, Clone, Serialize)]
pub struct Userland {
    pub edition: String,
    pub path: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Guest {
    #[serde(flatten)]
    pub config: VmConfig,
    pub running: bool,
    /// What a running guest said about itself.
    pub kernel: Option<String>,
    pub engines: Vec<String>,
}

#[derive(Debug)]
pub struct Host {
    pub arch: String,
    pub linuxulator: Linuxulator,
    /// Userland at the emulation path.
    pub emulation: Option<Runtime>,
    pub userlands: Vec<Userland>,
    pub guests: Vec<Guest>,
    pub user: String,
}

/// One command, so that asking a guest costs one connection.
const PROBE: &str = "uname -r; for e in docker podman containerd; do \
    command -v $e >/dev/null 2>&1 && echo $e; done; true";

impl Host {
    pub fn detect(runner: &dyn CommandRunner, manager: &Manager<'_>, user: &str) -> Self {
        let platform = Platform::detect(runner);
        let linuxulator = Linuxulator::detect(runner);
        let emulation = linux2bsd::discover(Path::new("/"), &linuxulator.emul_path)
            .into_iter()
            .find(|runtime| runtime.is_emul_path);
        let userlands = Root::all(runner)
            .unwrap_or_default()
            .iter()
            .filter(|root| root.state() != State::Absent)
            .map(|root| Userland {
                edition: root.edition.id.clone(),
                path: root.path().to_owned(),
                running: root.state() == State::Running,
            })
            .collect();
        let guests = manager
            .list()
            .into_iter()
            .map(|config| Guest {
                running: manager.state(&config) == VmState::Running,
                config,
                kernel: None,
                engines: Vec::new(),
            })
            .collect();
        Self {
            arch: platform.arch,
            linuxulator,
            emulation,
            userlands,
            guests,
            user: user.to_owned(),
        }
    }

    /// Asks running guests about themselves. This takes a connection each, so it is
    /// left out when no guest comes into question.
    pub fn probe(&mut self, manager: &Manager<'_>) {
        for guest in self.guests.iter_mut().filter(|g| g.running) {
            if self.user != guest.config.owner && self.user != "root" {
                continue;
            }
            let connection = manager.connection(&guest.config);
            if let Ok((0, text)) = connection.capture(5, &["sh", "-c", PROBE]) {
                let mut lines = text.lines().map(str::to_owned);
                guest.kernel = lines.next();
                guest.engines = lines.collect();
            }
        }
    }

    /// The runtimes that could run this program, with what is known about each.
    pub fn candidates(&self, profile: &Profile) -> Vec<Candidate> {
        let mut candidates = vec![self.native(profile), self.direct(profile)];
        candidates.extend(self.userlands.iter().map(|u| self.userland(u, profile)));
        candidates.extend(self.guests.iter().map(|g| self.guest(g, profile)));
        candidates
    }

    fn native(&self, profile: &Profile) -> Candidate {
        let mut candidate = Candidate::new(RuntimeKind::Native);
        if let Some(binary) = &profile.binary {
            self.machine_code(binary, &mut candidate);
        }
        if let Some(line) = &profile.interpreter {
            let path = line.split_whitespace().next().unwrap_or_default();
            if !Path::new(path).exists() {
                candidate.obstacles.push(format!("there is no {path}"));
            }
        }
        candidate
    }

    fn machine_code(&self, binary: &Binary, candidate: &mut Candidate) {
        if binary.arch.freebsd_name() != Some(&self.arch) {
            candidate
                .obstacles
                .push(format!("built for another processor, not {}", self.arch));
        }
    }

    /// Limits of everything that runs on Linuxulator.
    fn linuxulator(&self, profile: &Profile, candidate: &mut Candidate) {
        candidate.kernel = self.linuxulator.kernel_version().map(|(a, b, _)| (a, b));
        let Some(binary) = &profile.binary else {
            return;
        };
        let machine = binary.arch.freebsd_name();
        let compatible =
            machine == Some(&self.arch) || (machine == Some("i386") && self.arch == "amd64");
        if !compatible {
            candidate
                .obstacles
                .push(format!("built for another processor, not {}", self.arch));
        }
        if matches!(
            binary.abi,
            Abi::Linux {
                libc: Libc::Bionic,
                ..
            }
        ) {
            candidate
                .obstacles
                .push("Android programs are not supported".into());
        }
        let name = profile
            .program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        for advice in Database::builtin().advice_for(&name, binary) {
            if advice.severity == Severity::Blocking {
                candidate.obstacles.push(advice.summary.clone());
            }
        }
    }

    fn libraries(root: &Path, profile: &Profile, candidate: &mut Candidate) {
        let Some(binary) = &profile.binary else {
            return;
        };
        let found = linux2bsd::resolve::resolve_libraries(root, &profile.program, binary);
        if found.interpreter_missing {
            candidate
                .obstacles
                .push("its program loader is not installed there".into());
        }
        for (library, _) in found.missing.iter().take(5) {
            candidate
                .obstacles
                .push(format!("{library} is not installed there"));
        }
        for (version, library) in found.missing_versions.iter().take(5) {
            candidate
                .obstacles
                .push(format!("{version} is missing ({library})"));
        }
    }

    fn direct(&self, profile: &Profile) -> Candidate {
        let mut candidate = Candidate::new(RuntimeKind::Linuxulator);
        self.linuxulator(profile, &mut candidate);
        if !self.linuxulator.loaded {
            candidate.obstacles.push(
                "Linux support is not loaded; run: sysrc linux_enable=YES && service linux start"
                    .into(),
            );
        }
        if profile.kind == Kind::Script {
            candidate
                .obstacles
                .push("the interpreter of a script decides".into());
        }
        let statically = profile
            .binary
            .as_ref()
            .is_some_and(|b| b.interpreter.is_none() && b.needed.is_empty());
        match &self.emulation {
            Some(runtime) => Self::libraries(&runtime.root, profile, &mut candidate),
            None if statically => {}
            None => candidate
                .obstacles
                .push("there is no Linux userland to use".into()),
        }
        candidate
    }

    fn userland(&self, userland: &Userland, profile: &Profile) -> Candidate {
        let mut candidate = Candidate::new(RuntimeKind::Userspace);
        candidate.instance = Some(userland.edition.clone());
        candidate.ready = userland.running;
        if !userland.running {
            candidate.preparation = Some(format!("sudo meta-compat start {}", userland.edition));
        }
        self.linuxulator(profile, &mut candidate);
        Self::libraries(Path::new(&userland.path), profile, &mut candidate);
        if let Some(line) = &profile.interpreter {
            let path = line.split_whitespace().next().unwrap_or_default();
            let inside = linux2bsd::resolve::path_in_root(Path::new(&userland.path), path);
            if inside.is_none_or(|p| !p.exists()) {
                candidate
                    .obstacles
                    .push(format!("there is no {path} in it"));
            }
        }
        candidate
    }

    fn guest(&self, guest: &Guest, profile: &Profile) -> Candidate {
        let mut candidate = Candidate::new(RuntimeKind::VirtualMachine);
        let config = &guest.config;
        candidate.instance = Some(config.name.clone());
        candidate.ready = guest.running;
        if !guest.running {
            candidate.preparation = Some(format!("sudo meta-linux vm start {}", config.name));
        }
        if self.user != config.owner && self.user != "root" {
            candidate
                .obstacles
                .push(format!("it belongs to {}", config.owner));
        }
        if let Some(binary) = &profile.binary {
            let machine = binary.arch.freebsd_name();
            if machine != Some("amd64") && machine != Some("i386") {
                candidate
                    .obstacles
                    .push("built for another processor, not amd64".into());
            }
            if matches!(
                binary.abi,
                Abi::Linux {
                    libc: Libc::Bionic,
                    ..
                }
            ) {
                candidate
                    .obstacles
                    .push("Android programs are not supported".into());
            }
        }
        for (version, requirement) in [
            (Cgroups::V1, "kernel:cgroup-v1"),
            (Cgroups::V2, "kernel:cgroup-v2"),
        ] {
            let provision = if config.cgroups == version {
                Provision::Provided
            } else {
                Provision::Missing
            };
            let detail = "the machine's configuration";
            candidate.fact(requirement, provision, Source::Observation, detail);
        }
        if let Some(kernel) = &guest.kernel {
            candidate.kernel = parse_version(kernel);
            for engine in ["docker", "podman", "containerd"] {
                let found = guest.engines.iter().any(|e| e == engine);
                let (provision, detail) = if found {
                    (Provision::Provided, "installed in the guest")
                } else {
                    (Provision::Configurable, "not installed in the guest")
                };
                candidate.fact(
                    &format!("container:{engine}"),
                    provision,
                    Source::Observation,
                    detail,
                );
            }
        }
        candidate
    }
}

/// Major and minor version of a kernel release such as `6.1.0-41-amd64`.
pub fn parse_version(release: &str) -> Option<(u32, u32)> {
    let mut parts = release.split(['.', '-']);
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("6.1.0-41-amd64"), Some((6, 1)));
        assert_eq!(parse_version("6.17.8-arch1-1"), Some((6, 17)));
        assert_eq!(parse_version("x"), None);
    }

    fn guest(cgroups: Cgroups, running: bool) -> Guest {
        Guest {
            config: VmConfig {
                name: "g".into(),
                image: "debian-12".into(),
                cpus: 1,
                memory_mib: 1024,
                disk_gib: 10,
                network: meta_linux_vm::Network::Isolated,
                cgroups,
                owner: "alice".into(),
                owner_uid: 1001,
                uuid: "00000000-1111-2222-3333-444444444444".into(),
                ssh_port: 22010,
                forwards: Vec::new(),
                shares: Vec::new(),
            },
            running,
            kernel: running.then(|| "6.1.0-41-amd64".into()),
            engines: vec!["podman".into()],
        }
    }

    fn host(guests: Vec<Guest>, user: &str) -> Host {
        Host {
            arch: "amd64".into(),
            linuxulator: Linuxulator {
                loaded: false,
                emul_path: "/compat/linux".into(),
                osrelease: None,
            },
            emulation: None,
            userlands: Vec::new(),
            guests,
            user: user.into(),
        }
    }

    fn script() -> Profile {
        Profile {
            program: "/x".into(),
            kind: Kind::Script,
            interpreter: Some("/nonexistent/sh".into()),
            binary: None,
            needs: Vec::new(),
            limitations: Vec::new(),
        }
    }

    #[test]
    fn guests_report_what_they_have() {
        let host = host(vec![guest(Cgroups::V1, true)], "alice");
        let candidates = host.candidates(&script());
        let vm = candidates.last().unwrap();
        assert_eq!(vm.kernel, Some((6, 1)));
        assert_eq!(vm.facts["kernel:cgroup-v1"].0, Provision::Provided);
        assert_eq!(vm.facts["kernel:cgroup-v2"].0, Provision::Missing);
        assert_eq!(vm.facts["container:podman"].0, Provision::Provided);
        assert_eq!(vm.facts["container:docker"].0, Provision::Configurable);
        assert!(vm.obstacles.is_empty() && vm.ready);
    }

    #[test]
    fn other_peoples_machines_are_not_used() {
        let host = host(vec![guest(Cgroups::V2, false)], "bob");
        let candidates = host.candidates(&script());
        let vm = candidates.last().unwrap();
        assert!(vm.obstacles[0].contains("alice"));
        assert!(vm.preparation.as_ref().unwrap().contains("vm start g"));
    }

    #[test]
    fn missing_pieces_are_obstacles() {
        let host = host(Vec::new(), "alice");
        let candidates = host.candidates(&script());
        assert!(candidates[0].obstacles[0].contains("/nonexistent/sh"));
        assert!(
            candidates[1]
                .obstacles
                .iter()
                .any(|o| o.contains("not loaded"))
        );
    }
}
