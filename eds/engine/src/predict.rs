//! MPDC: predicting whether a Linux driver's source could be ported.
//!
//! Reads source text and compares what it uses with the LinuxKPI registry. It never
//! builds, loads or changes anything, and cannot prove that a driver works.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use meta_model::{Evidence, Graph, NodeKind, Relation, Source};
use serde::Serialize;

use crate::evaluate::Verdict;
use crate::linuxkpi::{Advice, InterfaceState, KpiRegistry};

/// Functions of FreeBSD's own kernel that Linux drivers call by the same name.
const FREEBSD: &[&str] = &[
    "bool",
    "abs",
    "bcmp",
    "max",
    "memchr",
    "memcmp",
    "memcpy",
    "memmove",
    "memset",
    "min",
    "offsetof",
    "roundup",
    "rounddown",
    "snprintf",
    "sprintf",
    "sscanf",
    "strcat",
    "strchr",
    "strcmp",
    "strcpy",
    "strdup",
    "strlcat",
    "strlcpy",
    "strlen",
    "strncmp",
    "strncpy",
    "strnlen",
    "strrchr",
    "strsep",
    "strstr",
    "vsnprintf",
    "printf",
];
const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: u64 = 8 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterfaceUse {
    pub name: String,
    pub uses: usize,
    pub state: InterfaceState,
    pub advice: Option<Advice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Prediction {
    pub verdict: Verdict,
    pub reason: String,
    pub files: usize,
    /// Kernel functions the source calls and does not define.
    pub interfaces: usize,
    pub missing: Vec<InterfaceUse>,
    pub unfinished: Vec<InterfaceUse>,
    pub missing_headers: Vec<String>,
    pub firmware: Vec<String>,
    pub device_ids: Vec<(u16, u16)>,
    /// Architectures the source names in preprocessor conditions.
    pub architectures: BTreeSet<String>,
    /// Share of used interfaces that LinuxKPI declares without reservation.
    pub coverage: u8,
    pub limits: Vec<&'static str>,
    pub graph: Graph,
}

/// Analyzes the driver source below `directory`.
pub fn predict(directory: &Path, kpi: &KpiRegistry) -> Result<Prediction, String> {
    let mut source = SourceFacts::default();
    read_directory(directory, &mut source)?;
    if source.files == 0 {
        return Err(format!("{} contains no C source", directory.display()));
    }
    let used: BTreeMap<&String, usize> = source
        .calls
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            !source.own.contains(name) && !KEYWORDS.contains(&name) && !FREEBSD.contains(&name)
        })
        .map(|(name, count)| (name, *count))
        .collect();
    let mut missing = Vec::new();
    let mut unfinished = Vec::new();
    let mut declared = 0usize;
    for (name, uses) in &used {
        let state = kpi.state(name);
        let item = InterfaceUse {
            name: (*name).clone(),
            uses: *uses,
            advice: kpi.advice(name).cloned(),
            state: state.clone(),
        };
        match state {
            InterfaceState::Declared { .. } => declared += 1,
            InterfaceState::Unfinished { .. } => unfinished.push(item),
            InterfaceState::Missing => missing.push(item),
        }
    }
    for list in [&mut missing, &mut unfinished] {
        list.sort_by(|a, b| b.uses.cmp(&a.uses).then_with(|| a.name.cmp(&b.name)));
    }
    let missing_headers: Vec<String> = source
        .headers
        .iter()
        .filter(|h| kpi.header(h).is_none())
        .cloned()
        .collect();
    let coverage = u8::try_from(declared * 100 / used.len().max(1)).unwrap_or(100);
    let (verdict, reason) = decide(&missing, &unfinished);
    let graph = graph(directory, &source, &missing, &unfinished);
    Ok(Prediction {
        verdict,
        reason,
        files: source.files,
        interfaces: used.len(),
        missing,
        unfinished,
        missing_headers,
        firmware: source.firmware.into_iter().collect(),
        device_ids: source.device_ids.into_iter().collect(),
        architectures: source.architectures,
        coverage,
        limits: vec![
            "names are compared, not behaviour",
            "conditional code and code that is not built are included",
            "some functions of FreeBSD, and those from the driver's parent directories count as missing",
            "nothing was compiled or run",
        ],
        graph,
    })
}

/// Never better than experimental: a prediction is not a test.
fn decide(missing: &[InterfaceUse], unfinished: &[InterfaceUse]) -> (Verdict, String) {
    use crate::linuxkpi::Strategy;
    let blocked = |list: &[InterfaceUse]| {
        list.iter()
            .filter(|i| {
                i.advice
                    .as_ref()
                    .is_some_and(|a| a.strategy == Strategy::UpstreamContribution)
            })
            .count()
    };
    let upstream = blocked(missing) + blocked(unfinished);
    if upstream > 0 {
        (
            Verdict::Incompatible,
            format!("{upstream} interfaces need work in FreeBSD before the driver can run"),
        )
    } else if missing.is_empty() && unfinished.is_empty() {
        (
            Verdict::Experimental,
            "every interface is declared; behaviour is unknown".into(),
        )
    } else {
        (
            Verdict::InsufficientEvidence,
            format!(
                "{} interfaces are missing and {} unfinished; which matter needs reading the source",
                missing.len(),
                unfinished.len()
            ),
        )
    }
}

#[derive(Debug, Default)]
struct SourceFacts {
    files: usize,
    headers: BTreeSet<String>,
    calls: BTreeMap<String, usize>,
    own: BTreeSet<String>,
    firmware: BTreeSet<String>,
    device_ids: BTreeSet<(u16, u16)>,
    architectures: BTreeSet<String>,
}

const KEYWORDS: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "return",
    "sizeof",
    "typeof",
    "defined",
    "do",
    "else",
    "case",
    "alignof",
    "offsetof",
    "asm",
    "static_assert",
    "va_start",
    "va_end",
    "va_arg",
    "va_copy",
];

fn read_directory(directory: &Path, facts: &mut SourceFacts) -> Result<(), String> {
    let mut pending = vec![directory.to_owned()];
    while let Some(current) = pending.pop() {
        let entries =
            std::fs::read_dir(&current).map_err(|e| format!("{}: {e}", current.display()))?;
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            // Links are not followed, so a tree cannot lead elsewhere.
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path.extension().is_some_and(|e| e == "c" || e == "h")
                && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_SIZE)
            {
                if facts.files >= MAX_FILES {
                    return Err("too many files".into());
                }
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                facts.files += 1;
                scan(&String::from_utf8_lossy(&bytes), facts);
            }
        }
    }
    Ok(())
}

/// Collects facts from one file.
fn scan(text: &str, facts: &mut SourceFacts) {
    let code = strip(text);
    for line in code.lines() {
        let line = line.trim_start();
        let Some(directive) = line.strip_prefix('#').map(str::trim_start) else {
            continue;
        };
        if let Some(rest) = directive.strip_prefix("include") {
            let rest = rest.trim();
            if let Some(header) = rest.strip_prefix('<').and_then(|r| r.split('>').next()) {
                let linux = [
                    "linux/", "net/", "asm/", "crypto/", "acpi/", "drm/", "video/",
                ];
                if linux.iter().any(|p| header.starts_with(p)) {
                    facts.headers.insert(header.to_owned());
                }
            }
        } else if let Some(rest) = directive.strip_prefix("define") {
            let name: String = rest.trim_start().chars().take_while(|c| word(*c)).collect();
            facts.own.insert(name);
        } else if directive.starts_with("if") {
            for arch in ["CONFIG_X86", "CONFIG_ARM64", "CONFIG_PPC", "CONFIG_RISCV"] {
                if directive.contains(arch) {
                    facts
                        .architectures
                        .insert(arch.trim_start_matches("CONFIG_").to_lowercase());
                }
            }
        }
    }
    // Strings were removed from `code`; firmware names need them.
    for (index, _) in text.match_indices("MODULE_FIRMWARE(") {
        let rest = &text[index + "MODULE_FIRMWARE(".len()..];
        let argument = rest.trim_start();
        if let Some(name) = argument.strip_prefix('"').and_then(|r| r.split('"').next()) {
            facts.firmware.insert(name.to_owned());
        } else {
            let name: String = argument.chars().take_while(|c| word(*c)).collect();
            if !name.is_empty() {
                facts.firmware.insert(name);
            }
        }
    }
    for macro_name in ["PCI_DEVICE(", "USB_DEVICE("] {
        for (index, _) in code.match_indices(macro_name) {
            let arguments: Vec<&str> = code[index + macro_name.len()..]
                .split(')')
                .next()
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .collect();
            if let [vendor, device] = arguments.as_slice()
                && let (Some(vendor), Some(device)) = (hex(vendor), hex(device))
            {
                facts.device_ids.insert((vendor, device));
            }
        }
    }
    calls(&code, facts);
}

fn hex(text: &str) -> Option<u16> {
    u16::from_str_radix(text.strip_prefix("0x")?, 16).ok()
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Finds identifiers followed by a parenthesis. One at the start of a line outside
/// any function, or after a type, is a definition; the rest are calls.
fn calls(code: &str, facts: &mut SourceFacts) {
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match byte {
            b'{' => depth += 1,
            b'}' => depth = (depth - 1).max(0),
            _ if byte.is_ascii_lowercase() || byte == b'_' => {
                let start = index;
                while index < bytes.len() && word(bytes[index] as char) {
                    index += 1;
                }
                let name = &code[start..index];
                let mut after = index;
                while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
                    after += 1;
                }
                let before = code[..start].trim_end().as_bytes().last().copied();
                let member = matches!(before, Some(b'.')) || code[..start].ends_with("->");
                if bytes.get(after) == Some(&b'(') && !member && name.len() > 2 {
                    if depth == 0 {
                        facts.own.insert(name.to_owned());
                    } else {
                        *facts.calls.entry(name.to_owned()).or_default() += 1;
                    }
                }
                continue;
            }
            _ if byte.is_ascii_uppercase() || byte.is_ascii_digit() => {
                while index < bytes.len() && word(bytes[index] as char) {
                    index += 1;
                }
                continue;
            }
            _ => {}
        }
        index += 1;
    }
}

/// Replaces comments and string contents, keeping line breaks.
fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                    }
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '"' | '\'' => {
                out.push(c);
                let mut escaped = false;
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        out.push('\n');
                        break;
                    }
                    if !escaped && inner == c {
                        break;
                    }
                    escaped = inner == '\\' && !escaped;
                }
                out.push(c);
            }
            // Non-ASCII text only occurs in comments and strings.
            c if c.is_ascii() => out.push(c),
            _ => out.push(' '),
        }
    }
    out
}

fn graph(
    directory: &Path,
    source: &SourceFacts,
    missing: &[InterfaceUse],
    unfinished: &[InterfaceUse],
) -> Graph {
    let mut graph = Graph::new();
    let name = directory
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let driver = format!("driver:{name}");
    graph.node(&driver, NodeKind::Driver, &name);
    let evidence = |detail: String| vec![Evidence::new(Source::StaticAnalysis, detail)];
    let mut link =
        |id: String, kind, label: String, from: Option<&str>, relation, detail: String| {
            graph.node(&id, kind, label);
            let (from, to) = from.map_or((driver.as_str(), id.as_str()), |from| {
                (from, driver.as_str())
            });
            // Both ends were just added.
            let _ = graph.edge(from, to, relation, evidence(detail));
        };
    for (vendor, device) in &source.device_ids {
        let id = format!("device:{vendor:04x}:{device:04x}");
        let label = format!("{vendor:04x}:{device:04x}");
        link(
            id.clone(),
            NodeKind::Device,
            label,
            Some(&id),
            Relation::DrivenBy,
            "device table".into(),
        );
    }
    for item in missing.iter().chain(unfinished) {
        let state = match item.state {
            InterfaceState::Unfinished { .. } => "unfinished in LinuxKPI",
            _ => "not in LinuxKPI",
        };
        link(
            format!("linuxkpi:{}", item.name),
            NodeKind::LinuxKpi,
            item.name.clone(),
            None,
            Relation::Lacks,
            format!("used {} times, {state}", item.uses),
        );
    }
    for firmware in &source.firmware {
        link(
            format!("firmware:{firmware}"),
            NodeKind::Firmware,
            firmware.clone(),
            None,
            Relation::Requires,
            "MODULE_FIRMWARE".into(),
        );
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: &str) -> Prediction {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.c"), code).unwrap();
        predict(dir.path(), &KpiRegistry::builtin()).unwrap()
    }

    const CODE: &str = r#"
#include <linux/slab.h>
#include <linux/nonexistent_header.h>
#define HELPER(x) (x)
/* iommu_map(1) in a comment */
static const struct pci_device_id ids[] = { { PCI_DEVICE(0x17cb, 0x1103) }, };
MODULE_FIRMWARE("ath11k/WCN6855/hw2.0/amss.bin");
static int own_function(int a)
{
	return a;
}
static int probe(struct pci_dev *p)
{
	void *m = kmalloc(1, GFP_KERNEL);
	printk("iommu_map(x) in a string");
	own_function(1);
	p->ops->callback(1);
	if (mhi_sync_power_up(m))
		return 1;
	totally_unknown_call(m);
	return 0;
}
"#;

    #[test]
    fn uses_are_found_and_classified() {
        let p = run(CODE);
        let names: Vec<&str> = p.missing.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"totally_unknown_call"), "{names:?}");
        for wrong in [
            "iommu_map",
            "own_function",
            "callback",
            "probe",
            "kmalloc",
            "if",
        ] {
            assert!(!names.contains(&wrong), "{wrong} in {names:?}");
        }
        assert!(p.unfinished.iter().any(|i| i.name == "mhi_sync_power_up"));
        assert_eq!(p.device_ids, [(0x17cb, 0x1103)]);
        assert_eq!(p.firmware, ["ath11k/WCN6855/hw2.0/amss.bin"]);
        assert_eq!(p.missing_headers, ["linux/nonexistent_header.h"]);
    }

    #[test]
    fn needed_upstream_work_makes_a_driver_incompatible() {
        let p = run(CODE);
        assert_eq!(p.verdict, Verdict::Incompatible);
        assert!(p.unfinished[0].advice.is_some());
        assert!(
            p.graph
                .edges()
                .iter()
                .any(|e| e.relation == Relation::DrivenBy)
        );
    }

    #[test]
    fn a_prediction_never_confirms() {
        let p = run("int f(void)\n{\n\tkfree(kmalloc(1, 2));\n\treturn 0;\n}\n");
        assert_eq!(p.verdict, Verdict::Experimental);
        assert_eq!(p.coverage, 100);
        assert!(!p.limits.is_empty());
    }

    #[test]
    fn unterminated_input_does_not_hang_or_panic() {
        for code in [
            "/* open",
            "\"open",
            "'",
            "int f() { /",
            "#define",
            "#include <",
            "\\",
            "é(é(",
        ] {
            let _ = run(code);
        }
    }

    #[test]
    fn empty_directories_are_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(predict(dir.path(), &KpiRegistry::builtin()).is_err());
    }
}
