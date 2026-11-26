//! Shared by `meta-linux` and `meta-exec`.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{Args, ValueEnum};
use meta_analyzer::{Known, Observations, Profile, analyze};
use meta_common::SystemRunner;
use meta_linux_vm::Manager;
use meta_maes::decide::Preference;
use meta_maes::launch::{self, EXIT_LAUNCHER, EXIT_NOT_FOUND, EXIT_NOT_RUNNABLE, Settings};
use meta_maes::{Decision, Host, Verdict, decide};
use meta_model::{Registry, RuntimeKind};
use serde::Serialize;

const TRUSS: &str = "/usr/bin/truss";

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum RuntimeArgument {
    Native,
    Linuxulator,
    Userspace,
    #[value(alias = "vm")]
    VirtualMachine,
}

impl From<RuntimeArgument> for RuntimeKind {
    fn from(argument: RuntimeArgument) -> Self {
        match argument {
            RuntimeArgument::Native => RuntimeKind::Native,
            RuntimeArgument::Linuxulator => RuntimeKind::Linuxulator,
            RuntimeArgument::Userspace => RuntimeKind::Userspace,
            RuntimeArgument::VirtualMachine => RuntimeKind::VirtualMachine,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct Selection {
    /// Use this kind of runtime or none
    #[arg(long, value_enum)]
    pub runtime: Option<RuntimeArgument>,
    /// Use this edition or machine or none
    #[arg(long, value_name = "NAME")]
    pub instance: Option<String>,
    /// Treat hints as requirements
    #[arg(long)]
    pub strict: bool,
}

impl Selection {
    fn preference(&self) -> Preference {
        Preference {
            runtime: self.runtime.map(Into::into),
            instance: self.instance.clone(),
            strict: self.strict,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct RunArguments {
    #[command(flatten)]
    pub selection: Selection,
    /// Show what would be done, and do nothing
    #[arg(long)]
    pub dry_run: bool,
    /// Let a program in a machine use this display. It can then see and control
    /// everything on it
    #[arg(long)]
    pub x11: bool,
    /// Pass a variable to a program in a machine
    #[arg(long, value_name = "NAME[=VALUE]")]
    pub env: Vec<String>,
    /// Program to run
    pub program: PathBuf,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub arguments: Vec<String>,
}

#[cfg(unix)]
pub fn is_root() -> bool {
    rustix::process::geteuid().is_root()
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    false
}

/// Who is asking. Under sudo, the person rather than root.
pub fn user() -> String {
    let variable = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    if is_root() {
        return variable("SUDO_USER").unwrap_or_else(|| "root".into());
    }
    // Not from the environment, which the caller controls.
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    real_uid()
        .and_then(|uid| user_name(&passwd, uid))
        .unwrap_or_default()
}

#[cfg(unix)]
fn real_uid() -> Option<u32> {
    Some(rustix::process::getuid().as_raw())
}

#[cfg(not(unix))]
fn real_uid() -> Option<u32> {
    None
}

/// Name of a user number in passwd(5) format.
fn user_name(passwd: &str, uid: u32) -> Option<String> {
    passwd.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        let matches = fields.get(2).and_then(|f| f.parse::<u32>().ok()) == Some(uid);
        (matches && !line.starts_with('#')).then(|| fields[0].to_owned())
    })
}

fn observations() -> Observations {
    let state = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| Path::new(&home).join(".local/state")));
    state.map_or_else(
        |_| Observations::default(),
        |path| Observations::load(&path.join("meta-exec/observations.toml")),
    )
}

pub fn profile(program: &Path) -> Result<Profile, String> {
    analyze(program, &Known::builtin(), &observations())
}

#[derive(Debug, Serialize)]
pub struct Diagnosis {
    pub profile: Profile,
    pub decision: Decision,
}

pub fn diagnose(program: &Path, selection: &Selection) -> Result<Diagnosis, String> {
    let runner = SystemRunner;
    let manager = Manager::new(&runner);
    let profile = profile(program)?;
    let mut host = Host::detect(&runner, &manager, &user());
    let (registry, preference) = (Registry::builtin(), selection.preference());
    let mut decision = decide(&profile, &registry, host.candidates(&profile), &preference);
    let local = |kind| kind != RuntimeKind::VirtualMachine;
    if !decision.choice().is_some_and(|a| local(a.candidate.kind)) {
        // Only now do the guests matter.
        host.probe(&manager);
        decision = decide(&profile, &registry, host.candidates(&profile), &preference);
    }
    Ok(Diagnosis { profile, decision })
}

pub fn print_json(value: &impl Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

pub fn print_profile(profile: &Profile) {
    println!("program  {}", profile.program.display());
    println!("kind     {:?}", profile.kind);
    if let Some(line) = &profile.interpreter {
        println!("run by   {line}");
    }
    for need in &profile.needs {
        let strength = if need.is_firm() { "needs" } else { "hint" };
        println!("{strength:8} {}", need.requirement);
        for evidence in &need.evidence {
            println!("         {:?}: {evidence}", evidence.source);
        }
    }
    for limitation in &profile.limitations {
        println!("limit    {limitation}");
    }
}

pub fn print_decision(decision: &Decision) {
    for line in &decision.explanation {
        println!("{line}");
    }
    println!();
    for a in &decision.assessed {
        let verdict = match a.verdict {
            Verdict::Suitable => "suitable",
            Verdict::Uncertain => "uncertain",
            Verdict::Unsuitable => "unsuitable",
        };
        println!(
            "{:32} {verdict:10} {}",
            a.candidate.label(),
            a.reasons.join("; ")
        );
    }
}

fn environment(entries: &[String]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|entry| match entry.split_once('=') {
            Some((name, value)) => Some((name.to_owned(), value.to_owned())),
            None => std::env::var(entry)
                .ok()
                .map(|value| (entry.clone(), value)),
        })
        .collect()
}

/// Runs a program and returns its exit code, or one of the launcher's own.
pub fn run(tool: &str, arguments: &RunArguments) -> ExitCode {
    let fail = |code: u8, message: &str| {
        eprintln!("{tool}: {message}");
        ExitCode::from(code)
    };
    let Some(program) = find(&arguments.program) else {
        let message = format!("{}: not found", arguments.program.display());
        return fail(EXIT_NOT_FOUND, &message);
    };
    let diagnosis = match diagnose(&program, &arguments.selection) {
        Ok(diagnosis) => diagnosis,
        Err(error) => return fail(EXIT_NOT_RUNNABLE, &error),
    };
    let Some(choice) = diagnosis.decision.choice() else {
        return fail(
            EXIT_NOT_RUNNABLE,
            &diagnosis.decision.explanation.join("\n"),
        );
    };
    let runner = SystemRunner;
    let manager = Manager::new(&runner);
    let directory = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let action = match launch::plan(
        choice,
        &program,
        &arguments.arguments,
        &directory,
        &manager,
        &runner,
    ) {
        Ok(action) => action,
        Err(error) => return fail(EXIT_NOT_RUNNABLE, &error),
    };
    if arguments.dry_run {
        print_decision(&diagnosis.decision);
        println!();
        for line in action.describe() {
            println!("would {line}");
        }
        return ExitCode::SUCCESS;
    }
    let settings = Settings {
        x11: arguments.x11,
        terminal: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        environment: environment(&arguments.env),
    };
    match launch::run(&action, &settings, &manager) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(EXIT_LAUNCHER)),
        Err(error) => fail(EXIT_LAUNCHER, &error),
    }
}

/// Resolves a program the way a shell would.
fn find(program: &Path) -> Option<PathBuf> {
    if program.components().count() > 1 || program.is_absolute() {
        return program.exists().then(|| program.to_owned());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(program))
            .find(|candidate| candidate.is_file())
    })
}

/// Runs a Linux program under truss and remembers which calls did not exist.
pub fn trial(program: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    if is_root() {
        return Err("programs are not tried out as root; run this as an ordinary user".into());
    }
    let program = std::path::absolute(program).map_err(|e| e.to_string())?;
    let trace = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    let status = Command::new(TRUSS)
        .args(["-f", "-o"])
        .arg(trace.path())
        .arg(&program)
        .args(arguments)
        .status()
        .map_err(|e| format!("{TRUSS}: {e}"))?;
    let text = std::fs::read(trace.path()).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&text);
    let missing = meta_analyzer::observe::missing_calls(&text);
    // truss exits with 0 whatever the program did.
    let failed = !status.success() || meta_analyzer::observe::exit_status(&text) != Some(0);
    observations().record(&program, &missing, failed)?;
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_are_taken_from_the_argument_or_the_environment() {
        let all = environment(&["A=b=c".into(), "META_NOT_SET".into(), "E=".into()]);
        assert_eq!(
            all,
            [("A".into(), "b=c".into()), ("E".into(), String::new())]
        );
    }

    #[test]
    fn users_are_found_by_number() {
        let passwd = "# comment:x:5
root:*:0:0::/root:/bin/sh
alice:*:1001:1001::/home/alice:/bin/sh
";
        assert_eq!(user_name(passwd, 1001).as_deref(), Some("alice"));
        assert_eq!(user_name(passwd, 0).as_deref(), Some("root"));
        assert_eq!(user_name(passwd, 7), None);
    }

    #[test]
    fn missing_programs_are_not_found() {
        assert_eq!(find(Path::new("/nonexistent/x")), None);
        assert_eq!(find(Path::new("meta-no-such-program")), None);
    }
}
