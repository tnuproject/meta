//! Finds out what a program needs.
//!
//! Four sources are combined, and each need keeps the evidence behind it:
//!
//! * the file itself: architecture, interface, libraries, versions ([`elf`]);
//! * traces of system calls and paths in it, which only hint ([`hints`]);
//! * a list of known programs, and requirements shipped with a program ([`known`]);
//! * failures seen when the program ran ([`observe`]).
//!
//! Finding nothing does not show that nothing is needed.

pub mod elf;
pub mod hints;
pub mod known;
pub mod observe;

use std::io::Read;
use std::path::{Path, PathBuf};

use linux2bsd::elf::{Abi, Binary, ElfError, ProgramKind};
use meta_model::{Evidence, Need, Source};
use serde::Serialize;

pub use known::Known;
pub use observe::Observations;

/// Larger files are identified, but not searched for hints.
const MAX_SCAN: u64 = 512 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    FreeBsd,
    Linux,
    Script,
    /// An executable for something else, or one that says nothing about it.
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub program: PathBuf,
    pub kind: Kind,
    /// Interpreter line of a script.
    pub interpreter: Option<String>,
    pub binary: Option<Binary>,
    pub needs: Vec<Need>,
    /// What this analysis could not see.
    pub limitations: Vec<String>,
}

/// Identifies a program and collects what it needs.
pub fn analyze(
    program: &Path,
    known: &Known,
    observations: &Observations,
) -> Result<Profile, String> {
    let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", program.display());
    let mut profile = Profile {
        program: program.to_owned(),
        kind: Kind::Other,
        interpreter: None,
        binary: None,
        needs: Vec::new(),
        limitations: Vec::new(),
    };
    match linux2bsd::inspect(program) {
        Ok(binary) => {
            profile.kind = match (&binary.abi, binary.kind) {
                (Abi::FreeBsd { .. }, ProgramKind::Executable) => Kind::FreeBsd,
                (Abi::Linux { .. }, ProgramKind::Executable) => Kind::Linux,
                _ => Kind::Other,
            };
            profile.needs.extend(elf::needs(&binary));
            if profile.kind == Kind::Linux {
                scan(program, &binary, &mut profile);
            }
            profile.binary = Some(binary);
        }
        Err(ElfError::Script(line)) => {
            profile.kind = Kind::Script;
            profile.interpreter = Some(line);
            profile.limitations.push(
                "a script needs whatever the commands it runs need, which is not analysed".into(),
            );
        }
        Err(error) => return Err(fail(&error)),
    }
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if matches!(profile.kind, Kind::Linux | Kind::Script) {
        for (requirement, evidence) in known.needs(&name, profile.kind == Kind::Script) {
            Need::add(&mut profile.needs, requirement, evidence);
        }
    }
    for (requirement, evidence) in known::declared(program)? {
        Need::add(&mut profile.needs, requirement, evidence);
    }
    for (requirement, evidence) in observations.needs(program) {
        Need::add(&mut profile.needs, requirement, evidence);
    }
    Ok(profile)
}

fn scan(program: &Path, binary: &Binary, profile: &mut Profile) {
    let data = std::fs::File::open(program).and_then(|file| {
        let mut data = Vec::new();
        file.take(MAX_SCAN + 1).read_to_end(&mut data)?;
        Ok(data)
    });
    match data {
        Ok(data) if data.len() as u64 <= MAX_SCAN => {
            for (requirement, evidence) in hints::find(&data, binary) {
                Need::add(&mut profile.needs, requirement, evidence);
            }
        }
        Ok(_) => profile
            .limitations
            .push("too large to be searched for hints".into()),
        Err(error) => profile
            .limitations
            .push(format!("not searched for hints: {error}")),
    }
    profile
        .limitations
        .push("plugins, helper programs and libraries loaded at run time are not analysed".into());
}

pub(crate) fn evidence(source: Source, detail: impl Into<String>) -> Evidence {
    Evidence::new(source, detail)
}
