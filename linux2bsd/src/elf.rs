//! Executable identification.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use goblin::elf::{Elf, header, program_header};
use serde::Serialize;

/// Files larger than this are not read into memory.
const MAX_FILE_SIZE: u64 = 1 << 30;
const VER_FLG_WEAK: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Arch {
    Amd64,
    I386,
    Aarch64,
    Arm,
    Riscv64,
    Other(u16),
}

impl Arch {
    /// FreeBSD `MACHINE_ARCH` name.
    pub fn freebsd_name(self) -> Option<&'static str> {
        match self {
            Arch::Amd64 => Some("amd64"),
            Arch::I386 => Some("i386"),
            Arch::Aarch64 => Some("aarch64"),
            Arch::Arm => Some("armv7"),
            Arch::Riscv64 => Some("riscv64"),
            Arch::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Libc {
    Glibc,
    Musl,
    Bionic,
    /// No dynamic loader: statically linked.
    None,
    Unknown,
}

/// The operating system interface a program was built for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "os", rename_all = "snake_case")]
pub enum Abi {
    Linux {
        libc: Libc,
        min_kernel: Option<(u32, u32, u32)>,
    },
    FreeBsd {
        osreldate: Option<u32>,
    },
    OtherBsd {
        name: String,
    },
    /// Nothing in the file identifies the operating system.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramKind {
    Executable,
    SharedObject,
    Core,
    Relocatable,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Binary {
    pub kind: ProgramKind,
    pub arch: Arch,
    pub bits: u8,
    pub abi: Abi,
    /// What the ABI identification is based on.
    pub evidence: Vec<String>,
    pub interpreter: Option<String>,
    pub soname: Option<String>,
    pub needed: Vec<String>,
    pub search_paths: Vec<String>,
    /// Symbol versions needed, as (library, version), such as GLIBC_2.38.
    pub needed_versions: Vec<(String, String)>,
    /// Symbol versions this object provides.
    pub defined_versions: Vec<String>,
    /// AppImage type, when the file is also an AppImage.
    pub appimage: Option<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum ElfError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not an ELF file")]
    NotElf,
    #[error("a script run by {0}")]
    Script(String),
    #[error("file is too large to inspect")]
    TooLarge,
    #[error("malformed ELF file: {0}")]
    Malformed(String),
}

pub fn inspect(path: &Path) -> Result<Binary, ElfError> {
    let file = File::open(path)?;
    let mut head = Vec::new();
    (&file).take(4).read_to_end(&mut head)?;
    // Avoids reading large non-ELF files completely.
    let limit = if head.starts_with(header::ELFMAG) {
        MAX_FILE_SIZE
    } else {
        4096
    };
    let mut data = head;
    file.take(limit).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE_SIZE {
        return Err(ElfError::TooLarge);
    }
    inspect_bytes(&data)
}

pub fn inspect_bytes(data: &[u8]) -> Result<Binary, ElfError> {
    if let Some(rest) = data.strip_prefix(b"#!") {
        let line = rest.split(|b| *b == b'\n').next().unwrap_or_default();
        return Err(ElfError::Script(
            String::from_utf8_lossy(line).trim().to_owned(),
        ));
    }
    if !data.starts_with(header::ELFMAG) {
        return Err(ElfError::NotElf);
    }
    let elf = Elf::parse(data).map_err(|e| ElfError::Malformed(e.to_string()))?;
    let interpreter = elf.interpreter.map(str::to_owned);
    let (abi, evidence) = identify_abi(&elf, data, interpreter.as_deref());
    let split = |paths: &[&str]| -> Vec<String> {
        paths
            .iter()
            .flat_map(|p| p.split(':'))
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    Ok(Binary {
        kind: program_kind(&elf, interpreter.is_some()),
        arch: match elf.header.e_machine {
            header::EM_X86_64 => Arch::Amd64,
            header::EM_386 => Arch::I386,
            header::EM_AARCH64 => Arch::Aarch64,
            header::EM_ARM => Arch::Arm,
            header::EM_RISCV if elf.is_64 => Arch::Riscv64,
            other => Arch::Other(other),
        },
        bits: if elf.is_64 { 64 } else { 32 },
        abi,
        evidence,
        interpreter,
        soname: elf.soname.map(str::to_owned),
        needed: elf.libraries.iter().map(|s| (*s).to_owned()).collect(),
        // DT_RUNPATH makes the dynamic loader ignore DT_RPATH.
        search_paths: if elf.runpaths.is_empty() {
            split(&elf.rpaths)
        } else {
            split(&elf.runpaths)
        },
        needed_versions: needed_versions(&elf),
        defined_versions: defined_versions(&elf),
        appimage: match data.get(8..11) {
            Some([b'A', b'I', kind @ (1 | 2)]) => Some(*kind),
            _ => None,
        },
    })
}

fn needed_versions(elf: &Elf<'_>) -> Vec<(String, String)> {
    let mut versions = Vec::new();
    for need in elf.verneed.iter().flat_map(|section| section.iter()) {
        let Some(library) = elf.dynstrtab.get_at(need.vn_file) else {
            continue;
        };
        for aux in need.iter() {
            // Weak references are optional at run time.
            let weak = aux.vna_flags & VER_FLG_WEAK != 0;
            if let (false, Some(version)) = (weak, elf.dynstrtab.get_at(aux.vna_name)) {
                versions.push((library.to_owned(), version.to_owned()));
            }
        }
    }
    versions
}

fn defined_versions(elf: &Elf<'_>) -> Vec<String> {
    elf.verdef
        .iter()
        .flat_map(|section| section.iter())
        // The first auxiliary entry is the version's own name; the rest are parents.
        .filter_map(|definition| definition.iter().next())
        .filter_map(|aux| elf.dynstrtab.get_at(aux.vda_name))
        .map(str::to_owned)
        .collect()
}

fn program_kind(elf: &Elf<'_>, has_interpreter: bool) -> ProgramKind {
    const DF_1_PIE: u64 = 0x0800_0000;
    match elf.header.e_type {
        header::ET_EXEC => ProgramKind::Executable,
        // Position-independent executables and shared objects share a type.
        header::ET_DYN => {
            let pie_flag = elf
                .dynamic
                .as_ref()
                .is_some_and(|d| d.info.flags_1 & DF_1_PIE != 0);
            if has_interpreter || pie_flag {
                ProgramKind::Executable
            } else {
                ProgramKind::SharedObject
            }
        }
        header::ET_CORE => ProgramKind::Core,
        header::ET_REL => ProgramKind::Relocatable,
        _ => ProgramKind::Other,
    }
}

fn identify_abi(elf: &Elf<'_>, data: &[u8], interpreter: Option<&str>) -> (Abi, Vec<String>) {
    let mut evidence = Vec::new();
    let mut linux_kernel = None;
    let mut linux = false;
    let mut android = false;
    let mut freebsd = None;
    let mut other_bsd = None;

    for note in elf.iter_note_headers(data).into_iter().flatten().flatten() {
        let word = |index: usize| -> Option<u32> {
            let bytes: [u8; 4] = note.desc.get(index * 4..index * 4 + 4)?.try_into().ok()?;
            Some(if elf.little_endian {
                u32::from_le_bytes(bytes)
            } else {
                u32::from_be_bytes(bytes)
            })
        };
        // Type 1 is the ABI tag for all of these owners.
        match (note.name.trim_end_matches('\0'), note.n_type) {
            ("GNU", 1) if word(0) == Some(0) => {
                linux = true;
                linux_kernel = word(1)
                    .zip(word(2))
                    .zip(word(3))
                    .map(|((a, b), c)| (a, b, c));
                evidence.push("GNU ABI tag note for Linux".to_owned());
            }
            ("Android", 1) => {
                android = true;
                evidence.push("Android note".to_owned());
            }
            ("FreeBSD", 1) => {
                freebsd = Some(word(0));
                evidence.push("FreeBSD ABI tag note".to_owned());
            }
            (name @ ("OpenBSD" | "NetBSD" | "DragonFly"), 1) => {
                other_bsd = Some(name.to_owned());
                evidence.push(format!("{name} note"));
            }
            _ => {}
        }
    }

    match elf.header.e_ident[header::EI_OSABI] {
        header::ELFOSABI_FREEBSD => {
            freebsd = freebsd.or(Some(None));
            evidence.push("FreeBSD OS/ABI byte".to_owned());
        }
        header::ELFOSABI_LINUX => {
            linux = true;
            evidence.push("Linux OS/ABI byte".to_owned());
        }
        _ => {}
    }

    let mut libc = if has_segment(elf, program_header::PT_INTERP) {
        Libc::Unknown
    } else {
        Libc::None
    };
    if let Some(path) = interpreter {
        let file = path.rsplit('/').next().unwrap_or(path);
        if file.starts_with("ld-musl-") {
            (linux, libc) = (true, Libc::Musl);
            evidence.push(format!("musl loader {path}"));
        } else if path.starts_with("/system/bin/linker") {
            android = true;
            evidence.push(format!("Android loader {path}"));
        } else if file.starts_with("ld-linux") || path.starts_with("/lib64/ld64.so") {
            (linux, libc) = (true, Libc::Glibc);
            evidence.push(format!("glibc loader {path}"));
        } else if file.starts_with("ld-elf") {
            freebsd = freebsd.or(Some(None));
            evidence.push(format!("FreeBSD loader {path}"));
        }
    }
    if android {
        (linux, libc) = (true, Libc::Bionic);
    } else if libc == Libc::None
        && elf
            .libraries
            .iter()
            .any(|name| name.starts_with("libc.so.6"))
    {
        // Shared objects have no loader of their own.
        (linux, libc) = (true, Libc::Glibc);
        evidence.push("depends on libc.so.6".to_owned());
    }

    // An explicit FreeBSD mark wins: FreeBSD's own binaries never carry Linux
    // marks, so a conflict means the file is unusual and is treated as native.
    let abi = if let Some(osreldate) = freebsd {
        Abi::FreeBsd { osreldate }
    } else if let Some(name) = other_bsd {
        Abi::OtherBsd { name }
    } else if linux {
        Abi::Linux {
            libc,
            min_kernel: linux_kernel,
        }
    } else {
        Abi::Unknown
    };
    (abi, evidence)
}

fn has_segment(elf: &Elf<'_>, kind: u32) -> bool {
    elf.program_headers
        .iter()
        .any(|header| header.p_type == kind)
}
