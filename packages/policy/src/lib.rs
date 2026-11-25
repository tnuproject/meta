//! Package ecosystem isolation and override policy.
//!
//! An executable form of docs/packaging. It decides; it does not install.

use serde::{Deserialize, Serialize};

/// Who owns a part of the filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ecosystem {
    /// pkg(8): FreeBSD and META packages.
    Native,
    Debian,
    Arch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub id: String,
    pub name: String,
    pub ecosystem: Ecosystem,
    /// Absolute path of the edition's userland on the host.
    pub root: String,
    pub package_manager: String,
    pub package_format: String,
    /// Host paths exposed inside the root, and whether they are writable there.
    #[serde(default)]
    pub shared: Vec<Shared>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shared {
    pub host: String,
    pub inside: String,
    pub writable: bool,
    pub reason: String,
}

/// Host directories a compatibility root may never contain, be, or write to.
const PROTECTED: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/libexec", "/rescue", "/root", "/sbin", "/usr",
    "/var/db",
];

impl Edition {
    pub fn builtin() -> Vec<Self> {
        [
            include_str!("../../editions/debian.toml"),
            include_str!("../../editions/arch.toml"),
        ]
        .into_iter()
        .map(|text| toml::from_str(text).expect("editions are validated by the test suite"))
        .collect()
    }
}

/// Splits an absolute path, resolving `.` and `..` lexically. `None` if relative.
fn components(path: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for part in path.strip_prefix('/')?.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(parts)
}

fn is_within(path: &str, directory: &str) -> bool {
    match (components(path), components(directory)) {
        (Some(path), Some(directory)) => path.starts_with(&directory),
        _ => false,
    }
}

/// Checks that edition roots are separate from the host and from each other.
pub fn check_isolation(editions: &[Edition]) -> Result<(), Vec<String>> {
    let mut problems = Vec::new();
    for edition in editions {
        let root = &edition.root;
        if components(root).is_none_or(|parts| parts.len() < 2) {
            problems.push(format!(
                "{}: root {root} must be an absolute subdirectory",
                edition.id
            ));
        }
        for protected in PROTECTED {
            if is_within(root, protected) || is_within(protected, root) {
                problems.push(format!("{}: root {root} overlaps {protected}", edition.id));
            }
        }
        for shared in edition.shared.iter().filter(|shared| shared.writable) {
            if PROTECTED
                .iter()
                .any(|p| is_within(&shared.host, p) || is_within(p, &shared.host))
            {
                problems.push(format!(
                    "{}: {} must not be writable",
                    edition.id, shared.host
                ));
            }
        }
    }
    for (index, a) in editions.iter().enumerate() {
        for b in &editions[index + 1..] {
            if is_within(&a.root, &b.root) || is_within(&b.root, &a.root) {
                problems.push(format!("{} and {} share a root", a.id, b.id));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// The ecosystem that owns a host path.
pub fn owner_of(path: &str, editions: &[Edition]) -> Ecosystem {
    editions
        .iter()
        .find(|edition| is_within(path, &edition.root))
        .map_or(Ecosystem::Native, |edition| edition.ecosystem.clone())
}

/// Whether a package manager may write to a host path.
pub fn may_write(writer: &Ecosystem, path: &str, editions: &[Edition]) -> bool {
    components(path).is_some() && owner_of(path, editions) == *writer
}

/// A reason an installation was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Violation {
    SignatureInvalid,
    ChecksumMismatch,
    UntrustedRepository,
    KnownVulnerability,
    MissingDependency,
    DependencyConflict,
    AbiMismatch,
    /// The package was built for a newer FreeBSD in the same major version.
    NewerOsVersion,
    /// A file belongs to another package of the same ecosystem.
    FileConflict,
    /// A file belongs to another ecosystem.
    EcosystemBoundary,
    /// The package is marked as incompatible in the compatibility database.
    CompatibilityAdvisory,
    /// The same version is already installed.
    AlreadyInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

impl Violation {
    /// Whether `--force=<reason>` may override this violation at all.
    pub fn overridable(self) -> bool {
        match self {
            Violation::NewerOsVersion
            | Violation::CompatibilityAdvisory
            | Violation::AlreadyInstalled => true,
            Violation::SignatureInvalid
            | Violation::ChecksumMismatch
            | Violation::UntrustedRepository
            | Violation::KnownVulnerability
            | Violation::MissingDependency
            | Violation::DependencyConflict
            | Violation::AbiMismatch
            | Violation::FileConflict
            | Violation::EcosystemBoundary => false,
        }
    }
}

/// Decides whether to continue. Every violation must be overridable and named
/// explicitly; there is no override for "everything".
pub fn evaluate(violations: &[Violation], forced: &[Violation]) -> Verdict {
    let covered = violations
        .iter()
        .all(|v| v.overridable() && forced.contains(v));
    if covered {
        Verdict::Allow
    } else {
        Verdict::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Violation] = &[
        Violation::SignatureInvalid,
        Violation::ChecksumMismatch,
        Violation::UntrustedRepository,
        Violation::KnownVulnerability,
        Violation::MissingDependency,
        Violation::DependencyConflict,
        Violation::AbiMismatch,
        Violation::NewerOsVersion,
        Violation::FileConflict,
        Violation::EcosystemBoundary,
        Violation::CompatibilityAdvisory,
        Violation::AlreadyInstalled,
    ];

    fn edition(id: &str, root: &str) -> Edition {
        Edition {
            id: id.into(),
            name: id.into(),
            ecosystem: Ecosystem::Debian,
            root: root.into(),
            package_manager: "apt".into(),
            package_format: "deb".into(),
            shared: Vec::new(),
        }
    }

    #[test]
    fn builtin_editions_are_isolated() {
        check_isolation(&Edition::builtin()).unwrap();
    }

    #[test]
    fn overlapping_roots_are_rejected() {
        for root in [
            "/",
            "/usr",
            "/usr/local/x",
            "/var",
            "/compat",
            "relative",
            "/compat/../usr",
        ] {
            let editions = [edition("a", root), edition("b", "/compat/meta/b")];
            assert!(check_isolation(&editions).is_err(), "{root}");
        }
        let nested = [edition("a", "/compat/x"), edition("b", "/compat/x/y")];
        assert!(check_isolation(&nested).is_err());
    }

    #[test]
    fn writable_system_paths_are_rejected() {
        let mut edition = edition("a", "/compat/a");
        edition.shared.push(Shared {
            host: "/usr/local".into(),
            inside: "/usr/local".into(),
            writable: true,
            reason: "test".into(),
        });
        assert!(check_isolation(&[edition]).is_err());
    }

    #[test]
    fn ownership_boundaries() {
        let editions = Edition::builtin();
        let debian = editions.iter().find(|e| e.id == "debian").unwrap();
        let inside = format!("{}/usr/bin/x", debian.root);
        assert!(may_write(&Ecosystem::Debian, &inside, &editions));
        assert!(!may_write(&Ecosystem::Native, &inside, &editions));
        assert!(!may_write(&Ecosystem::Arch, &inside, &editions));
        assert!(!may_write(&Ecosystem::Debian, "/usr/bin/x", &editions));
        let escape = format!("{}/../../../usr/bin/x", debian.root);
        assert!(!may_write(&Ecosystem::Debian, &escape, &editions));
        // A sibling that merely shares a name prefix is not inside the root.
        assert!(!may_write(
            &Ecosystem::Debian,
            &format!("{}-old/x", debian.root),
            &editions
        ));
        assert!(!may_write(&Ecosystem::Native, "relative", &editions));
    }

    #[test]
    fn integrity_and_safety_are_never_overridable() {
        for violation in ALL {
            let expected = if violation.overridable() {
                Verdict::Allow
            } else {
                Verdict::Deny
            };
            assert_eq!(evaluate(&[*violation], ALL), expected, "{violation:?}");
            assert_eq!(evaluate(&[*violation], &[]), Verdict::Deny);
        }
        assert_eq!(evaluate(&[], &[]), Verdict::Allow);
    }

    #[test]
    fn overrides_must_name_every_violation() {
        let both = [Violation::NewerOsVersion, Violation::CompatibilityAdvisory];
        assert_eq!(evaluate(&both, &both[..1]), Verdict::Deny);
        assert_eq!(evaluate(&both, &both), Verdict::Allow);
        let mixed = [Violation::NewerOsVersion, Violation::ChecksumMismatch];
        assert_eq!(evaluate(&mixed, &mixed), Verdict::Deny);
    }
}
