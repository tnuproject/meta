//! Manages Linux compatibility roots.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_common::{Platform, SystemRunner};
use meta_compat::{Root, State};

#[derive(Parser)]
#[command(
    name = "meta-compat",
    version,
    about = "Manage Linux compatibility roots"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List editions and their state
    List,
    /// Create a root from the distribution's signed archive
    Create { edition: String },
    /// Mount and start a root
    Start { edition: String },
    /// Stop a root and unmount everything in it
    Stop { edition: String },
    /// Run a command in a root
    Exec {
        edition: String,
        /// User to run as; defaults to the user who called sudo
        #[arg(long)]
        user: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Take a snapshot and update the distribution
    Update { edition: String },
    /// Take a snapshot
    Snapshot { edition: String },
    /// Return to a snapshot, or list snapshots
    Rollback {
        edition: String,
        snapshot: Option<String>,
        /// Confirm; everything after the snapshot is lost
        #[arg(long)]
        yes: bool,
    },
    /// Destroy a root
    Remove {
        edition: String,
        /// Confirm; the root cannot be recovered
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("meta-compat: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<ExitCode, String> {
    let runner = SystemRunner;
    let open = |id: &str| Root::open(id, &runner).map_err(|e| e.to_string());
    if !matches!(command, Command::List) {
        Platform::detect(&runner).require_freebsd("managing compatibility roots")?;
        require_root()?;
    }
    match command {
        Command::List => {
            for root in Root::all(&runner).map_err(|e| e.to_string())? {
                let state = match root.state() {
                    State::Absent => "not created",
                    State::Stopped => "stopped",
                    State::Running => "running",
                };
                println!("{:8} {:12} {}", root.edition.id, state, root.path());
            }
        }
        Command::Create { edition } => {
            let root = open(&edition)?;
            root.create().map_err(|e| e.to_string())?;
            println!("{} is ready in {}", root.edition.name, root.path());
        }
        Command::Start { edition } => open(&edition)?.start().map_err(|e| e.to_string())?,
        Command::Stop { edition } => open(&edition)?.stop().map_err(|e| e.to_string())?,
        Command::Exec {
            edition,
            user,
            command,
        } => {
            let root = open(&edition)?;
            root.start().map_err(|e| e.to_string())?;
            return exec(&root, user, &command);
        }
        Command::Update { edition } => {
            let snapshot = open(&edition)?
                .update(&label())
                .map_err(|e| e.to_string())?;
            println!("updated; the previous state is in {snapshot}");
        }
        Command::Snapshot { edition } => {
            println!(
                "{}",
                open(&edition)?
                    .snapshot(&label())
                    .map_err(|e| e.to_string())?
            );
        }
        Command::Rollback {
            edition,
            snapshot: None,
            ..
        } => {
            for name in open(&edition)?.snapshots().map_err(|e| e.to_string())? {
                println!("{name}");
            }
        }
        Command::Rollback {
            edition,
            snapshot: Some(snapshot),
            yes,
        } => {
            if !yes {
                println!("Everything in the root after {snapshot} would be lost.");
                println!("Nothing was changed. Run again with --yes to proceed.");
                return Ok(ExitCode::SUCCESS);
            }
            open(&edition)?
                .rollback(&snapshot)
                .map_err(|e| e.to_string())?;
        }
        Command::Remove { edition, yes } => {
            let root = open(&edition)?;
            if !yes {
                println!("Would destroy {}, including its snapshots.", root.path());
                println!("Shared directories such as /home are not affected.");
                println!("Nothing was changed. Run again with --yes to proceed.");
                return Ok(ExitCode::SUCCESS);
            }
            root.remove().map_err(|e| e.to_string())?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Snapshot label from the clock, without depending on a time library.
fn label() -> String {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    milliseconds.to_string()
}

#[cfg(unix)]
fn require_root() -> Result<(), String> {
    if rustix::process::geteuid().is_root() {
        Ok(())
    } else {
        Err("managing compatibility roots needs root; use sudo".into())
    }
}

#[cfg(not(unix))]
fn require_root() -> Result<(), String> {
    Err("not supported on this operating system".into())
}

#[cfg(unix)]
fn exec(root: &Root<'_>, user: Option<String>, command: &[String]) -> Result<ExitCode, String> {
    use std::os::unix::process::CommandExt;
    // Programs run as the person who asked, not as root.
    let user = user
        .or_else(|| std::env::var("SUDO_USER").ok())
        .unwrap_or_else(|| "root".into());
    let valid = !user.is_empty()
        && !user.starts_with('-')
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if !valid {
        return Err(format!("invalid user {user:?}"));
    }
    let directory =
        std::env::current_dir().map_or_else(|_| "/".into(), |d| d.display().to_string());
    let arguments = root.exec_arguments(&user, &directory, command);
    let error = std::process::Command::new(meta_compat::root::JEXEC)
        .args(arguments)
        .exec();
    Err(error.to_string())
}

#[cfg(not(unix))]
fn exec(_root: &Root<'_>, _user: Option<String>, _command: &[String]) -> Result<ExitCode, String> {
    Err("not supported on this operating system".into())
}
