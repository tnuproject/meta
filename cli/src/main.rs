//! The `meta` command.

mod compat;
mod doctor;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_common::{
    CODENAME, CommandRunner, PROJECT, Platform, RELEASE_STAGE, SystemRunner, TAGLINE, VERSION,
    components,
};
use meta_eds_db::{Database, Validation};
use meta_eds_detect::detect;

#[derive(Parser)]
#[command(name = "meta", about = "META system information and diagnostics")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show the META version
    Version,
    /// Show the platform and the status of every META component
    Info {
        #[arg(long)]
        json: bool,
    },
    /// Show detected hardware and what is known about it
    Hardware {
        #[arg(long)]
        json: bool,
    },
    /// Network information
    Network {
        #[command(subcommand)]
        command: NetworkCommand,
    },
    /// Linux compatibility
    Compatibility {
        #[command(subcommand)]
        command: CompatibilityCommand,
    },
    /// Start the installer (which only produces a plan)
    Installer {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        arguments: Vec<String>,
    },
    /// Check this system for problems
    Doctor,
}

#[derive(Subcommand)]
enum NetworkCommand {
    /// Show interfaces, routes and name servers
    Status {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CompatibilityCommand {
    /// Check which Linux programs in the given files or directories could run
    Scan {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Also list programs without problems
        #[arg(long)]
        all: bool,
    },
}

fn main() -> ExitCode {
    let runner = SystemRunner;
    let result = match Cli::parse().command {
        Command::Version => {
            println!("{PROJECT} {VERSION} \"{CODENAME}\" ({RELEASE_STAGE})");
            println!("{TAGLINE}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Info { json } => info(&runner, json),
        Command::Hardware { json } => hardware(&runner, json),
        Command::Network {
            command: NetworkCommand::Status { json },
        } => network(&runner, json),
        Command::Compatibility {
            command: CompatibilityCommand::Scan { paths, all },
        } => compat::scan(&runner, &paths, all),
        Command::Installer { arguments } => installer(&arguments),
        Command::Doctor => Ok(doctor::run(&runner)),
    };
    result.unwrap_or_else(|error| {
        eprintln!("meta: {error}");
        ExitCode::FAILURE
    })
}

fn info(runner: &dyn CommandRunner, json: bool) -> Result<ExitCode, String> {
    let platform = Platform::detect(runner);
    let components = components();
    if json {
        let value = serde_json::json!({
            "project": PROJECT,
            "version": VERSION,
            "codename": CODENAME,
            "stage": RELEASE_STAGE,
            "platform": platform,
            "components": components,
        });
        println!("{value:#}");
        return Ok(ExitCode::SUCCESS);
    }
    println!("{PROJECT} {VERSION} \"{CODENAME}\" ({RELEASE_STAGE})");
    let unknown = || "unknown".to_owned();
    println!("  host          {} {}", platform.os_name, platform.arch);
    if platform.is_freebsd() {
        println!(
            "  kernel        {}",
            platform.kernel_version.clone().unwrap_or_else(unknown)
        );
        println!(
            "  userland      {}",
            platform.userland_version.clone().unwrap_or_else(unknown)
        );
        match platform.osreldate {
            Some(version) => println!("  osreldate     {version}"),
            None => println!("  osreldate     unknown"),
        }
    }
    match &platform.meta_release {
        Some(release) => println!("  installed     META {}", release.version),
        None => println!("  installed     not an installed META system"),
    }
    println!("\nComponents:");
    for component in components {
        println!(
            "  {:18} {:18} {}",
            component.name, component.status, component.summary
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn hardware(runner: &dyn CommandRunner, json: bool) -> Result<ExitCode, String> {
    Platform::detect(runner).require_freebsd("hardware detection")?;
    let inventory = detect(runner);
    let database = Database::builtin();
    if json {
        let devices: Vec<_> = inventory
            .devices
            .iter()
            .map(|device| serde_json::json!({ "device": device, "entries": database.lookup(device) }))
            .collect();
        let value = serde_json::json!({ "devices": devices, "unavailable": inventory.unavailable });
        println!("{value:#}");
        return Ok(ExitCode::SUCCESS);
    }
    for device in &inventory.devices {
        let driver = device.driver.as_deref().unwrap_or("-");
        let known = match database.lookup(device).first() {
            Some(entry) if entry.validation == Validation::Validated => {
                format!("{} (tested by META)", entry.name)
            }
            Some(entry) if device.has_specific_driver() => {
                format!("{} (not tested by META)", entry.name)
            }
            Some(entry) => format!("candidate: {} (not tested by META)", entry.name),
            None => device
                .description
                .clone()
                .unwrap_or_else(|| "not in database".to_owned()),
        };
        println!(
            "{:14} {:04x}:{:04x}  {:14} {known}",
            device.location, device.vendor, device.device, driver
        );
    }
    for (bus, reason) in &inventory.unavailable {
        eprintln!("warning: {bus:?} devices not shown: {reason}");
    }
    Ok(ExitCode::SUCCESS)
}

fn network(runner: &dyn CommandRunner, json: bool) -> Result<ExitCode, String> {
    Platform::detect(runner).require_freebsd("network status")?;
    let status = meta_net::status(runner).map_err(|e| e.to_string())?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).map_err(|e| e.to_string())?
        );
        return Ok(ExitCode::SUCCESS);
    }
    for interface in &status.interfaces {
        let state = match (interface.up, interface.status.as_deref()) {
            (false, _) => "down",
            (true, Some(status)) => status,
            (true, None) => "up",
        };
        print!("{:10} {state}", interface.name);
        if let Some(ssid) = &interface.ssid {
            print!("  \"{ssid}\"");
        }
        println!();
        for address in &interface.addresses {
            println!("{:10} {address}", "");
        }
    }
    for (family, route) in [
        ("IPv4", &status.default_ipv4),
        ("IPv6", &status.default_ipv6),
    ] {
        match route {
            Some(route) => println!(
                "{family} default via {} on {}",
                route.gateway, route.interface
            ),
            None => println!("{family} has no default route"),
        }
    }
    println!("name servers: {}", status.nameservers.join(", "));
    Ok(ExitCode::SUCCESS)
}

/// Runs the installer that sits next to this program.
fn installer(arguments: &[String]) -> Result<ExitCode, String> {
    let own = std::env::current_exe().map_err(|e| e.to_string())?;
    let program = own.with_file_name(format!("meta-installer{}", std::env::consts::EXE_SUFFIX));
    if !program.is_file() {
        return Err(format!(
            "meta-installer is not installed at {}",
            program.display()
        ));
    }
    let status = std::process::Command::new(&program)
        .args(arguments)
        .status()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
