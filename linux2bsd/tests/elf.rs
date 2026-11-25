//! Identification and decisions on synthetic ELF files.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

use linux2bsd::decision::HostState;
use linux2bsd::{
    Abi, Arch, Database, Decision, ElfError, Libc, Linuxulator, ProgramKind, Report, Runtime,
    analyze, inspect_bytes,
};

/// Builds a minimal little-endian ELF64 image with the given segments.
struct Image {
    osabi: u8,
    kind: u16,
    machine: u16,
    interpreter: Option<&'static str>,
    note: Option<(&'static str, u32, Vec<u8>)>,
    needed: Vec<&'static str>,
    runpath: Option<&'static str>,
}

impl Default for Image {
    fn default() -> Self {
        Self {
            osabi: 0,
            kind: 3,
            machine: 62,
            interpreter: None,
            note: None,
            needed: Vec::new(),
            runpath: None,
        }
    }
}

impl Image {
    fn linux(needed: Vec<&'static str>) -> Self {
        Self {
            interpreter: Some("/lib64/ld-linux-x86-64.so.2"),
            needed,
            ..Self::default()
        }
    }

    fn library(needed: Vec<&'static str>) -> Self {
        Self {
            needed,
            ..Self::default()
        }
    }

    fn build(&self) -> Vec<u8> {
        const HEADERS: usize = 64 + 56 * 4;
        let mut body = Vec::new();
        let mut segments: Vec<(u32, usize, usize)> = Vec::new();
        let mut add = |kind: u32, data: &[u8], body: &mut Vec<u8>| {
            while !(HEADERS + body.len()).is_multiple_of(8) {
                body.push(0);
            }
            let offset = HEADERS + body.len();
            body.extend_from_slice(data);
            segments.push((kind, offset, data.len()));
            offset
        };
        if let Some(path) = self.interpreter {
            add(3, &[path.as_bytes(), b"\0"].concat(), &mut body);
        }
        if let Some((name, kind, desc)) = &self.note {
            let mut note = Vec::new();
            for word in [name.len() as u32 + 1, desc.len() as u32, *kind] {
                note.extend_from_slice(&word.to_le_bytes());
            }
            note.extend_from_slice(name.as_bytes());
            note.push(0);
            note.resize(note.len().next_multiple_of(4), 0);
            note.extend_from_slice(desc);
            note.resize(note.len().next_multiple_of(4), 0);
            add(4, &note, &mut body);
        }
        if !self.needed.is_empty() {
            let mut strings = vec![0u8];
            let mut entries: Vec<(u64, u64)> = Vec::new();
            for (tag, text) in self
                .needed
                .iter()
                .map(|n| (1u64, *n))
                .chain(self.runpath.map(|path| (29, path)))
            {
                entries.push((tag, strings.len() as u64));
                strings.extend_from_slice(text.as_bytes());
                strings.push(0);
            }
            while !(HEADERS + body.len()).is_multiple_of(8) {
                body.push(0);
            }
            let strtab = (HEADERS + body.len()) as u64;
            body.extend_from_slice(&strings);
            entries.extend([(5, strtab), (10, strings.len() as u64), (0, 0)]);
            let dynamic: Vec<u8> = entries
                .iter()
                .flat_map(|(tag, value)| [tag.to_le_bytes(), value.to_le_bytes()].concat())
                .collect();
            add(2, &dynamic, &mut body);
        }
        let total = HEADERS + body.len();
        segments.insert(0, (1, 0, total));
        let mut image = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, self.osabi];
        image.resize(16, 0);
        image.extend_from_slice(&self.kind.to_le_bytes());
        image.extend_from_slice(&self.machine.to_le_bytes());
        image.extend_from_slice(&1u32.to_le_bytes());
        image.extend_from_slice(&0u64.to_le_bytes());
        image.extend_from_slice(&64u64.to_le_bytes());
        image.extend_from_slice(&0u64.to_le_bytes());
        image.extend_from_slice(&0u32.to_le_bytes());
        for half in [64u16, 56, segments.len() as u16, 64, 0, 0] {
            image.extend_from_slice(&half.to_le_bytes());
        }
        for (kind, offset, size) in &segments {
            image.extend_from_slice(&kind.to_le_bytes());
            image.extend_from_slice(&4u32.to_le_bytes());
            for value in [*offset, *offset, *offset, *size, *size, 4] {
                image.extend_from_slice(&(value as u64).to_le_bytes());
            }
        }
        image.resize(HEADERS, 0);
        image.extend_from_slice(&body);
        image
    }
}

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn identifies_glibc_program() {
    let image = Image {
        note: Some(("GNU", 1, words(&[0, 3, 2, 0]))),
        runpath: Some("$ORIGIN/lib:/opt/lib"),
        ..Image::linux(vec!["libc.so.6", "libm.so.6"])
    };
    let binary = inspect_bytes(&image.build()).unwrap();
    assert_eq!(
        binary.abi,
        Abi::Linux {
            libc: Libc::Glibc,
            min_kernel: Some((3, 2, 0))
        }
    );
    assert_eq!(binary.kind, ProgramKind::Executable);
    assert_eq!(binary.arch, Arch::Amd64);
    assert_eq!(binary.needed, ["libc.so.6", "libm.so.6"]);
    assert_eq!(binary.search_paths, ["$ORIGIN/lib", "/opt/lib"]);
}

#[test]
fn identifies_musl_and_android_by_loader() {
    let musl = Image {
        interpreter: Some("/lib/ld-musl-x86_64.so.1"),
        ..Image::default()
    };
    assert_eq!(
        inspect_bytes(&musl.build()).unwrap().abi,
        Abi::Linux {
            libc: Libc::Musl,
            min_kernel: None
        }
    );
    let android = Image {
        interpreter: Some("/system/bin/linker64"),
        ..Image::default()
    };
    assert_eq!(
        inspect_bytes(&android.build()).unwrap().abi,
        Abi::Linux {
            libc: Libc::Bionic,
            min_kernel: None
        }
    );
}

#[test]
fn identifies_freebsd_by_each_mark() {
    let by_osabi = Image {
        osabi: 9,
        kind: 2,
        ..Image::default()
    };
    assert_eq!(
        inspect_bytes(&by_osabi.build()).unwrap().abi,
        Abi::FreeBsd { osreldate: None }
    );
    let by_note = Image {
        note: Some(("FreeBSD", 1, words(&[1_501_000]))),
        kind: 2,
        ..Image::default()
    };
    assert_eq!(
        inspect_bytes(&by_note.build()).unwrap().abi,
        Abi::FreeBsd {
            osreldate: Some(1_501_000)
        }
    );
    let by_loader = Image {
        interpreter: Some("/libexec/ld-elf.so.1"),
        ..Image::default()
    };
    assert_eq!(
        inspect_bytes(&by_loader.build()).unwrap().abi,
        Abi::FreeBsd { osreldate: None }
    );
}

#[test]
fn unmarked_static_program_is_unknown() {
    let image = Image {
        kind: 2,
        ..Image::default()
    };
    assert_eq!(inspect_bytes(&image.build()).unwrap().abi, Abi::Unknown);
}

#[test]
fn non_elf_input() {
    assert!(matches!(inspect_bytes(b""), Err(ElfError::NotElf)));
    assert!(matches!(inspect_bytes(b"MZ"), Err(ElfError::NotElf)));
    match inspect_bytes(b"#! /bin/sh -e\necho") {
        Err(ElfError::Script(line)) => assert_eq!(line, "/bin/sh -e"),
        other => panic!("{other:?}"),
    }
}

/// Every truncation and every single-byte corruption must return, not panic.
#[test]
fn malformed_input_never_panics() {
    let image = Image {
        note: Some(("GNU", 1, words(&[0, 3, 2, 0]))),
        runpath: Some("/opt/lib"),
        ..Image::linux(vec!["libc.so.6"])
    }
    .build();
    for length in 0..image.len() {
        let _ = inspect_bytes(&image[..length]);
    }
    for index in 0..image.len() {
        for value in [0x00, 0x7f, 0xff] {
            let mut copy = image.clone();
            copy[index] = value;
            let _ = inspect_bytes(&copy);
        }
    }
}

struct Host {
    dir: tempfile::TempDir,
    linuxulator: Linuxulator,
}

impl Host {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
            linuxulator: Linuxulator {
                loaded: true,
                emul_path: PathBuf::from("/compat/linux"),
                osrelease: Some("5.15.0".into()),
            },
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    fn put(&self, path: &str, image: &Image) {
        let path = self.root().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, image.build()).unwrap();
    }

    fn analyze(&self, image: &Image) -> Report {
        let binary = inspect_bytes(&image.build()).unwrap();
        let runtimes = [Runtime {
            root: self.root(),
            is_emul_path: true,
            distribution: None,
        }];
        let host = HostState {
            machine_arch: "amd64",
            linuxulator: &self.linuxulator,
            runtimes: if self.root().exists() { &runtimes } else { &[] },
        };
        analyze(
            Path::new("/opt/app/bin/app"),
            &binary,
            &host,
            &Database::builtin(),
        )
    }
}

#[test]
fn complete_userland_is_selected() {
    let host = Host::new();
    host.put("lib64/ld-linux-x86-64.so.2", &Image::library(vec![]));
    host.put(
        "lib64/libc.so.6",
        &Image::library(vec!["ld-linux-x86-64.so.2"]),
    );
    let report = host.analyze(&Image::linux(vec!["libc.so.6"]));
    assert_eq!(
        report.decision,
        Decision::Linuxulator { root: host.root() },
        "{report:?}"
    );
    assert_eq!(report.resolution.unwrap().found.len(), 2);
}

#[test]
fn transitive_missing_library_is_named() {
    let host = Host::new();
    host.put("lib64/ld-linux-x86-64.so.2", &Image::library(vec![]));
    host.put("lib64/libfoo.so.1", &Image::library(vec!["libbar.so.2"]));
    let report = host.analyze(&Image::linux(vec!["libfoo.so.1"]));
    assert_eq!(report.decision, Decision::NotRunnable);
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("libbar.so.2 (needed by libfoo.so.1)")),
        "{:?}",
        report.problems
    );
}

#[test]
fn wrong_architecture_library_does_not_satisfy() {
    let host = Host::new();
    host.put("lib64/ld-linux-x86-64.so.2", &Image::library(vec![]));
    host.put(
        "lib64/libc.so.6",
        &Image {
            machine: 183,
            ..Image::library(vec![])
        },
    );
    let report = host.analyze(&Image::linux(vec!["libc.so.6"]));
    assert!(report.problems.iter().any(|p| p.contains("libc.so.6")));
}

#[test]
fn freebsd_library_does_not_satisfy() {
    let host = Host::new();
    host.put("lib64/ld-linux-x86-64.so.2", &Image::library(vec![]));
    host.put(
        "lib64/libz.so.1",
        &Image {
            osabi: 9,
            ..Image::library(vec![])
        },
    );
    let report = host.analyze(&Image::linux(vec!["libz.so.1"]));
    assert!(report.problems.iter().any(|p| p.contains("libz.so.1")));
}

#[test]
fn runpath_is_searched_in_the_root() {
    let host = Host::new();
    host.put("lib64/ld-linux-x86-64.so.2", &Image::library(vec![]));
    host.put("opt/app/bin/app", &Image::linux(vec![]));
    host.put("opt/app/lib/libx.so", &Image::library(vec![]));
    let image = Image {
        runpath: Some("$ORIGIN/../lib"),
        ..Image::linux(vec!["libx.so"])
    };
    let report = host.analyze(&image);
    assert!(
        matches!(report.decision, Decision::Linuxulator { .. }),
        "{:?}",
        report.problems
    );
}

#[test]
fn unloaded_linuxulator_is_a_problem_with_a_fix() {
    let mut host = Host::new();
    host.linuxulator = Linuxulator {
        loaded: false,
        emul_path: PathBuf::from("/compat/linux"),
        osrelease: None,
    };
    let report = host.analyze(&Image::linux(vec!["libc.so.6"]));
    assert!(report.problems.iter().any(|p| p.contains("linux_enable")));
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.contains("no Linux userland"))
    );
}

#[test]
fn newer_kernel_requirement_is_reported() {
    let host = Host::new();
    let image = Image {
        note: Some(("GNU", 1, words(&[0, 6, 1, 0]))),
        kind: 2,
        ..Image::default()
    };
    let report = host.analyze(&image);
    assert_eq!(report.decision, Decision::NotRunnable);
    assert!(report.problems[0].contains("requires Linux 6.1.0"));
}

#[test]
fn static_linux_program_needs_no_userland() {
    let host = Host::new();
    let image = Image {
        note: Some(("GNU", 1, words(&[0, 3, 2, 0]))),
        kind: 2,
        ..Image::default()
    };
    assert!(matches!(
        host.analyze(&image).decision,
        Decision::Linuxulator { .. }
    ));
}

#[test]
fn other_architectures_are_refused() {
    let host = Host::new();
    let image = Image {
        machine: 183,
        ..Image::linux(vec![])
    };
    let report = host.analyze(&image);
    assert!(report.problems[0].contains("does not translate machine code"));
}

#[test]
fn freebsd_program_is_native() {
    let host = Host::new();
    let image = Image {
        osabi: 9,
        kind: 2,
        ..Image::default()
    };
    assert_eq!(host.analyze(&image).decision, Decision::Native);
}

/// A Linux host's own programs must resolve completely against its own root.
#[cfg(target_os = "linux")]
#[test]
fn host_shell_resolves_against_the_host() {
    let program = fs::canonicalize("/bin/sh").unwrap();
    let binary = linux2bsd::inspect(&program).unwrap();
    assert!(matches!(binary.abi, Abi::Linux { .. }));
    let resolution = linux2bsd::resolve_libraries(Path::new("/"), &program, &binary);
    assert!(resolution.complete(), "{resolution:?}");
    if let Abi::Linux {
        libc: Libc::Glibc, ..
    } = binary.abi
    {
        assert!(
            binary
                .needed_versions
                .iter()
                .any(|(_, v)| v.starts_with("GLIBC_"))
        );
        let too_new = linux2bsd::Binary {
            needed_versions: vec![("libc.so.6".into(), "GLIBC_99.0".into())],
            ..binary
        };
        let resolution = linux2bsd::resolve_libraries(Path::new("/"), &program, &too_new);
        assert!(
            resolution
                .missing_versions
                .contains_key("libc.so.6 GLIBC_99.0")
        );
    }
}
