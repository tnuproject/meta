//! Carries out a plan made by meta-installer. Erases the chosen disk.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::Parser;
use meta_common::{Platform, SystemRunner};
use meta_installer::config::{InstallConfig, Password};
use meta_installer::execute::{Options, install};
use meta_installer::plan::{FORMAT, Plan};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "meta-install-run",
    version,
    about = "Carry out a META installation plan"
)]
struct Cli {
    /// Plan saved by meta-installer
    #[arg(long, value_name = "FILE")]
    plan: PathBuf,
    /// pkg repository configuration to install from
    #[arg(
        long,
        value_name = "DIR",
        default_value = "/usr/local/share/meta-installer/repos"
    )]
    repositories: PathBuf,
    /// Trust anchor and package keys to install
    #[arg(long, value_name = "DIR")]
    trust: Option<PathBuf>,
    /// Name of the new ZFS pool
    #[arg(long, default_value = "zroot")]
    pool: String,
    /// Where to mount the new system meanwhile
    #[arg(long, default_value = "/mnt")]
    target: PathBuf,
    /// Use the first serial port as a console too
    #[arg(long)]
    serial_console: bool,
    /// Read the user's password from the first line of standard input
    #[arg(long)]
    password_stdin: bool,
    /// Name of the disk to erase; asked for when absent
    #[arg(long, value_name = "DISK")]
    confirm: Option<String>,
}

/// The parts of a plan that matter here. Steps and warnings are for people; the
/// configuration is validated again.
#[derive(Deserialize)]
struct SavedPlan {
    format: String,
    configuration: InstallConfig,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("meta-install-run: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let runner = SystemRunner;
    Platform::detect(&runner).require_freebsd("installing META")?;
    let text =
        std::fs::read_to_string(&cli.plan).map_err(|e| format!("{}: {e}", cli.plan.display()))?;
    let saved: SavedPlan =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", cli.plan.display()))?;
    if saved.format != FORMAT {
        return Err(format!("unsupported plan format {:?}", saved.format));
    }
    let config = saved.configuration;
    let plan = Plan::build(&config).map_err(|problems| problems.join("; "))?;
    let password = if cli.password_stdin {
        let mut line = Zeroizing::new(String::new());
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Password::from_text(line.trim_end_matches(['\r', '\n']))
    } else {
        ask_password(&config.user.name)?
    };
    let confirmation = match cli.confirm {
        Some(confirmation) => confirmation,
        None => ask_confirmation(&plan, config.disk.as_deref().unwrap_or_default())?,
    };
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let options = Options {
        repositories: cli.repositories,
        pool: cli.pool,
        target: cli.target,
        confirmation,
        serial_console: cli.serial_console,
        label: format!("{seconds:x}"),
        trust: cli.trust,
    };
    install(&config, &password, &options, &runner, &mut |step| {
        println!("==> {step}")
    })
    .map_err(|e| e.to_string())?;
    println!(
        "META {} is installed on {}.",
        meta_common::VERSION,
        options.confirmation
    );
    Ok(())
}

fn terminal() -> Result<std::fs::File, String> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|e| format!("no terminal to ask on: {e}"))
}

fn ask(prompt: &str) -> Result<Zeroizing<String>, String> {
    let mut tty = terminal()?;
    tty.write_all(prompt.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut line = Zeroizing::new(String::new());
    std::io::BufReader::new(tty)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(Zeroizing::new(
        line.trim_end_matches(['\r', '\n']).to_owned(),
    ))
}

fn ask_confirmation(plan: &Plan, disk: &str) -> Result<String, String> {
    println!();
    for step in plan.steps.iter().filter(|step| step.destructive) {
        println!("  {}", step.description);
    }
    // What is on the disk now, for a last look.
    let _ = Command::new("/sbin/gpart")
        .args(["show", "-p", disk])
        .status();
    println!("\nEverything on {disk} will be lost.");
    ask("Type the name of the disk to erase it, or anything else to stop: ")
        .map(|answer| answer.to_string())
}

fn ask_password(user: &str) -> Result<Password, String> {
    let stty = |mode: &str| {
        let tty = terminal()?;
        Command::new("/bin/stty")
            .arg(mode)
            .stdin(tty)
            .status()
            .map_err(|e| e.to_string())
    };
    stty("-echo")?;
    let answer = ask(&format!("Password for {user}: "));
    stty("echo")?;
    println!();
    Ok(Password::from_text(&answer?))
}
