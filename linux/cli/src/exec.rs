//! `meta-exec`: where a program would run and why, and running it there.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_linux::{
    RunArguments, Selection, diagnose, print_decision, print_json, print_profile, profile, run,
    trial,
};
use meta_maes::launch::{EXIT_LAUNCHER, EXIT_NOT_RUNNABLE};

#[derive(Parser)]
#[command(
    name = "meta-exec",
    version,
    about = "Run a program in the runtime it needs"
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
    /// Show what a program needs, and how that is known
    Analyze {
        /// Run the program as you, under truss, and remember what it lacked
        #[arg(long)]
        trial: bool,
        program: PathBuf,
        /// Arguments for a trial
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Show where a program would run, and why not elsewhere
    Explain {
        #[command(flatten)]
        selection: Selection,
        /// Print the graph in DOT format
        #[arg(long)]
        graph: bool,
        program: PathBuf,
    },
    /// Run a program; its exit code is passed on
    Run(RunArguments),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Analyze {
            trial,
            program,
            arguments,
        } => analyze(&program, trial, &arguments, cli.json),
        Command::Explain {
            selection,
            graph,
            program,
        } => explain(&program, &selection, graph, cli.json),
        Command::Run(arguments) => return run("meta-exec", &arguments),
    };
    result.unwrap_or_else(|error| {
        eprintln!("meta-exec: {error}");
        ExitCode::from(EXIT_LAUNCHER)
    })
}

fn analyze(
    program: &std::path::Path,
    wanted: bool,
    arguments: &[String],
    json: bool,
) -> Result<ExitCode, String> {
    if wanted {
        let missing = trial(program, arguments)?;
        if !json {
            println!("trial    {} calls were not implemented", missing.len());
        }
    }
    let profile = profile(program)?;
    if json {
        print_json(&profile)?
    } else {
        print_profile(&profile)
    }
    Ok(ExitCode::SUCCESS)
}

fn explain(
    program: &std::path::Path,
    selection: &Selection,
    graph: bool,
    json: bool,
) -> Result<ExitCode, String> {
    let diagnosis = diagnose(program, selection)?;
    if graph {
        print!("{}", diagnosis.decision.graph.to_dot());
    } else if json {
        print_json(&diagnosis)?;
    } else {
        print_decision(&diagnosis.decision);
    }
    let found = diagnosis.decision.choice().is_some();
    Ok(if found {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_NOT_RUNNABLE)
    })
}
