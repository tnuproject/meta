//! Extended Driver Support command line tool.
//!
//! Detection and reporting work. Installing drivers does not exist yet: `enable`
//! and `disable` record the administrator's policy and nothing else.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meta_common::{Platform, SystemRunner};
use meta_eds_db::{Database, Entry, Support, Validation};
use meta_eds_detect::{Device, Inventory, detect};
use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG: &str = "/usr/local/etc/meta/eds.toml";
const EXIT_UNAVAILABLE: u8 = 69;

#[derive(Parser)]
#[command(
    name = "meta-eds",
    version,
    about = "Extended Driver Support (prototype)"
)]
struct Cli {
    /// Policy file
    #[arg(long, global = true, default_value = DEFAULT_CONFIG, value_name = "FILE")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show whether EDS is enabled
    Status,
    /// Detect hardware and show what is known about it
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Record that EDS should be active
    Enable,
    /// Record that EDS should not be active
    Disable,
    /// Update drivers (not implemented)
    Update,
    /// List the compatibility database
    List,
    /// Show one database entry
    Info { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    enabled: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Status => status(&cli.config),
        Command::Scan { json } => scan(json),
        Command::Enable => set_policy(&cli.config, true),
        Command::Disable => set_policy(&cli.config, false),
        Command::Update => {
            eprintln!(
                "meta-eds: driver updates are delivered by metaupd, which cannot apply them yet"
            );
            return ExitCode::from(EXIT_UNAVAILABLE);
        }
        Command::List => {
            list();
            Ok(())
        }
        Command::Info { id } => info(&id),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("meta-eds: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_policy(path: &Path) -> Result<Option<Policy>, String> {
    match fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{}: {e}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn set_policy(path: &Path, enabled: bool) -> Result<(), String> {
    let at = |e: std::io::Error| format!("{}: {e}", path.display());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(at)?;
    }
    let text = toml::to_string(&Policy { enabled }).map_err(|e| e.to_string())?;
    fs::write(path, text).map_err(at)?;
    println!(
        "EDS policy: {}",
        if enabled { "enabled" } else { "disabled" }
    );
    println!("Only the policy was recorded; this version does not install or remove drivers.");
    Ok(())
}

fn status(path: &Path) -> Result<(), String> {
    let policy = match load_policy(path)? {
        Some(Policy { enabled: true }) => "enabled",
        Some(Policy { enabled: false }) => "disabled",
        None => "not configured",
    };
    let database = Database::builtin();
    let validated = database
        .entries()
        .iter()
        .filter(|e| e.validation == Validation::Validated)
        .count();
    println!("policy     {policy}");
    println!(
        "database   {} entries, {validated} validated by META",
        database.entries().len()
    );
    println!("installer  not implemented");
    Ok(())
}

fn inventory() -> Result<Inventory, String> {
    Platform::detect(&SystemRunner).require_freebsd("hardware detection")?;
    Ok(detect(&SystemRunner))
}

fn scan(json: bool) -> Result<(), String> {
    let inventory = inventory()?;
    let database = Database::builtin();
    if json {
        let devices: Vec<_> = inventory
            .devices
            .iter()
            .map(|device| {
                let entries: Vec<_> = database.lookup(device).iter().map(|e| &e.id).collect();
                serde_json::json!({ "device": device, "kind": device.kind(), "entries": entries })
            })
            .collect();
        let value = serde_json::json!({
            "devices": devices,
            "unavailable": inventory.unavailable,
        });
        println!("{value:#}");
        return Ok(());
    }
    for device in &inventory.devices {
        print_device(device, &database.lookup(device));
    }
    for (bus, reason) in &inventory.unavailable {
        eprintln!("warning: {bus:?} devices not shown: {reason}");
    }
    Ok(())
}

fn print_device(device: &Device, entries: &[&Entry]) {
    println!(
        "{:14} {:04x}:{:04x}  {:13}  driver {}",
        device.location,
        device.vendor,
        device.device,
        label(&device.kind()),
        device.driver.as_deref().unwrap_or("-"),
    );
    for entry in entries {
        let candidate = if device.has_specific_driver() {
            ""
        } else {
            "candidate: "
        };
        println!(
            "    {candidate}{} [{}] {}",
            entry.name,
            entry.id,
            describe(entry)
        );
    }
}

fn describe(entry: &Entry) -> String {
    let tested = match entry.validation {
        Validation::Validated => "tested by META",
        Validation::Documented => "not tested by META",
    };
    format!("{}, {tested}", support_label(entry.support))
}

fn support_label(support: Support) -> &'static str {
    match support {
        Support::Native => "native FreeBSD driver",
        Support::Linuxkpi => "LinuxKPI driver",
        Support::FirmwareOnly => "firmware only",
        Support::Userspace => "userspace driver",
        Support::Unsupported => "unsupported",
    }
}

/// The kebab-case name serde uses.
fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn list() {
    for entry in Database::builtin().entries() {
        println!(
            "{:24} {:11} {:11} {}",
            entry.id,
            label(&entry.category),
            label(&entry.support),
            entry.name
        );
    }
}

fn info(id: &str) -> Result<(), String> {
    let database = Database::builtin();
    let entry = database
        .get(id)
        .ok_or_else(|| format!("no database entry {id:?}"))?;
    println!("{}", entry.name);
    println!("  id        {}", entry.id);
    println!("  category  {}", label(&entry.category));
    println!("  support   {}", describe(entry));
    for (name, values) in [("modules", &entry.modules), ("packages", &entry.packages)] {
        if !values.is_empty() {
            println!("  {name:9} {}", values.join(", "));
        }
    }
    println!("  source    {}", entry.source);
    if let Some(notes) = &entry.notes {
        println!("  notes     {notes}");
    }
    for test in &entry.tested_on {
        println!(
            "  tested    {} on {} ({}): {}",
            test.release, test.machine, test.date, test.result
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/eds.toml");
        assert_eq!(load_policy(&path).unwrap(), None);
        set_policy(&path, true).unwrap();
        assert_eq!(load_policy(&path).unwrap(), Some(Policy { enabled: true }));
        set_policy(&path, false).unwrap();
        assert_eq!(load_policy(&path).unwrap(), Some(Policy { enabled: false }));
        fs::write(&path, "enabled = 3").unwrap();
        assert!(load_policy(&path).is_err());
    }
}
