use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_common::{CODENAME, Platform, SystemRunner};
use metaupd::apply::{self, Confirmation};
use metaupd::engine::{EngineError, host_from_platform};
use metaupd::history;
use metaupd::state::State;
use metaupd::store::Paths;
use metaupd::trust_store;
use metaupd::{CheckReport, Engine, METAUPD_VERSION};
use metaupd_github::{CommandFetcher, DirectorySource, GithubSource, ReleaseSource};
use metaupd_metadata::{Channel, Timestamp};

/// Exit status for operations this version does not implement (EX_UNAVAILABLE).
const EXIT_UNAVAILABLE: u8 = 69;

#[derive(Parser)]
#[command(name = "metaupd", version = METAUPD_VERSION, about = "META OTA updater (prototype)")]
struct Cli {
    /// Directory for state, history and downloads
    #[arg(long, global = true, value_name = "DIR")]
    state_dir: Option<PathBuf>,
    /// Directory holding the trusted root metadata
    #[arg(long, global = true, value_name = "DIR")]
    trust_dir: Option<PathBuf>,
    /// Use a local release directory instead of GitHub Releases
    #[arg(long, global = true, value_name = "DIR")]
    source: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Look for a newer release
    Check {
        #[arg(long)]
        json: bool,
    },
    /// Download and verify the newest release
    Download,
    /// Install a downloaded release into a new boot environment
    Install {
        /// Version to install; defaults to the newest downloaded one
        version: Option<String>,
        /// Only show what would happen
        #[arg(long)]
        dry_run: bool,
        /// Confirm the change
        #[arg(long, conflicts_with = "dry_run")]
        yes: bool,
    },
    /// After a reboot, make a healthy update permanent
    Confirm,
    /// Show updater status
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show the update history and verify its integrity
    History,
    /// List boot environments, or select one for the next boot
    Rollback {
        /// Boot environment; defaults to the newest inactive one
        name: Option<String>,
        /// List boot environments and exit
        #[arg(long, conflicts_with_all = ["yes", "once"])]
        list: bool,
        /// Select it for the next boot only
        #[arg(long)]
        once: bool,
        /// Confirm the change
        #[arg(long)]
        yes: bool,
    },
    /// Show or change the update channel
    Channel { channel: Option<Channel> },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("metaupd: {error}");
            match error {
                EngineError::Unsupported(_) => ExitCode::from(EXIT_UNAVAILABLE),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

fn run(cli: Cli) -> Result<(), EngineError> {
    let mut paths = Paths::default();
    if let Some(dir) = cli.state_dir {
        paths.state_dir = dir;
    }
    if let Some(dir) = cli.trust_dir {
        paths.trust_dir = dir;
    }
    let source: Box<dyn ReleaseSource> = match cli.source {
        Some(dir) => Box::new(DirectorySource::new(dir)),
        None => Box::new(GithubSource::official(CommandFetcher)),
    };
    let runner = SystemRunner;
    let platform = Platform::detect(&runner);
    let (host, edition) = host_from_platform(&platform);
    let engine = Engine {
        paths,
        source: source.as_ref(),
        runner: &runner,
        now: Timestamp::now(),
        host,
        edition,
        require_separate_state: true,
    };
    match cli.command.unwrap_or(Command::Status { json: false }) {
        Command::Check { json } => check(&engine, json),
        Command::Download => download(&engine),
        Command::Install {
            version,
            dry_run,
            yes,
        } => install(&engine, version, dry_run, yes),
        Command::Confirm => confirm(&engine),
        Command::Status { json } => status(&engine, json),
        Command::History => show_history(&engine),
        Command::Rollback {
            name,
            list,
            once,
            yes,
        } => rollback(&engine, name.as_deref(), list, once, yes),
        Command::Channel { channel: None } => {
            println!("{}", engine.channel()?);
            Ok(())
        }
        Command::Channel {
            channel: Some(channel),
        } => {
            engine.set_channel(channel)?;
            println!("update channel set to {channel}");
            Ok(())
        }
    }
}

fn print_problems(report: &CheckReport) {
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    for (tag, reason) in &report.rejected {
        eprintln!("rejected {tag}: {reason}");
    }
}

fn check(engine: &Engine<'_>, json: bool) -> Result<(), EngineError> {
    let report = engine.check()?;
    if json {
        let value = serde_json::json!({
            "source": engine.source.describe(),
            "offer": report.offer.as_ref().map(|offer| serde_json::json!({
                "tag": offer.release.tag,
                "manifest": offer.manifest.manifest(),
                "manifest_sha256": offer.manifest.digest(),
                "incompatibilities":
                    offer.incompatibilities.iter().map(ToString::to_string).collect::<Vec<_>>(),
            })),
            "rejected": report.rejected,
            "warnings": report.warnings,
        });
        println!("{value:#}");
        return Ok(());
    }
    print_problems(&report);
    match &report.offer {
        None => println!("No update is available from {}.", engine.source.describe()),
        Some(offer) => {
            let manifest = offer.manifest.manifest();
            let total: u64 = manifest.artifacts_for(engine.edition).map(|a| a.size).sum();
            println!(
                "META {} \"{}\" ({})",
                manifest.version, manifest.codename, manifest.channel
            );
            println!("  sequence   {}", manifest.sequence);
            println!("  published  {}", manifest.created);
            println!(
                "  artifacts  {} ({total} bytes)",
                manifest.artifacts_for(engine.edition).count()
            );
            for migration in &manifest.migrations {
                let note = if migration.reversible {
                    ""
                } else {
                    " (not reversible)"
                };
                println!("  migration  {}: {}{note}", migration.id, migration.summary);
            }
            for reason in &offer.incompatibilities {
                println!("  cannot install: {reason}");
            }
        }
    }
    Ok(())
}

fn download(engine: &Engine<'_>) -> Result<(), EngineError> {
    let (offer, directory) = engine.download()?;
    println!(
        "META {} verified in {}",
        offer.manifest.manifest().version,
        directory.display()
    );
    for reason in &offer.incompatibilities {
        println!("  cannot install: {reason}");
    }
    Ok(())
}

fn install(
    engine: &Engine<'_>,
    version: Option<String>,
    dry_run: bool,
    yes: bool,
) -> Result<(), EngineError> {
    let version = match version {
        Some(version) => version,
        None => engine
            .staged_versions()
            .first()
            .map(ToString::to_string)
            .ok_or_else(|| EngineError::NotStaged("release".into()))?,
    };
    let staged = engine.staged(&version)?;
    let plan = engine.plan(&staged);
    println!(
        "META {version}: every staged artifact verified in {}",
        staged.directory.display()
    );
    println!("Plan:");
    for (index, step) in plan.steps.iter().enumerate() {
        let mark = if step.undone_by_rollback {
            ""
        } else {
            "  [not undone by rollback]"
        };
        println!("  {:2}. {}{mark}", index + 1, step.description);
    }
    if plan.requires_reboot {
        println!("A reboot is required.");
    }
    if !staged.incompatibilities.is_empty() {
        return Err(EngineError::Incompatible(staged.incompatibilities));
    }
    if dry_run {
        return Ok(());
    }
    if !yes {
        println!("Nothing was changed. Run again with --yes to install.");
        return Ok(());
    }
    let pending = apply::apply(engine, &staged)?;
    println!(
        "META {version} is installed in {}.",
        pending.boot_environment
    );
    println!(
        "Reboot to try it. Unless it is confirmed, the boot after that returns to {}.",
        pending.previous
    );
    Ok(())
}

fn confirm(engine: &Engine<'_>) -> Result<(), EngineError> {
    match apply::confirm(engine)? {
        Confirmation::Nothing => println!("No update is waiting for confirmation."),
        Confirmation::Confirmed(pending) => {
            println!(
                "META {} is now permanent in {}.",
                pending.version, pending.boot_environment
            );
        }
        Confirmation::Unhealthy(pending, reason) => {
            println!("META {} was not confirmed: {reason}.", pending.version);
            println!("The next boot returns to {}.", pending.previous);
        }
        Confirmation::Reverted(pending) => {
            println!(
                "META {} did not stay active; the previous system is running.",
                pending.version
            );
        }
    }
    Ok(())
}

fn status(engine: &Engine<'_>, json: bool) -> Result<(), EngineError> {
    let state = State::load(&engine.paths)?;
    let trust = trust_store::load(&engine.paths, state.root_version);
    let staged = engine.staged_versions();
    let environments = engine.boot_environments();
    if json {
        let value = serde_json::json!({
            "metaupd_version": METAUPD_VERSION,
            "installed": engine.host.meta_version,
            "channel": state.channel,
            "last_check": state.last_check,
            "trusted_root_version": trust.as_ref().ok().map(|root| root.version()),
            "trust_error": trust.as_ref().err().map(ToString::to_string),
            "staged": staged,
            "pending": state.pending,
            "boot_environments": environments.as_ref().ok(),
        });
        println!("{value:#}");
        return Ok(());
    }
    let installed = match &engine.host.meta_version {
        Some(version) => version.to_string(),
        None => "not an installed META system".to_owned(),
    };
    println!("metaupd            {METAUPD_VERSION} (\"{CODENAME}\", prototype)");
    println!("installed          {installed}");
    println!("channel            {}", state.channel);
    println!("source             {}", engine.source.describe());
    match state.last_check {
        Some(time) => println!("last check         {time}"),
        None => println!("last check         never"),
    }
    match trust {
        Ok(root) => println!(
            "trusted root       version {}, expires {}",
            root.version(),
            root.metadata().expires
        ),
        Err(error) => println!("trusted root       unavailable: {error}"),
    }
    let staged: Vec<String> = staged.iter().map(ToString::to_string).collect();
    println!(
        "downloaded         {}",
        if staged.is_empty() {
            "none".into()
        } else {
            staged.join(", ")
        }
    );
    if let Some(pending) = &state.pending {
        println!(
            "waiting            META {} in {}",
            pending.version, pending.boot_environment
        );
    }
    match environments {
        Ok(list) => println!("boot environments  {}", list.len()),
        Err(error) => println!("boot environments  {error}"),
    }
    Ok(())
}

fn show_history(engine: &Engine<'_>) -> Result<(), EngineError> {
    let history = history::load(&engine.paths)?;
    if history.records.is_empty() {
        println!("No history.");
    }
    for record in &history.records {
        let action = record.action.as_str();
        let outcome = if record.success { "ok" } else { "failed" };
        println!(
            "{}  {action:13}  {outcome:6}  {}",
            record.time,
            record.detail.escape_debug()
        );
    }
    Ok(())
}

fn rollback(
    engine: &Engine<'_>,
    name: Option<&str>,
    list: bool,
    once: bool,
    yes: bool,
) -> Result<(), EngineError> {
    let environments = engine.boot_environments()?;
    if list {
        for be in &environments {
            let mut flags = String::new();
            for (set, flag) in [
                (be.active_now, 'N'),
                (be.active_on_reboot, 'R'),
                (be.active_once, 'T'),
            ] {
                if set {
                    flags.push(flag);
                }
            }
            println!("{:32} {flags:3} {:>8}  {}", be.name, be.space, be.created);
        }
        return Ok(());
    }
    let target = metaupd_rollback::choose_target(&environments, name)?;
    let scope = if once {
        "the next boot only"
    } else {
        "every following boot"
    };
    if !yes {
        println!("Would select boot environment {} for {scope}.", target.name);
        println!("Firmware, EFI system partition and data outside it are not reverted.");
        println!("Nothing was changed. Run again with --yes to proceed.");
        return Ok(());
    }
    engine.rollback(&target.name, once)?;
    println!(
        "{} will be used for {scope}. Reboot to complete the rollback.",
        target.name
    );
    Ok(())
}
