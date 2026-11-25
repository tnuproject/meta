//! The installer's interface. Collects choices and writes a plan; with
//! `--install` it then hands the plan to meta-install-run.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use clap::Parser;
use meta_common::{Platform, SystemRunner};
use meta_eds_detect::detect;
use meta_installer::app::{App, Outcome};
use meta_installer::config::InstallConfig;
use meta_installer::disks;
use meta_installer::plan::Plan;
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{Clear, ClearType};

#[derive(Parser)]
#[command(name = "meta-installer", version, about = "META installer")]
struct Cli {
    /// Validate an answer file and print its plan instead of starting the interface
    #[arg(long, value_name = "FILE")]
    answers: Option<PathBuf>,
    /// Where the interface saves the plan
    #[arg(long, value_name = "FILE", default_value = "meta-install-plan.json")]
    output: PathBuf,
    /// Carry the plan out afterwards, which asks for confirmation
    #[arg(long)]
    install: bool,
    /// Arguments for meta-install-run, after --
    #[arg(last = true, requires = "install")]
    run_arguments: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match &cli.answers {
        Some(path) => from_answers(path),
        None => interactive(&cli),
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("meta-installer: {error}");
            ExitCode::FAILURE
        }
    }
}

fn from_answers(path: &Path) -> Result<ExitCode, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: InstallConfig =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let plan = Plan::build(&config).map_err(|problems| problems.join("\n"))?;
    println!("{}", plan.to_json());
    Ok(ExitCode::SUCCESS)
}

fn interactive(cli: &Cli) -> Result<ExitCode, String> {
    let platform = Platform::detect(&SystemRunner);
    let (disks, disk_notice) = disks::detect(&platform, &SystemRunner);
    let inventory = platform.is_freebsd().then(|| detect(&SystemRunner));
    let mut app = App::new(disks, disk_notice, inventory);
    let mut terminal = ratatui::try_init().map_err(|e| format!("cannot use the terminal: {e}"))?;
    // FreeBSD's console has no alternate screen, so earlier output would remain.
    // Terminal::clear asks for the cursor position, which serial lines may not
    // answer.
    let cleared = execute!(std::io::stdout(), Clear(ClearType::All));
    let outcome = cleared.and_then(|()| app.run(&mut terminal));
    ratatui::restore();
    match outcome.map_err(|e| e.to_string())? {
        Outcome::Quit => {
            println!("No plan was saved.");
            Ok(ExitCode::FAILURE)
        }
        Outcome::SavePlan(plan) => {
            std::fs::write(&cli.output, plan.to_json() + "\n")
                .map_err(|e| format!("{}: {e}", cli.output.display()))?;
            println!(
                "Plan saved to {}. Nothing was installed.",
                cli.output.display()
            );
            if cli.install {
                hand_over(cli, &plan)
            } else {
                Ok(ExitCode::SUCCESS)
            }
        }
    }
}

/// Runs meta-install-run, passing the password on its standard input.
fn hand_over(cli: &Cli, plan: &Plan) -> Result<ExitCode, String> {
    let program = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .with_file_name(format!("meta-install-run{}", std::env::consts::EXE_SUFFIX));
    let mut child = Command::new(&program)
        .arg("--plan")
        .arg(&cli.output)
        .arg("--password-stdin")
        .args(&cli.run_arguments)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let password = plan.configuration.user.password.expose();
        stdin
            .write_all(password.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .map_err(|e| e.to_string())?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
