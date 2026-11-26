//! Needs that the file states.

use linux2bsd::elf::{Abi, Binary};
use meta_model::{Evidence, Need, Requirement, Source};

/// Libraries that are the interface to something only some runtimes have. A program
/// linked with them was built to use it, though it may cope without.
const LIBRARIES: &[(&str, &str, Source)] = &[
    ("liburing.so", "kernel:io-uring", Source::StaticAnalysis),
    ("libbpf.so", "kernel:bpf", Source::StaticAnalysis),
    ("libfuse.so", "kernel:fuse", Source::StaticAnalysis),
    ("libfuse3.so", "kernel:fuse", Source::StaticAnalysis),
    ("libcgroup.so", "kernel:cgroup-v1", Source::Heuristic),
    ("libkmod.so", "kernel:modules", Source::Heuristic),
    ("libsystemd.so", "init:systemd", Source::Heuristic),
    ("libnftables.so", "kernel:netfilter", Source::StaticAnalysis),
    ("libiptc.so", "kernel:netfilter", Source::StaticAnalysis),
    ("libX11.so", "desktop:x11", Source::Heuristic),
    ("libxcb.so", "desktop:x11", Source::Heuristic),
    ("libwayland-client.so", "desktop:wayland", Source::Heuristic),
    ("libasound.so", "desktop:audio", Source::Heuristic),
    ("libpulse.so", "desktop:audio", Source::Heuristic),
    ("libGL.so", "device:gpu", Source::Heuristic),
    ("libEGL.so", "device:gpu", Source::Heuristic),
    ("libvulkan.so", "device:gpu", Source::Heuristic),
    ("libdrm.so", "device:gpu", Source::Heuristic),
];

pub fn needs(binary: &Binary) -> Vec<Need> {
    let mut needs = Vec::new();
    let mut add = |requirement, source, detail: String| {
        Need::add(&mut needs, requirement, Evidence::new(source, detail));
    };
    let stated = Source::StaticAnalysis;
    if let Some(arch) = binary.arch.freebsd_name() {
        add(
            Requirement::Arch(arch.into()),
            stated,
            "ELF machine type".into(),
        );
    }
    match &binary.abi {
        Abi::FreeBsd { .. } => {
            add(
                Requirement::Abi("freebsd".into()),
                stated,
                binary.evidence.join(", "),
            );
        }
        Abi::Linux { min_kernel, .. } => {
            add(
                Requirement::Abi("linux".into()),
                stated,
                binary.evidence.join(", "),
            );
            if let Some((major, minor, _)) = min_kernel {
                let requirement = Requirement::KernelVersion(*major, *minor);
                add(requirement, stated, "GNU ABI tag note".into());
            }
        }
        Abi::OtherBsd { .. } | Abi::Unknown => {}
    }
    if !matches!(binary.abi, Abi::Linux { .. }) {
        return needs;
    }
    for library in &binary.needed {
        let requirement = Requirement::Library(library.clone());
        add(requirement, stated, format!("linked with {library}"));
        for (prefix, requirement, source) in LIBRARIES {
            if library.starts_with(prefix)
                && let Ok(requirement) = requirement.parse()
            {
                add(requirement, *source, format!("linked with {library}"));
            }
        }
    }
    for (library, version) in &binary.needed_versions {
        let requirement = Requirement::SymbolVersion(format!("{library} {version}"));
        add(requirement, stated, "symbol version table".into());
    }
    needs
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use linux2bsd::elf::{Arch, Libc, ProgramKind};

    pub(crate) fn linux(needed: &[&str]) -> Binary {
        Binary {
            kind: ProgramKind::Executable,
            arch: Arch::Amd64,
            bits: 64,
            abi: Abi::Linux {
                libc: Libc::Glibc,
                min_kernel: Some((6, 1, 0)),
            },
            evidence: vec!["test".into()],
            interpreter: None,
            soname: None,
            needed: needed.iter().map(|s| (*s).to_owned()).collect(),
            search_paths: Vec::new(),
            needed_versions: vec![("libc.so.6".into(), "GLIBC_2.38".into())],
            defined_versions: Vec::new(),
            appimage: None,
        }
    }

    #[test]
    fn stated_needs_are_firm() {
        let needs = needs(&linux(&["libc.so.6", "liburing.so.2", "libX11.so.6"]));
        let find = |text: &str| needs.iter().find(|n| n.requirement.to_string() == text);
        for firm in [
            "arch:amd64",
            "abi:linux",
            "kernel-version:6.1",
            "library:libc.so.6",
            "symbol-version:libc.so.6 GLIBC_2.38",
            "kernel:io-uring",
        ] {
            assert!(find(firm).unwrap().is_firm(), "{firm}");
        }
        // Many programs link X libraries and also work without a display.
        assert!(!find("desktop:x11").unwrap().is_firm());
    }
}
