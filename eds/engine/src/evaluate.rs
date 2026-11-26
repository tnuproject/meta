//! Compatibility verdicts.
//!
//! A verdict is derived from checks by fixed rules, and every check says what it
//! looked at. There is no score that could outvote a failed check.

use meta_eds_db::{Entry, Match, Support, Validation};
use meta_eds_detect::Device;
use meta_model::{Evidence, Source};
use serde::Serialize;

use crate::host::HostFacts;
use crate::linuxkpi::{InterfaceState, KpiRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Incompatible,
    InsufficientEvidence,
    Experimental,
    Probable,
    Confirmed,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Confirmed => "CONFIRMED",
            Verdict::Probable => "PROBABLE",
            Verdict::Experimental => "EXPERIMENTAL",
            Verdict::Incompatible => "INCOMPATIBLE",
            Verdict::InsufficientEvidence => "INSUFFICIENT_EVIDENCE",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Pass,
    Fail,
    /// Could be fixed by installing something.
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub outcome: Outcome,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Assessment {
    pub driver: String,
    pub name: String,
    pub category: &'static str,
    pub verdict: Verdict,
    pub reason: String,
    pub checks: Vec<Check>,
    /// Packages, modules and firmware that would have to be added.
    pub missing: Vec<String>,
    pub evidence: Vec<Evidence>,
    /// Share of checks with a definite answer. Says how much is known, not how
    /// likely the driver is to work.
    pub known: u8,
}

/// Assesses every candidate for a device, best verdict first.
pub fn assess_device(
    device: &Device,
    candidates: &[&Entry],
    host: &HostFacts,
    kpi: &KpiRegistry,
) -> Vec<Assessment> {
    let mut found: Vec<Assessment> = candidates
        .iter()
        .map(|entry| assess(device, entry, host, kpi))
        .collect();
    found.sort_by(|a, b| {
        b.verdict
            .cmp(&a.verdict)
            .then_with(|| a.driver.cmp(&b.driver))
    });
    found
}

pub fn assess(device: &Device, entry: &Entry, host: &HostFacts, kpi: &KpiRegistry) -> Assessment {
    let mut checks = Vec::new();
    let mut missing = Vec::new();
    let mut check = |name, outcome, detail: String| {
        checks.push(Check {
            name,
            outcome,
            detail,
        })
    };

    let matching = entry
        .matches
        .iter()
        .filter(|m| m.applies_to(device))
        .max_by_key(|m| exactness(m));
    let exact = matching.is_some_and(|m| exactness(m) == 2);
    match matching.map(exactness) {
        Some(2) => check("hardware id", Outcome::Pass, "this device is listed".into()),
        Some(_) => check(
            "hardware id",
            Outcome::Unknown,
            "only the vendor or kind of device is listed, not this device".into(),
        ),
        None => check(
            "hardware id",
            Outcome::Fail,
            "this device is not listed".into(),
        ),
    }

    if entry.arch.is_empty() || entry.arch.contains(&host.arch) {
        check("architecture", Outcome::Pass, host.arch.clone());
    } else {
        let detail = format!("needs {}, this is {}", entry.arch.join(" or "), host.arch);
        check("architecture", Outcome::Fail, detail);
    }

    match (entry.osreldate, host.osreldate) {
        (None, _) => check(
            "kernel version",
            Outcome::Unknown,
            "no range is recorded".into(),
        ),
        (Some(_), None) => check("kernel version", Outcome::Unknown, "kernel unknown".into()),
        (Some(range), Some(found)) if found < range.min || found > range.max => check(
            "kernel version",
            Outcome::Fail,
            format!("{found} is outside {}-{}", range.min, range.max),
        ),
        (Some(_), Some(found)) => check("kernel version", Outcome::Pass, found.to_string()),
    }

    for module in &entry.modules {
        if host.is_loaded(module) {
            check(
                "kernel module",
                Outcome::Pass,
                format!("{module} is loaded"),
            );
        } else if host.has_module(module) {
            check(
                "kernel module",
                Outcome::Pass,
                format!("{module} is available"),
            );
        } else {
            missing.push(format!("module {module}"));
            check(
                "kernel module",
                Outcome::Missing,
                format!("{module} is not installed"),
            );
        }
    }
    for package in &entry.packages {
        if host.packages.contains(package) {
            check("package", Outcome::Pass, format!("{package} is installed"));
        } else {
            missing.push(format!("package {package}"));
            check(
                "package",
                Outcome::Missing,
                format!("{package} is not installed"),
            );
        }
    }
    for firmware in &entry.firmware {
        if host.packages.contains(firmware) || host.has_module(firmware) {
            check(
                "firmware",
                Outcome::Pass,
                format!("{firmware} is installed"),
            );
        } else {
            missing.push(format!("firmware {firmware}"));
            check(
                "firmware",
                Outcome::Missing,
                format!("{firmware} is not installed"),
            );
        }
    }
    let mut unfinished = false;
    for interface in &entry.linuxkpi {
        match kpi.state(interface) {
            InterfaceState::Declared { .. } => check(
                "LinuxKPI",
                // Declared says nothing about behaviour.
                Outcome::Unknown,
                format!("{interface} is declared in {}", kpi.release),
            ),
            InterfaceState::Unfinished { .. } => {
                unfinished = true;
                check(
                    "LinuxKPI",
                    Outcome::Unknown,
                    format!("{interface} is unfinished"),
                );
            }
            InterfaceState::Missing => {
                check(
                    "LinuxKPI",
                    Outcome::Fail,
                    format!("{interface} does not exist"),
                );
            }
        }
    }
    for conflict in &entry.conflicts {
        if host.is_loaded(conflict) {
            check("conflict", Outcome::Fail, format!("{conflict} is loaded"));
        }
    }
    let attached = entry.driver.is_some()
        && entry.driver.as_deref() == device.driver_name()
        && device.has_specific_driver();
    if attached {
        let driver = device.driver.as_deref().unwrap_or_default();
        check(
            "attached",
            Outcome::Pass,
            format!("the kernel attached {driver}"),
        );
    }

    let (verdict, reason) = decide(entry, &checks, exact, attached, unfinished, host);
    let definite = checks
        .iter()
        .filter(|c| c.outcome != Outcome::Unknown)
        .count();
    let known = u8::try_from(definite * 100 / checks.len().max(1)).unwrap_or(100);
    Assessment {
        driver: entry.id.clone(),
        name: entry.name.clone(),
        category: entry.category_label(),
        verdict,
        reason,
        missing,
        evidence: evidence(entry, attached, device),
        checks,
        known,
    }
}

/// 2: lists this device, 1: lists its vendor or kind.
fn exactness(m: &Match) -> u8 {
    if m.vendor.is_some() && !m.devices.is_empty() {
        2
    } else {
        1
    }
}

fn decide(
    entry: &Entry,
    checks: &[Check],
    exact: bool,
    attached: bool,
    unfinished: bool,
    host: &HostFacts,
) -> (Verdict, String) {
    if entry.support == Support::Unsupported {
        return (Verdict::Incompatible, "known not to work".into());
    }
    if let Some(failed) = checks.iter().find(|c| c.outcome == Outcome::Fail) {
        return (
            Verdict::Incompatible,
            format!("{}: {}", failed.name, failed.detail),
        );
    }
    // A test only confirms the major release it was made on.
    let tested = entry.validation == Validation::Validated
        && entry
            .tested_on
            .iter()
            .any(|t| same_major(&t.release, host.osreldate));
    if tested && (exact || attached) {
        return (
            Verdict::Confirmed,
            "tested by META on this major release".into(),
        );
    }
    if unfinished {
        return (
            Verdict::Experimental,
            "depends on unfinished LinuxKPI interfaces".into(),
        );
    }
    match entry.support {
        Support::Experimental => (Verdict::Experimental, "recorded as experimental".into()),
        Support::PartiallySupported => (
            Verdict::Experimental,
            "recorded as partially supported".into(),
        ),
        _ if attached => (
            Verdict::Probable,
            "the kernel attached this driver; not tested by META".into(),
        ),
        _ if exact => (
            Verdict::Probable,
            "upstream lists this device; not tested by META".into(),
        ),
        _ => (
            Verdict::InsufficientEvidence,
            "only the vendor or kind of device is known to be covered".into(),
        ),
    }
}

fn same_major(release: &str, osreldate: Option<u32>) -> bool {
    let major = release
        .split('.')
        .next()
        .and_then(|m| m.parse::<u32>().ok());
    // __FreeBSD_version is MMmmRxx: 1501000 is 15.1.
    major.is_some() && major == osreldate.map(|v| v / 100_000)
}

fn evidence(entry: &Entry, attached: bool, device: &Device) -> Vec<Evidence> {
    let mut evidence =
        vec![Evidence::new(Source::Documentation, entry.name.clone()).at(entry.source.clone())];
    for test in &entry.tested_on {
        let detail = format!("{} on {}: {}", test.release, test.machine, test.result);
        evidence.push(Evidence::new(Source::Test, detail).at(test.date.clone()));
    }
    if attached {
        let detail = format!(
            "{} is attached",
            device.driver.as_deref().unwrap_or_default()
        );
        evidence.push(Evidence::new(Source::Observation, detail).at(device.location.clone()));
    }
    evidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_eds_db::Database;
    use meta_eds_detect::Bus;

    fn device(vendor: u16, id: u16, class: u32, driver: Option<&str>) -> Device {
        Device {
            vendor,
            device: id,
            class,
            driver: driver.map(str::to_owned),
            ..Device::new(Bus::Pci, "pci0:0:1:0")
        }
    }

    fn host() -> HostFacts {
        HostFacts {
            arch: "amd64".into(),
            osreldate: Some(1_501_000),
            ..HostFacts::default()
        }
    }

    fn entry(extra: &str) -> Entry {
        let text = format!(
            "schema = 2\n[[entry]]\nid = \"x\"\nname = \"X\"\ncategory = \"wireless\"\n\
             support = \"linuxkpi\"\nvalidation = \"documented\"\nsource = \"https://example.org\"\n\
             driver = \"xx\"\n{extra}\n[[entry.matches]]\nbus = \"pci\"\nvendor = 4660\ndevices = [1]\n"
        );
        Database::parse(&[("t", &text)]).unwrap().entries()[0].clone()
    }

    fn verdict(entry: &Entry, device: &Device, host: &HostFacts) -> Verdict {
        assess(device, entry, host, &KpiRegistry::builtin()).verdict
    }

    #[test]
    fn listed_devices_are_probable_until_tested() {
        let listed = device(0x1234, 1, 0x028000, None);
        assert_eq!(verdict(&entry(""), &listed, &host()), Verdict::Probable);
        let other = device(0x1234, 2, 0x028000, None);
        assert_eq!(verdict(&entry(""), &other, &host()), Verdict::Incompatible);
    }

    #[test]
    fn failed_checks_decide_whatever_else_is_known() {
        let listed = device(0x1234, 1, 0x028000, Some("xx0"));
        let arch = entry("arch = [\"aarch64\"]");
        assert_eq!(verdict(&arch, &listed, &host()), Verdict::Incompatible);
        let kernel = entry("osreldate = { min = 1600000, max = 1699999 }");
        assert_eq!(verdict(&kernel, &listed, &host()), Verdict::Incompatible);
        let mut busy = host();
        busy.loaded.insert("other".into());
        assert_eq!(
            verdict(&entry("conflicts = [\"other\"]"), &listed, &busy),
            Verdict::Incompatible
        );
        assert_eq!(
            verdict(&entry("linuxkpi = [\"iommu_map\"]"), &listed, &host()),
            Verdict::Incompatible
        );
    }

    #[test]
    fn compiling_against_linuxkpi_is_not_compatibility() {
        let listed = device(0x1234, 1, 0x028000, None);
        let unfinished = entry("linuxkpi = [\"mhi_sync_power_up\"]");
        let result = assess(&listed, &unfinished, &host(), &KpiRegistry::builtin());
        assert_eq!(result.verdict, Verdict::Experimental);
        // Declared interfaces add no certainty either.
        let declared = assess(
            &listed,
            &entry("linuxkpi = [\"kmalloc\"]"),
            &host(),
            &KpiRegistry::builtin(),
        );
        assert!(
            declared
                .checks
                .iter()
                .any(|c| c.name == "LinuxKPI" && c.outcome == Outcome::Unknown)
        );
    }

    #[test]
    fn missing_parts_are_named_without_deciding() {
        let listed = device(0x1234, 1, 0x028000, None);
        let needs = entry("packages = [\"p\"]\nmodules = [\"m\"]\nfirmware = [\"f\"]");
        let result = assess(&listed, &needs, &host(), &KpiRegistry::builtin());
        assert_eq!(result.missing, ["module m", "package p", "firmware f"]);
        assert_eq!(result.verdict, Verdict::Probable);
    }

    #[test]
    fn tests_confirm_only_their_major_release() {
        let tested = "[[entry.tested_on]]\nrelease = \"15.1-RELEASE\"\nmachine = \"m\"\n\
                      date = \"d\"\nresult = \"r\"\n";
        let text = entry("").id;
        assert_eq!(text, "x");
        let text = format!(
            "schema = 2\n[[entry]]\nid = \"x\"\nname = \"X\"\ncategory = \"gpu\"\nsupport = \"native\"\n\
             validation = \"validated\"\nsource = \"https://example.org\"\n{tested}\
             [[entry.matches]]\nbus = \"pci\"\nvendor = 4660\ndevices = [1]\n"
        );
        let entry = Database::parse(&[("t", &text)]).unwrap().entries()[0].clone();
        let listed = device(0x1234, 1, 0x030000, None);
        assert_eq!(verdict(&entry, &listed, &host()), Verdict::Confirmed);
        let mut later = host();
        later.osreldate = Some(1_600_000);
        assert_eq!(verdict(&entry, &listed, &later), Verdict::Probable);
    }

    #[test]
    fn vendor_wide_entries_need_more_than_the_vendor() {
        let database = Database::builtin();
        let unattached = device(0x8086, 0xabcd, 0x028000, None);
        let found = assess_device(
            &unattached,
            &database.lookup(&unattached),
            &host(),
            &KpiRegistry::builtin(),
        );
        assert!(!found.is_empty());
        assert!(
            found
                .iter()
                .all(|a| a.verdict == Verdict::InsufficientEvidence)
        );
        let attached = device(0x8086, 0xabcd, 0x028000, Some("iwlwifi0"));
        let found = assess_device(
            &attached,
            &database.lookup(&attached),
            &host(),
            &KpiRegistry::builtin(),
        );
        assert_eq!(found[0].verdict, Verdict::Probable);
    }
}
