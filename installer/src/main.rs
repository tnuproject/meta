//! META installer.
//!
//! A plan-only prototype: it collects choices, validates them and writes an
//! installation plan. It contains no code that partitions, formats or writes
//! to a disk.

mod app;
mod catalog;
mod config;
mod disks;
mod plan;
mod screens;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use meta_common::{Platform, SystemRunner};
use meta_eds_detect::detect;

use crate::app::{App, Outcome};
use crate::config::InstallConfig;
use crate::plan::Plan;

#[derive(Parser)]
#[command(name = "meta-installer", version, about = "META installer (plan only)")]
struct Cli {
    /// Validate an answer file and print its plan instead of starting the interface
    #[arg(long, value_name = "FILE")]
    answers: Option<PathBuf>,
    /// Where the interface saves the plan
    #[arg(long, value_name = "FILE", default_value = "meta-install-plan.json")]
    output: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.answers {
        Some(path) => from_answers(&path),
        None => interactive(&cli.output),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("meta-installer: {error}");
            ExitCode::FAILURE
        }
    }
}

fn from_answers(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: InstallConfig =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let plan = Plan::build(&config).map_err(|problems| problems.join("\n"))?;
    println!("{}", plan.to_json());
    Ok(())
}

fn interactive(output: &Path) -> Result<(), String> {
    let platform = Platform::detect(&SystemRunner);
    let (disks, disk_notice) = disks::detect(&platform, &SystemRunner);
    let inventory = platform.is_freebsd().then(|| detect(&SystemRunner));
    let mut app = App::new(disks, disk_notice, inventory);
    let mut terminal = ratatui::try_init().map_err(|e| format!("cannot use the terminal: {e}"))?;
    let outcome = app.run(&mut terminal);
    ratatui::restore();
    match outcome.map_err(|e| e.to_string())? {
        Outcome::Quit => {
            println!("No plan was saved.");
            Ok(())
        }
        Outcome::SavePlan(plan) => {
            std::fs::write(output, plan.to_json() + "\n")
                .map_err(|e| format!("{}: {e}", output.display()))?;
            println!("Plan saved to {}. Nothing was installed.", output.display());
            Ok(())
        }
    }
}
