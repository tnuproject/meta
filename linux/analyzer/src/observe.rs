//! What was seen when a program ran.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use meta_model::{Evidence, Requirement, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// System calls that failed because they do not exist, from truss(1) output.
pub fn missing_calls(trace: &str) -> Vec<String> {
    let mut calls: Vec<String> = Vec::new();
    for line in trace.lines() {
        // "1234: linux_unshare(0x10000000)	 ERR#-38 'Function not implemented'"
        // Only the result, not a text that the program happened to write.
        if !line.trim_end().ends_with("'Function not implemented'") {
            continue;
        }
        let Some((before, _)) = line.split_once('(') else {
            continue;
        };
        let name = before.rsplit([':', ' ', '\t']).next().unwrap_or_default();
        let name = name.strip_prefix("linux_").unwrap_or(name);
        let valid = !name.is_empty()
            && name.len() < 40
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if valid && !calls.iter().any(|known| known == name) {
            calls.push(name.to_owned());
        }
    }
    calls
}

/// Exit status of the first process in truss(1) output; truss itself exits with 0.
pub fn exit_status(trace: &str) -> Option<i32> {
    let mut first: Option<&str> = None;
    for line in trace.lines() {
        // With -f, lines start with the process number.
        let (pid, rest) = match line.trim_start().split_once(": ") {
            Some((pid, rest)) if pid.bytes().all(|b| b.is_ascii_digit()) => (pid, rest),
            _ => ("", line.trim_start()),
        };
        let first = *first.get_or_insert(pid);
        if pid == first
            && let Some(value) = rest.strip_prefix("process exit, rval = ")
        {
            return value.trim().parse().ok();
        }
    }
    None
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    program: String,
    missing: Vec<String>,
    /// Whether the program also ended with an error.
    failed: bool,
}

/// Failures of earlier runs, kept per program content.
#[derive(Debug, Clone, Default)]
pub struct Observations {
    path: Option<PathBuf>,
    records: BTreeMap<String, Record>,
}

impl Observations {
    /// Reads the calling user's record. An unreadable one counts as empty.
    pub fn load(path: &Path) -> Self {
        let records = fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path: Some(path.to_owned()),
            records,
        }
    }

    pub fn needs(&self, program: &Path) -> Vec<(Requirement, Evidence)> {
        if self.records.is_empty() {
            return Vec::new();
        }
        let Some(record) = digest(program).and_then(|d| self.records.get(&d).cloned()) else {
            return Vec::new();
        };
        record
            .missing
            .into_iter()
            .map(|call| {
                let detail = format!("{call} was not implemented when the program ran");
                // A program may try a call and continue without it. One that then
                // failed is taken to have needed it.
                let source = if record.failed {
                    Source::Observation
                } else {
                    Source::Heuristic
                };
                (Requirement::Syscall(call), Evidence::new(source, detail))
            })
            .collect()
    }

    /// Remembers the outcome of a run, if a place for it is known.
    pub fn record(
        &mut self,
        program: &Path,
        missing: &[String],
        failed: bool,
    ) -> Result<(), String> {
        let Some(digest) = digest(program) else {
            return Ok(());
        };
        let record = Record {
            program: program.display().to_string(),
            missing: missing.to_vec(),
            failed,
        };
        self.records.insert(digest, record);
        let Some(path) = &self.path else {
            return Ok(());
        };
        let fail = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| fail(&e))?;
        }
        let text = toml::to_string(&self.records).map_err(|e| fail(&e))?;
        let mut file = fs::File::create(path).map_err(|e| fail(&e))?;
        file.write_all(text.as_bytes()).map_err(|e| fail(&e))
    }
}

fn digest(program: &Path) -> Option<String> {
    let mut file = fs::File::open(program).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => return Some(hex::encode(hasher.finalize())),
            read => hasher.update(&buffer[..read]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_name_the_missing_calls() {
        let trace = "linux_unshare(0x10000000)\t ERR#-38 'Function not implemented'\n\
                     991: linux_setns(0x3,0x0)\t ERR#78 'Function not implemented'\n\
                     991: linux_setns(0x4,0x0)\t ERR#78 'Function not implemented'\n\
                     write(1,\"Function not implemented\\n\",25)\t = 25 (0x19)\n\
                     openat(AT_FDCWD,\"x\",0,0)\t ERR#2 'No such file or directory'\n";
        assert_eq!(missing_calls(trace), ["unshare", "setns"]);
        assert!(missing_calls("no calls here Function not implemented").is_empty());
    }

    #[test]
    fn the_first_process_decides_the_exit_status() {
        let trace = " 10: fork()	 = 11
 11: process exit, rval = 0
 10: process exit, rval = 38
";
        assert_eq!(exit_status(trace), Some(38));
        assert_eq!(
            exit_status(
                "write(1)
process exit, rval = 0
"
            ),
            Some(0)
        );
        assert_eq!(exit_status("killed"), None);
    }

    #[test]
    fn records_follow_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("p");
        let state = dir.path().join("state/observations.toml");
        fs::write(&program, "one").unwrap();
        let mut observations = Observations::load(&state);
        assert!(observations.needs(&program).is_empty());
        observations
            .record(&program, &["unshare".into()], false)
            .unwrap();
        let loaded = Observations::load(&state);
        assert_eq!(loaded.needs(&program)[0].0.to_string(), "syscall:unshare");
        assert_eq!(loaded.needs(&program)[0].1.source, Source::Heuristic);
        observations
            .record(&program, &["unshare".into()], true)
            .unwrap();
        assert_eq!(
            observations.needs(&program)[0].1.source,
            Source::Observation
        );
        fs::write(&program, "two").unwrap();
        assert!(loaded.needs(&program).is_empty());
        fs::write(&state, "not toml [").unwrap();
        assert!(Observations::load(&state).records.is_empty());
    }
}
