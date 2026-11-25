//! `meta compatibility scan`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use linux2bsd::decision::HostState;
use linux2bsd::{Abi, Database, Decision, Linuxulator, ProgramKind, analyze, discover, inspect};
use meta_common::{CommandRunner, Platform};

const MAX_DEPTH: usize = 16;

pub fn scan(runner: &dyn CommandRunner, paths: &[PathBuf], all: bool) -> Result<ExitCode, String> {
    let platform = Platform::detect(runner);
    let linuxulator = Linuxulator::detect(runner);
    let runtimes = discover(Path::new("/"), &linuxulator.emul_path);
    let host = HostState {
        machine_arch: &platform.arch,
        linuxulator: &linuxulator,
        runtimes: &runtimes,
    };
    if !platform.is_freebsd() {
        eprintln!(
            "note: this host runs {}; programs are identified, but nothing here can run them \
             through Linuxulator",
            platform.os_name
        );
    }
    let database = Database::builtin();
    let mut files = Vec::new();
    for path in paths {
        // A path the user named must exist; unreadable subdirectories are skipped.
        fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        collect(path, 0, &mut files);
    }
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let (mut linux, mut runnable) = (0, 0);
    for file in &files {
        let Ok(binary) = inspect(file) else { continue };
        let is_program = binary.kind == ProgramKind::Executable;
        if !is_program || !matches!(binary.abi, Abi::Linux { .. }) {
            continue;
        }
        linux += 1;
        let report = analyze(file, &binary, &host, &database);
        if report.decision == Decision::NotRunnable {
            println!("{}", file.display());
            for problem in &report.problems {
                println!("    {problem}");
                *problems.entry(problem.clone()).or_default() += 1;
            }
        } else {
            runnable += 1;
            if all {
                println!("{}\n    no known problem", file.display());
            }
        }
    }
    println!(
        "{} files examined, {linux} Linux programs, {runnable} without known problems",
        files.len()
    );
    let mut shared: Vec<_> = problems
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect();
    shared.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    if !shared.is_empty() {
        println!("Most common problems:");
        for (problem, count) in shared.iter().take(5) {
            println!("    {count:5}  {problem}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Collects regular files without following symbolic links.
fn collect(path: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_file() {
        files.push(path.to_owned());
    } else if metadata.is_dir() && depth < MAX_DEPTH {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
        children.sort();
        for child in children {
            collect(&child, depth + 1, files);
        }
    }
}
