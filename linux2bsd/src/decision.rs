//! Choosing how a program would run.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::database::{Advice, Database, Severity};
use crate::elf::{Abi, Arch, Binary, Libc, ProgramKind};
use crate::resolve::{Resolution, resolve_libraries};
use crate::runtime::{Linuxulator, Runtime};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    /// A FreeBSD program; no compatibility layer is involved.
    Native,
    /// The kernel runs it directly against the emulation path.
    Linuxulator { root: PathBuf },
    /// It needs a Linux userland other than the emulation path.
    OtherRoot { root: PathBuf },
    /// It cannot run as the system is now; the report says why.
    NotRunnable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub program: PathBuf,
    pub binary: Binary,
    pub decision: Decision,
    /// Library resolution in the best available root.
    pub resolution: Option<Resolution>,
    /// What prevents the program from running, and how to fix it.
    pub problems: Vec<String>,
    pub notes: Vec<String>,
    pub advice: Vec<Advice>,
}

/// Facts about the host that the decision depends on.
#[derive(Debug, Clone)]
pub struct HostState<'a> {
    pub machine_arch: &'a str,
    pub linuxulator: &'a Linuxulator,
    pub runtimes: &'a [Runtime],
}

pub fn analyze(
    program: &Path,
    binary: &Binary,
    host: &HostState<'_>,
    database: &Database,
) -> Report {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut report = Report {
        program: program.to_owned(),
        binary: binary.clone(),
        decision: Decision::NotRunnable,
        resolution: None,
        problems: Vec::new(),
        notes: Vec::new(),
        advice: database
            .advice_for(&name, binary)
            .into_iter()
            .cloned()
            .collect(),
    };
    if !matches!(binary.kind, ProgramKind::Executable) {
        report.problems.push("not an executable program".into());
        return report;
    }
    if !arch_supported(binary.arch, host.machine_arch) {
        let found = binary
            .arch
            .freebsd_name()
            .unwrap_or("an unknown architecture");
        report.problems.push(format!(
            "built for {found}; this machine is {} and linux2BSD does not translate machine code",
            host.machine_arch
        ));
        return report;
    }
    match &binary.abi {
        Abi::FreeBsd { .. } => {
            report.decision = Decision::Native;
            return report;
        }
        Abi::OtherBsd { name } => {
            report
                .problems
                .push(format!("{name} programs are not supported"));
            return report;
        }
        Abi::Unknown => {
            report.problems.push(
                "nothing in the file identifies its operating system; static Linux programs can \
                 look like this. If it is one, brandelf -t Linux marks it"
                    .into(),
            );
            return report;
        }
        Abi::Linux {
            libc: Libc::Bionic, ..
        } => {
            report
                .problems
                .push("Android programs are not supported".into());
            return report;
        }
        Abi::Linux { min_kernel, .. } => {
            if let (Some(needed), Some(emulated)) = (min_kernel, host.linuxulator.kernel_version())
                && *needed > emulated
            {
                report.problems.push(format!(
                    "requires Linux {}.{}.{} or newer; Linuxulator reports {}.{}.{}",
                    needed.0, needed.1, needed.2, emulated.0, emulated.1, emulated.2
                ));
            }
        }
    }
    if !host.linuxulator.loaded {
        report.problems.push(
            "Linux support is not loaded; run: sysrc linux_enable=YES && service linux start"
                .into(),
        );
    }
    let root = choose_root(program, binary, host, &mut report);
    if report
        .advice
        .iter()
        .any(|advice| advice.severity == Severity::Blocking)
    {
        report
            .problems
            .push("the compatibility database lists a blocking limitation".into());
    }
    if report.problems.is_empty()
        && let Some(runtime) = root
    {
        report.decision = if runtime.is_emul_path {
            Decision::Linuxulator {
                root: runtime.root.clone(),
            }
        } else {
            Decision::OtherRoot {
                root: runtime.root.clone(),
            }
        };
    }
    report
}

/// Picks the first root that provides everything; otherwise the best one, with
/// its gaps reported as problems.
fn choose_root(
    program: &Path,
    binary: &Binary,
    host: &HostState<'_>,
    report: &mut Report,
) -> Option<Runtime> {
    // A static program needs no userland, only the kernel.
    if binary.interpreter.is_none() && binary.needed.is_empty() {
        report
            .notes
            .push("statically linked; no Linux userland is needed".into());
        return Some(Runtime {
            root: host.linuxulator.emul_path.clone(),
            is_emul_path: true,
            distribution: None,
        });
    }
    let mut best: Option<(Runtime, Resolution)> = None;
    for runtime in host.runtimes {
        let resolution = resolve_libraries(&runtime.root, program, binary);
        if resolution.complete() {
            report.resolution = Some(resolution);
            return Some(runtime.clone());
        }
        let gaps = |r: &Resolution| {
            r.missing.len() + r.missing_versions.len() + usize::from(r.interpreter_missing) * 1000
        };
        if best
            .as_ref()
            .is_none_or(|(_, current)| gaps(&resolution) < gaps(current))
        {
            best = Some((runtime.clone(), resolution));
        }
    }
    let Some((runtime, resolution)) = best else {
        report
            .problems
            .push("no Linux userland is installed; for example: pkg install linux_base-rl9".into());
        return None;
    };
    let root = runtime.root.display();
    if resolution.interpreter_missing {
        let loader = binary.interpreter.as_deref().unwrap_or("unknown");
        report.problems.push(format!(
            "the dynamic loader {loader} is missing from {root}"
        ));
    }
    for (library, needed_by) in &resolution.missing {
        report.problems.push(format!(
            "{library} (needed by {needed_by}) is missing from {root}"
        ));
    }
    for (version, needed_by) in &resolution.missing_versions {
        report.problems.push(format!(
            "{version} (needed by {needed_by}) is not provided by {root}; the program was built \
             for a newer system"
        ));
    }
    report.resolution = Some(resolution);
    None
}

fn arch_supported(program: Arch, machine: &str) -> bool {
    match (program, machine) {
        // amd64 kernels run 32-bit Linux programs through linux.ko.
        (Arch::I386, "amd64" | "x86_64") | (Arch::Amd64, "x86_64") => true,
        (Arch::Aarch64, "aarch64" | "arm64") => true,
        (arch, machine) => arch.freebsd_name() == Some(machine),
    }
}
