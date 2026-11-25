use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use linux2bsd::decision::HostState;
use linux2bsd::{
    Database, Decision, Distribution, Linuxulator, Report, analyze, discover, inspect,
};
use meta_common::{Platform, SystemRunner};

#[derive(Parser)]
#[command(
    name = "linux2bsd",
    version,
    about = "Linux program compatibility diagnostics"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Identify a program and explain how it would run
    Inspect {
        program: PathBuf,
        #[arg(long)]
        json: bool,
        /// Check libraries against this Linux userland instead of the host's
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
    },
    /// List Linux userlands and the state of Linuxulator
    Runtimes {
        #[arg(long)]
        json: bool,
    },
    /// Run a program after checking that it can run
    Run {
        program: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Inspect {
            program,
            json,
            root,
        } => inspect_command(&program, json, root),
        Command::Runtimes { json } => runtimes_command(json),
        Command::Run { program, arguments } => run_command(&program, &arguments),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("linux2bsd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn report_for(program: &Path, root: Option<PathBuf>) -> Result<Report, String> {
    let binary = inspect(program).map_err(|e| format!("{}: {e}", program.display()))?;
    let platform = Platform::detect(&SystemRunner);
    let linuxulator = Linuxulator::detect(&SystemRunner);
    let runtimes = match root {
        Some(root) => vec![linux2bsd::Runtime {
            root,
            is_emul_path: false,
            distribution: None,
        }],
        None => discover(Path::new("/"), &linuxulator.emul_path),
    };
    let host = HostState {
        machine_arch: &platform.arch,
        linuxulator: &linuxulator,
        runtimes: &runtimes,
    };
    Ok(analyze(program, &binary, &host, &Database::builtin()))
}

fn inspect_command(program: &Path, json: bool, root: Option<PathBuf>) -> Result<ExitCode, String> {
    let report = report_for(program, root)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        print_report(&report);
    }
    Ok(ExitCode::SUCCESS)
}

fn print_report(report: &Report) {
    let binary = &report.binary;
    println!("program      {}", report.program.display());
    println!(
        "format       {}-bit ELF, {:?}, {:?}",
        binary.bits, binary.arch, binary.kind
    );
    println!("abi          {:?}", binary.abi);
    for item in &binary.evidence {
        println!("             based on {item}");
    }
    if let Some(interpreter) = &binary.interpreter {
        println!("interpreter  {interpreter}");
    }
    if let Some(resolution) = &report.resolution {
        println!(
            "libraries    {} found, {} missing",
            resolution.found.len(),
            resolution.missing.len()
        );
    }
    let decision = match &report.decision {
        Decision::Native => "runs natively".to_owned(),
        Decision::Linuxulator { root } => format!("Linuxulator with {}", root.display()),
        Decision::OtherRoot { root } => {
            format!(
                "needs {} (running from it is not implemented)",
                root.display()
            )
        }
        Decision::NotRunnable => "cannot run".to_owned(),
    };
    println!("decision     {decision}");
    for problem in &report.problems {
        println!("problem      {problem}");
    }
    for note in &report.notes {
        println!("note         {note}");
    }
    for advice in &report.advice {
        println!("known issue  {} [{}]", advice.summary, advice.id);
    }
}

fn runtimes_command(json: bool) -> Result<ExitCode, String> {
    let linuxulator = Linuxulator::detect(&SystemRunner);
    let runtimes = discover(Path::new("/"), &linuxulator.emul_path);
    if json {
        let value = serde_json::json!({ "linuxulator": linuxulator, "runtimes": runtimes });
        println!("{value:#}");
        return Ok(ExitCode::SUCCESS);
    }
    match &linuxulator.osrelease {
        Some(release) => println!("Linuxulator  loaded, reports Linux {release}"),
        None => println!("Linuxulator  not loaded"),
    }
    println!("emulation    {}", linuxulator.emul_path.display());
    if runtimes.is_empty() {
        println!("no Linux userland found");
    }
    for runtime in runtimes {
        let name = runtime
            .distribution
            .map_or("unknown".to_owned(), |d: Distribution| d.name);
        println!("userland     {} ({name})", runtime.root.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn run_command(program: &Path, arguments: &[String]) -> Result<ExitCode, String> {
    Platform::detect(&SystemRunner).require_freebsd("running programs through linux2BSD")?;
    let report = report_for(program, None)?;
    match &report.decision {
        Decision::Native | Decision::Linuxulator { .. } => execute(program, arguments),
        Decision::OtherRoot { root } => Err(format!(
            "this program needs {}; running programs from other roots is not implemented",
            root.display()
        )),
        Decision::NotRunnable => {
            print_report(&report);
            Err("not started".to_owned())
        }
    }
}

/// Replaces this process. The kernel selects the Linux ABI by itself.
#[cfg(unix)]
fn execute(program: &Path, arguments: &[String]) -> Result<ExitCode, String> {
    use std::os::unix::process::CommandExt;
    let program = std::path::absolute(program).map_err(|e| e.to_string())?;
    let error = std::process::Command::new(&program).args(arguments).exec();
    Err(format!("{}: {error}", program.display()))
}

#[cfg(not(unix))]
fn execute(_program: &Path, _arguments: &[String]) -> Result<ExitCode, String> {
    Err("not supported on this operating system".to_owned())
}
