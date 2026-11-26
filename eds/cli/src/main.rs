//! Extended Driver Support command line tool, the front end of MDCF.
//!
//! Everything that only looks needs no confirmation. Everything that changes the
//! system shows a plan and needs `--yes`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use meta_common::{Platform, SystemRunner};
use meta_eds_db::{Database, Entry, Validation};
use meta_eds_detect::{Device, Inventory, detect};
use meta_mdcf::lifecycle::{self, Request};
use meta_mdcf::{
    Assessment, HostFacts, KpiRegistry, Operation, Outcome, Plan, assess_device, capability,
    firmware, predict,
};
use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG: &str = "/usr/local/etc/meta/eds.toml";
const EXIT_UNAVAILABLE: u8 = 69;
/// `verify` found something wrong.
const EXIT_PROBLEM: u8 = 2;

#[derive(Parser)]
#[command(name = "meta-eds", version, about = "Extended Driver Support")]
struct Cli {
    /// Policy file
    #[arg(long, global = true, default_value = DEFAULT_CONFIG, value_name = "FILE")]
    config: PathBuf,
    /// Machine-readable output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone)]
struct Change {
    /// Database entry; see `meta-eds drivers`
    id: String,
    /// Carry the plan out
    #[arg(long)]
    yes: bool,
    /// Accept a component META has not tested
    #[arg(long)]
    accept_untested: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Show whether EDS is enabled, and the size of its databases
    Status,
    /// Detect hardware and show what is known about it
    Scan,
    /// Show detected hardware in full
    Hardware,
    /// Show every driver candidate for the detected hardware
    Drivers,
    /// Show firmware the detected hardware needs
    Firmware,
    /// Assess drivers, for all detected hardware or one entry
    Compatibility { id: Option<String> },
    /// Show what the hardware can do
    Capabilities,
    /// Show what LinuxKPI offers, or the state of an interface
    Linuxkpi { interface: Option<String> },
    /// Predict from source whether a Linux driver could be ported
    Predict {
        /// Directory with the driver's source
        source: PathBuf,
    },
    /// Look for driver and firmware problems
    Diagnose,
    /// Show the plan for an operation without carrying it out
    Plan {
        operation: OperationArg,
        id: String,
        #[arg(long)]
        accept_untested: bool,
    },
    /// Download a component's packages
    Prepare(Change),
    /// Install a component's packages
    Install(Change),
    /// Turn EDS on, or load a component now and at boot
    Enable {
        id: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        accept_untested: bool,
    },
    /// Turn EDS off, or stop loading a component at boot
    Disable {
        id: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Remove a component's packages
    Remove(Change),
    /// Check that a component is installed, loaded and attached
    Verify { id: String },
    /// Print a report about hardware and drivers for a bug report
    Report,
    /// Update drivers (delivered by metaupd)
    Update,
    /// List the compatibility database
    List,
    /// Show one database entry
    Info { id: String },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum OperationArg {
    Prepare,
    Install,
    Enable,
    Disable,
    Remove,
}

impl From<OperationArg> for Operation {
    fn from(value: OperationArg) -> Self {
        match value {
            OperationArg::Prepare => Operation::Prepare,
            OperationArg::Install => Operation::Install,
            OperationArg::Enable => Operation::Enable,
            OperationArg::Disable => Operation::Disable,
            OperationArg::Remove => Operation::Remove,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    enabled: bool,
}

/// Everything a decision about this machine is based on.
struct Machine {
    inventory: Inventory,
    host: HostFacts,
    database: Database,
    kpi: KpiRegistry,
}

impl Machine {
    fn detect() -> Result<Self, String> {
        let platform = Platform::detect(&SystemRunner);
        platform.require_freebsd("hardware detection")?;
        Ok(Self {
            inventory: detect(&SystemRunner),
            host: HostFacts::collect(&platform, &SystemRunner),
            database: Database::builtin(),
            kpi: KpiRegistry::builtin(),
        })
    }

    fn assess(&self, device: &Device) -> Vec<Assessment> {
        assess_device(device, &self.database.lookup(device), &self.host, &self.kpi)
    }

    fn entry(&self, id: &str) -> Result<&Entry, String> {
        self.database
            .get(id)
            .ok_or_else(|| format!("no database entry {id:?}"))
    }

    /// The best assessment of an entry over all devices it applies to.
    fn best(&self, id: &str) -> Option<Assessment> {
        self.inventory
            .devices
            .iter()
            .flat_map(|device| self.assess(device))
            .filter(|assessment| assessment.driver == id)
            .max_by_key(|assessment| assessment.verdict)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("meta-eds: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    let json = cli.json;
    match cli.command {
        Command::Status => status(&cli.config, json)?,
        Command::Scan => scan(json)?,
        Command::Hardware => hardware(json)?,
        Command::Drivers => drivers(json)?,
        Command::Firmware => firmware_command(json)?,
        Command::Compatibility { id } => compatibility(id.as_deref(), json)?,
        Command::Capabilities => capabilities(json)?,
        Command::Linuxkpi { interface } => linuxkpi(interface.as_deref(), json)?,
        Command::Predict { source } => predict_command(&source, json)?,
        Command::Diagnose => diagnose(json)?,
        Command::Plan {
            operation,
            id,
            accept_untested,
        } => {
            let plan = make_plan(&Machine::detect()?, operation.into(), &id, accept_untested)?;
            show_plan(&plan, json)?;
        }
        Command::Prepare(c) => change(Operation::Prepare, &c, json)?,
        Command::Install(c) => change(Operation::Install, &c, json)?,
        Command::Remove(c) => change(Operation::Remove, &c, json)?,
        Command::Enable { id: None, .. } => set_policy(&cli.config, true)?,
        Command::Enable {
            id: Some(id),
            yes,
            accept_untested,
        } => {
            change(
                Operation::Enable,
                &Change {
                    id,
                    yes,
                    accept_untested,
                },
                json,
            )?;
        }
        Command::Disable { id: None, .. } => set_policy(&cli.config, false)?,
        Command::Disable { id: Some(id), yes } => {
            change(
                Operation::Disable,
                &Change {
                    id,
                    yes,
                    accept_untested: false,
                },
                json,
            )?;
        }
        Command::Verify { id } => return verify(&id, json),
        Command::Report => report(&cli.config)?,
        Command::Update => {
            eprintln!("meta-eds: driver updates are delivered by metaupd as part of a release");
            return Ok(ExitCode::from(EXIT_UNAVAILABLE));
        }
        Command::List => list(json)?,
        Command::Info { id } => info(&id, json)?,
    }
    Ok(ExitCode::SUCCESS)
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}

/// The kebab-case name serde uses.
fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
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
    println!("Only the policy was recorded. Components are changed one by one, by name.");
    Ok(())
}

fn status(path: &Path, json: bool) -> Result<(), String> {
    let policy = match load_policy(path)? {
        Some(Policy { enabled: true }) => "enabled",
        Some(Policy { enabled: false }) => "disabled",
        None => "not configured",
    };
    let database = Database::builtin();
    let kpi = KpiRegistry::builtin();
    let entries = database.entries().len();
    let validated = database
        .entries()
        .iter()
        .filter(|e| e.validation == Validation::Validated)
        .count();
    let (declared, unfinished) = kpi
        .areas()
        .values()
        .fold((0, 0), |sum, area| (sum.0 + area.0, sum.1 + area.1));
    if json {
        return print_json(&serde_json::json!({
            "policy": policy, "entries": entries, "validated": validated,
            "linuxkpi": { "release": kpi.release, "declared": declared, "unfinished": unfinished },
        }));
    }
    println!("policy     {policy}");
    println!("database   {entries} entries, {validated} validated by META");
    println!(
        "linuxkpi   {declared} interfaces declared, {unfinished} unfinished ({})",
        kpi.release
    );
    Ok(())
}

fn warn_unavailable(inventory: &Inventory) {
    for (bus, reason) in &inventory.unavailable {
        eprintln!("warning: {bus:?} devices not shown: {reason}");
    }
}

fn scan(json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    if json {
        let devices: Vec<_> = machine
            .inventory
            .devices
            .iter()
            .map(|device| {
                serde_json::json!({
                    "device": device,
                    "kind": device.kind(),
                    "assessments": machine.assess(device),
                })
            })
            .collect();
        return print_json(&serde_json::json!({
            "devices": devices, "unavailable": machine.inventory.unavailable,
        }));
    }
    for device in &machine.inventory.devices {
        println!("{}", device_line(device));
        for assessment in machine.assess(device) {
            println!(
                "    {:21} {} [{}]",
                assessment.verdict.as_str(),
                assessment.name,
                assessment.driver
            );
        }
    }
    warn_unavailable(&machine.inventory);
    Ok(())
}

fn device_line(device: &Device) -> String {
    let id = match &device.hardware_id {
        Some(id) => id.clone(),
        None => format!("{:04x}:{:04x}", device.vendor, device.device),
    };
    format!(
        "{:20} {id:10} {:14} {}",
        device.location,
        label(&device.kind()),
        device.driver.as_deref().unwrap_or("-")
    )
}

fn hardware(json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    if json {
        return print_json(&machine.inventory);
    }
    for device in &machine.inventory.devices {
        println!("{}", device_line(device));
        if let Some((vendor, id)) = device.subsystem {
            println!("    subsystem    {vendor:04x}:{id:04x}");
        }
        if let Some(revision) = device.revision {
            println!("    revision     {revision:#04x}");
        }
        if let Some(description) = &device.description {
            println!("    description  {description}");
        }
        if !device.capabilities.is_empty() {
            println!("    capabilities {}", device.capabilities.join(", "));
        }
        if !device.interface_classes.is_empty() {
            let classes: Vec<String> = device
                .interface_classes
                .iter()
                .map(|c| format!("{c:#04x}"))
                .collect();
            println!("    interfaces   {}", classes.join(", "));
        }
    }
    warn_unavailable(&machine.inventory);
    Ok(())
}

fn drivers(json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    let mut entries: Vec<&Entry> = machine
        .inventory
        .devices
        .iter()
        .flat_map(|device| machine.database.lookup(device))
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| a.id == b.id);
    if json {
        return print_json(&entries);
    }
    for entry in entries {
        println!(
            "{:24} {:22} {}",
            entry.id,
            entry.category_label(),
            entry.name
        );
    }
    Ok(())
}

fn firmware_command(json: bool) -> Result<(), String> {
    Platform::detect(&SystemRunner).require_freebsd("firmware detection")?;
    let needed = firmware::needed(&SystemRunner).map_err(|e| e.to_string())?;
    let reports = needed
        .iter()
        .map(|package| firmware::report(&SystemRunner, package).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if json {
        return print_json(&reports);
    }
    if reports.is_empty() {
        println!("FreeBSD's fwget knows of no firmware this hardware needs.");
    }
    for report in reports {
        let license = if report.licenses.is_empty() {
            "unstated".into()
        } else {
            report.licenses.join(", ")
        };
        println!(
            "{:44} {:12} {:12} licence {license}",
            report.package,
            label(&report.state),
            report.version.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

fn compatibility(id: Option<&str>, json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    let mut found = Vec::new();
    for device in &machine.inventory.devices {
        for assessment in machine.assess(device) {
            if id.is_none_or(|id| id == assessment.driver) {
                found.push((device.location.clone(), assessment));
            }
        }
    }
    if let Some(id) = id {
        machine.entry(id)?;
        if found.is_empty() {
            return Err(format!("no detected device matches {id}"));
        }
    }
    if json {
        let list: Vec<_> = found
            .iter()
            .map(|(device, a)| serde_json::json!({ "device": device, "assessment": a }))
            .collect();
        return print_json(&list);
    }
    for (device, assessment) in found {
        print_assessment(&device, &assessment);
    }
    Ok(())
}

fn print_assessment(device: &str, assessment: &Assessment) {
    println!("{} for {device}", assessment.driver);
    println!(
        "    verdict   {} ({})",
        assessment.verdict, assessment.reason
    );
    println!("    category  {}", assessment.category);
    println!(
        "    known     {}% of the checks have a definite answer",
        assessment.known
    );
    for check in &assessment.checks {
        let mark = match check.outcome {
            Outcome::Pass => "yes    ",
            Outcome::Fail => "NO     ",
            Outcome::Missing => "missing",
            Outcome::Unknown => "unknown",
        };
        println!("    {mark}   {}: {}", check.name, check.detail);
    }
    for evidence in &assessment.evidence {
        println!("    evidence  {evidence}");
    }
}

fn capabilities(json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    let found = capability::query(&machine.inventory, &SystemRunner, Path::new("/"));
    if json {
        return print_json(&found);
    }
    for capability in found {
        println!(
            "{:18} {:12} {}",
            capability.id,
            match capability.state {
                // The hardware is there, but nothing makes it usable.
                capability::CapabilityState::Present => "not working".to_owned(),
                state => label(&state),
            },
            capability.basis
        );
    }
    Ok(())
}

fn linuxkpi(interface: Option<&str>, json: bool) -> Result<(), String> {
    let kpi = KpiRegistry::builtin();
    let Some(name) = interface else {
        let areas = kpi.areas();
        if json {
            return print_json(&areas);
        }
        println!("{} (declared, unfinished)", kpi.release);
        for (area, (declared, unfinished)) in areas {
            println!("    {area:16} {declared:5} {unfinished:5}");
        }
        return Ok(());
    };
    let state = kpi.state(name);
    let advice = kpi.advice(name);
    if json {
        return print_json(&serde_json::json!({
            "interface": name, "state": state, "advice": advice, "release": kpi.release,
        }));
    }
    let text = match &state {
        meta_mdcf::linuxkpi::InterfaceState::Declared { header, area } => {
            format!("declared in {header} ({area}); what it does is not checked")
        }
        meta_mdcf::linuxkpi::InterfaceState::Unfinished { header, area } => {
            format!("unfinished, in {header} ({area})")
        }
        meta_mdcf::linuxkpi::InterfaceState::Missing => "not declared".to_owned(),
    };
    println!("{name}: {text}, LinuxKPI of {}", kpi.release);
    if let Some(advice) = advice {
        println!(
            "    {}: {} ({})",
            label(&advice.strategy),
            advice.detail,
            advice.reference
        );
    }
    Ok(())
}

fn predict_command(source: &Path, json: bool) -> Result<(), String> {
    let prediction = predict(source, &KpiRegistry::builtin())?;
    if json {
        return print_json(&prediction);
    }
    println!("verdict     {} ({})", prediction.verdict, prediction.reason);
    println!(
        "source      {} files, {} kernel interfaces, {}% declared by LinuxKPI",
        prediction.files, prediction.interfaces, prediction.coverage
    );
    if !prediction.device_ids.is_empty() {
        println!("devices     {}", prediction.device_ids.len());
    }
    if !prediction.architectures.is_empty() {
        let list: Vec<&str> = prediction
            .architectures
            .iter()
            .map(String::as_str)
            .collect();
        println!("mentions    {}", list.join(", "));
    }
    for (title, list) in [
        ("missing", &prediction.missing),
        ("unfinished", &prediction.unfinished),
    ] {
        println!("{title:11} {}", list.len());
        for item in list.iter().take(15) {
            let advice = item
                .advice
                .as_ref()
                .map(|a| format!("  {}", label(&a.strategy)))
                .unwrap_or_default();
            println!("    {:4}  {}{advice}", item.uses, item.name);
        }
    }
    for limit in &prediction.limits {
        println!("limit       {limit}");
    }
    Ok(())
}

#[derive(Serialize)]
struct Problem {
    subject: String,
    detail: String,
}

/// Everything that looks wrong, without guessing at causes.
fn problems(machine: &Machine) -> Vec<Problem> {
    let mut found = Vec::new();
    let mut add = |subject: String, detail: String| found.push(Problem { subject, detail });
    for device in machine
        .inventory
        .devices
        .iter()
        .filter(|d| d.driver.is_none())
    {
        // usbconfig does not report drivers, so their absence means nothing there.
        if device.bus != meta_eds_detect::Bus::Usb {
            add(device_line(device), "no driver is attached".into());
        }
    }
    for device in &machine.inventory.devices {
        for assessment in machine.assess(device) {
            let attached = assessment.checks.iter().any(|c| c.name == "attached");
            if attached && !assessment.missing.is_empty() {
                add(
                    assessment.driver.clone(),
                    format!("attached, but missing: {}", assessment.missing.join(", ")),
                );
            }
        }
    }
    match firmware::needed(&SystemRunner) {
        Ok(needed) => {
            for package in needed {
                if let Ok(report) = firmware::report(&SystemRunner, &package)
                    && report.state != firmware::FirmwareState::Installed
                {
                    add(package, format!("firmware is {}", label(&report.state)));
                }
            }
        }
        Err(error) => add("firmware".into(), error.to_string()),
    }
    use meta_common::CommandRunner;
    if let Ok(text) = SystemRunner.stdout("/sbin/dmesg", &[]) {
        for line in firmware::failures(&text) {
            add("kernel message".into(), line);
        }
    }
    for (bus, reason) in &machine.inventory.unavailable {
        add(format!("{bus:?} bus"), reason.clone());
    }
    found
}

fn diagnose(json: bool) -> Result<(), String> {
    let found = problems(&Machine::detect()?);
    if json {
        return print_json(&found);
    }
    if found.is_empty() {
        println!("No problems found.");
    }
    for problem in found {
        println!("{}\n    {}", problem.subject, problem.detail);
    }
    Ok(())
}

fn make_plan(
    machine: &Machine,
    operation: Operation,
    id: &str,
    accept_untested: bool,
) -> Result<Plan, String> {
    let entry = machine.entry(id)?;
    let assessment = machine.best(id);
    let dependents = lifecycle::dependents(&SystemRunner, entry).map_err(|e| e.to_string())?;
    let request = Request {
        operation,
        entry,
        assessment: assessment.as_ref(),
        accept_untested,
    };
    lifecycle::plan(&request, &machine.host, &dependents).map_err(|e| e.to_string())
}

fn show_plan(plan: &Plan, json: bool) -> Result<(), String> {
    if json {
        return print_json(plan);
    }
    println!("{} {}:", label(&plan.operation), plan.entry);
    for (index, step) in plan.steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step.description);
        println!("     {}", step.command.join(" "));
        match &step.undo {
            Some(undo) => println!("     undo: {}", undo.join(" ")),
            None => println!("     cannot be undone automatically"),
        }
    }
    for note in &plan.notes {
        println!("  note: {note}");
    }
    Ok(())
}

fn change(operation: Operation, request: &Change, json: bool) -> Result<(), String> {
    let machine = Machine::detect()?;
    let plan = make_plan(&machine, operation, &request.id, request.accept_untested)?;
    if !request.yes {
        show_plan(&plan, json)?;
        if !json {
            println!("Nothing was changed. Run again with --yes to proceed.");
        }
        return Ok(());
    }
    lifecycle::execute(&plan, &SystemRunner, &mut |step| {
        println!("==> {}", step.description)
    })
    .map_err(|e| e.to_string())?;
    for note in &plan.notes {
        println!("note: {note}");
    }
    Ok(())
}

fn verify(id: &str, json: bool) -> Result<ExitCode, String> {
    let machine = Machine::detect()?;
    let entry = machine.entry(id)?;
    let mut checks: Vec<(String, bool)> = Vec::new();
    for package in entry.packages.iter().chain(&entry.firmware) {
        checks.push((
            format!("{package} is installed"),
            machine.host.packages.contains(package),
        ));
    }
    for module in &entry.modules {
        checks.push((
            format!("{module} is loaded"),
            machine.host.is_loaded(module),
        ));
    }
    if let Some(driver) = &entry.driver {
        let attached = machine
            .inventory
            .devices
            .iter()
            .any(|d| d.driver_name() == Some(driver.as_str()));
        checks.push((format!("{driver} is attached to a device"), attached));
    }
    let good = checks.iter().all(|(_, ok)| *ok);
    if json {
        print_json(&serde_json::json!({ "entry": id, "ok": good, "checks": checks }))?;
    } else {
        for (text, ok) in &checks {
            println!("{}  {text}", if *ok { "ok    " } else { "FAILED" });
        }
        println!("Whether the hardware works has to be tried.");
    }
    Ok(if good {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_PROBLEM)
    })
}

/// Contains no serial numbers, addresses or names; bus enumeration has none.
fn report(config: &Path) -> Result<(), String> {
    let machine = Machine::detect()?;
    let devices: Vec<_> = machine
        .inventory
        .devices
        .iter()
        .map(
            |device| serde_json::json!({ "device": device, "assessments": machine.assess(device) }),
        )
        .collect();
    print_json(&serde_json::json!({
        "format": "meta-hardware-report-v1",
        "architecture": machine.host.arch,
        "osreldate": machine.host.osreldate,
        "policy": load_policy(config)?.map(|p| p.enabled),
        "devices": devices,
        "unavailable": machine.inventory.unavailable,
        "capabilities": capability::query(&machine.inventory, &SystemRunner, Path::new("/")),
        "problems": problems(&machine),
    }))
}

fn list(json: bool) -> Result<(), String> {
    let database = Database::builtin();
    if json {
        return print_json(&database.entries());
    }
    for entry in database.entries() {
        println!(
            "{:24} {:11} {:22} {}",
            entry.id,
            label(&entry.category),
            entry.category_label(),
            entry.name
        );
    }
    Ok(())
}

fn info(id: &str, json: bool) -> Result<(), String> {
    let database = Database::builtin();
    let entry = database
        .get(id)
        .ok_or_else(|| format!("no database entry {id:?}"))?;
    if json {
        return print_json(entry);
    }
    let tested = match entry.validation {
        Validation::Validated => "tested by META",
        Validation::Documented => "not tested by META",
    };
    println!("{}", entry.name);
    println!("  id           {}", entry.id);
    println!("  category     {}", label(&entry.category));
    println!("  support      {}, {tested}", entry.category_label());
    for (name, values) in [
        ("modules", &entry.modules),
        ("packages", &entry.packages),
        ("firmware", &entry.firmware),
        ("linuxkpi", &entry.linuxkpi),
        ("userspace", &entry.userspace),
        ("conflicts", &entry.conflicts),
        ("limitations", &entry.limitations),
    ] {
        if !values.is_empty() {
            println!("  {name:12} {}", values.join(", "));
        }
    }
    println!("  source       {}", entry.source);
    if let Some(notes) = &entry.notes {
        println!("  notes        {notes}");
    }
    for test in &entry.tested_on {
        println!(
            "  tested       {} on {} ({}): {}",
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

    #[test]
    fn command_line_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
