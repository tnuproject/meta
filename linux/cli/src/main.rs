//! `meta-linux`: the Linux compatibility environment.

mod containers;
mod machines;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_analyzer::Kind;
use meta_common::SystemRunner;
use meta_linux::{
    RunArguments, Selection, diagnose, is_root, print_decision, print_json, print_profile, profile,
    run as run_program, user,
};
use meta_linux_vm::{Images, Manager, VmConfig, VmError};
use meta_maes::launch::EXIT_NOT_RUNNABLE;
use meta_maes::{Host, Verdict};
use meta_model::RuntimeKind;

const EXIT_NO_SUCH: u8 = 66;
const EXIT_UNAVAILABLE: u8 = 69;
const EXIT_NOT_READY: u8 = 75;
const EXIT_PERMISSION: u8 = 77;

#[derive(Parser)]
#[command(
    name = "meta-linux",
    version,
    about = "Linux compatibility environment"
)]
struct Cli {
    /// Machine-readable output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Summarise what can run Linux programs here
    Status,
    /// Find Linux programs below a directory and where they would run
    Scan {
        #[arg(default_value = ".")]
        directory: PathBuf,
    },
    /// List the kinds of runtime and whether each can be used
    Runtimes,
    /// List userlands and machines
    Environments,
    /// Guest images
    #[command(subcommand)]
    Image(ImageCommand),
    /// Linux virtual machines
    #[command(subcommand)]
    Vm(machines::Command),
    /// Linux programs
    #[command(subcommand)]
    App(AppCommand),
    /// Linux containers inside machines
    #[command(subcommand)]
    Container(containers::Command),
    /// Run a program; short for `app run`
    Run(RunArguments),
}

#[derive(Subcommand)]
enum ImageCommand {
    /// List the images machines can be made from
    List,
}

#[derive(Subcommand)]
enum AppCommand {
    /// Show what a program needs, and where it would run
    Diagnose {
        #[command(flatten)]
        selection: Selection,
        program: PathBuf,
    },
    /// Run a program; its exit code is passed on
    Run(RunArguments),
}

/// Errors with an exit code of their own.
struct Failure(u8, String);

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self(1, message)
    }
}

impl From<VmError> for Failure {
    fn from(error: VmError) -> Self {
        let code = match &error {
            VmError::Missing(_) => EXIT_NO_SUCH,
            VmError::Unsupported(_) => EXIT_UNAVAILABLE,
            VmError::NotReady(..) | VmError::NotRunning(_) => EXIT_NOT_READY,
            _ => 1,
        };
        Self(code, error.to_string())
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runner = SystemRunner;
    let manager = Manager::new(&runner);
    let result = match cli.command {
        Command::Status => status(&manager, cli.json),
        Command::Scan { directory } => scan(&directory, cli.json),
        Command::Runtimes => runtimes(&manager, cli.json),
        Command::Environments => environments(&manager, cli.json),
        Command::Image(ImageCommand::List) => images(cli.json),
        Command::Vm(command) => machines::run(&manager, command, cli.json),
        Command::App(AppCommand::Diagnose { selection, program }) => {
            app(&program, &selection, cli.json)
        }
        Command::App(AppCommand::Run(arguments)) | Command::Run(arguments) => {
            return run_program("meta-linux", &arguments);
        }
        Command::Container(command) => containers::run(&manager, &command)
            .map(exit_code)
            .map_err(Into::into),
    };
    result.unwrap_or_else(|Failure(code, message)| {
        eprintln!("meta-linux: {message}");
        ExitCode::from(code)
    })
}

fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(125))
}

/// Machines are root's business: they hold its devices and everyone's memory.
fn require_root() -> Result<(), Failure> {
    if is_root() {
        Ok(())
    } else {
        Err(Failure(
            EXIT_PERMISSION,
            "this changes machines and needs root; use sudo".into(),
        ))
    }
}

/// A machine the calling user may use.
fn user_machine(manager: &Manager<'_>, name: Option<&str>) -> Result<VmConfig, String> {
    let user = user();
    let allowed = |config: &VmConfig| config.owner == user || user == "root";
    if let Some(name) = name {
        let config = manager.load(name).map_err(|e| e.to_string())?;
        return if allowed(&config) {
            Ok(config)
        } else {
            Err(format!("{name} belongs to {}", config.owner))
        };
    }
    let mut mine: Vec<VmConfig> = manager.list().into_iter().filter(allowed).collect();
    match (mine.pop(), mine.is_empty()) {
        (Some(config), true) => Ok(config),
        (None, _) => Err("you have no machine; see: meta-linux vm create --help".into()),
        (Some(_), false) => Err("you have several machines; choose one with --vm".into()),
    }
}

fn status(manager: &Manager<'_>, json: bool) -> Result<ExitCode, Failure> {
    let host = Host::detect(&SystemRunner, manager, &user());
    let support = manager.support();
    if json {
        print_json(&serde_json::json!({
            "architecture": host.arch,
            "linuxulator": host.linuxulator,
            "emulation": host.emulation,
            "userlands": host.userlands,
            "machines": host.guests,
            "bhyve": support,
        }))?;
        return Ok(ExitCode::SUCCESS);
    }
    match &host.linuxulator.osrelease {
        Some(release) => println!("Linuxulator  loaded, reports Linux {release}"),
        None => println!("Linuxulator  not loaded"),
    }
    println!("userlands    {}", host.userlands.len());
    let running = host.guests.iter().filter(|g| g.running).count();
    println!("machines     {}, {running} running", host.guests.len());
    let missing: Vec<&str> = support
        .iter()
        .filter(|f| !f.ok)
        .map(|f| f.detail.as_str())
        .collect();
    if missing.is_empty() {
        println!("bhyve        usable");
    } else {
        println!("bhyve        not usable: {}", missing.join("; "));
    }
    Ok(ExitCode::SUCCESS)
}

/// Executables below a directory, without following links or crossing a limit.
fn executables(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![(directory.to_owned(), 0)];
    while let Some((path, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() && depth < 8 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() && found.len() < 1000 && executable(&entry) {
                found.push(entry.path());
            }
        }
    }
    found
}

#[cfg(unix)]
fn executable(entry: &std::fs::DirEntry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry
        .metadata()
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_entry: &std::fs::DirEntry) -> bool {
    true
}

fn scan(directory: &Path, json: bool) -> Result<ExitCode, Failure> {
    let selection = Selection {
        runtime: None,
        instance: None,
        strict: false,
    };
    let mut all = Vec::new();
    for path in executables(directory) {
        if !profile(&path).is_ok_and(|p| p.kind == Kind::Linux) {
            continue;
        }
        let diagnosis = diagnose(&path, &selection)?;
        let choice = diagnosis
            .decision
            .choice()
            .map_or_else(|| "none".to_owned(), |a| a.candidate.label());
        if json {
            all.push(serde_json::json!({ "program": path, "runtime": choice }));
        } else {
            println!("{choice:32} {}", path.display());
        }
    }
    if json {
        print_json(&all)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn runtimes(manager: &Manager<'_>, json: bool) -> Result<ExitCode, Failure> {
    let host = Host::detect(&SystemRunner, manager, &user());
    let support = manager.support();
    let bhyve = support.iter().find(|f| !f.ok);
    let rows = RuntimeKind::ALL.map(|kind| match kind {
        RuntimeKind::Native => (kind, true, "FreeBSD programs".to_owned()),
        RuntimeKind::Linuxulator => {
            let loaded = host.linuxulator.loaded;
            let detail = match &host.emulation {
                Some(runtime) if loaded => format!("with {}", runtime.root.display()),
                None if loaded => "static programs only; no userland".to_owned(),
                _ => "not loaded".to_owned(),
            };
            (kind, loaded, detail)
        }
        RuntimeKind::Userspace => {
            let names: Vec<&str> = host.userlands.iter().map(|u| u.edition.as_str()).collect();
            (kind, !names.is_empty(), names.join(", "))
        }
        RuntimeKind::VirtualMachine => {
            let detail = bhyve.map_or_else(
                || format!("{} machines", host.guests.len()),
                |finding| finding.detail.clone(),
            );
            (kind, bhyve.is_none() && !host.guests.is_empty(), detail)
        }
    });
    if json {
        let rows: Vec<_> = rows
            .iter()
            .map(|(kind, usable, detail)| {
                serde_json::json!({ "runtime": kind, "usable": usable, "detail": detail })
            })
            .collect();
        print_json(&rows)?;
    } else {
        for (kind, usable, detail) in rows {
            println!(
                "{kind:16} {:12} {detail}",
                if usable { "usable" } else { "not usable" }
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn environments(manager: &Manager<'_>, json: bool) -> Result<ExitCode, Failure> {
    let mut host = Host::detect(&SystemRunner, manager, &user());
    host.probe(manager);
    if json {
        print_json(&serde_json::json!({
            "userlands": host.userlands,
            "machines": host.guests,
        }))?;
        return Ok(ExitCode::SUCCESS);
    }
    let state = |running| if running { "running" } else { "stopped" };
    for userland in &host.userlands {
        println!(
            "userspace        {:16} {:8} {}",
            userland.edition,
            state(userland.running),
            userland.path
        );
    }
    for guest in &host.guests {
        println!(
            "virtual-machine  {:16} {:8} {} {}",
            guest.config.name,
            state(guest.running),
            guest.config.image,
            guest.kernel.as_deref().unwrap_or_default()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn images(json: bool) -> Result<ExitCode, Failure> {
    let images = Images::builtin();
    if json {
        print_json(&images.all())?;
    } else {
        for image in images.all() {
            println!("{:12} {}", image.id, image.name);
            println!("             {}", image.url);
            println!("             {}", image.digest);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn app(program: &Path, selection: &Selection, json: bool) -> Result<ExitCode, Failure> {
    let diagnosis = diagnose(program, selection)?;
    if json {
        print_json(&diagnosis)?;
    } else {
        print_profile(&diagnosis.profile);
        println!();
        print_decision(&diagnosis.decision);
    }
    let suitable = diagnosis
        .decision
        .choice()
        .is_some_and(|a| a.verdict != Verdict::Unsuitable);
    Ok(if suitable {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOT_RUNNABLE)
    })
}
