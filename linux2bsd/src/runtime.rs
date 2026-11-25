//! Discovery of Linux userlands and Linuxulator state.

use std::path::{Path, PathBuf};

use meta_common::CommandRunner;
use meta_common::platform::sysctl;
use serde::Serialize;

use crate::resolve::path_in_root;

/// FreeBSD's default `compat.linux.emul_path`.
pub const DEFAULT_EMUL_PATH: &str = "/compat/linux";

/// Other places a Linux userland may be installed, by convention or by the
/// META editions (see packages/editions).
const OTHER_ROOTS: &[&str] = &[
    "/compat/meta/debian",
    "/compat/meta/arch",
    "/compat/ubuntu",
    "/compat/debian",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Distribution {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Runtime {
    pub root: PathBuf,
    /// The kernel translates paths into this root for every Linux process.
    pub is_emul_path: bool,
    pub distribution: Option<Distribution>,
}

/// State of the kernel's Linux support.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Linuxulator {
    /// The `linux` ABI module is available for this architecture.
    pub loaded: bool,
    pub emul_path: PathBuf,
    pub osrelease: Option<String>,
}

impl Linuxulator {
    pub fn detect(runner: &dyn CommandRunner) -> Self {
        let emul_path = sysctl(runner, "compat.linux.emul_path");
        let osrelease = sysctl(runner, "compat.linux.osrelease");
        Self {
            // The compat.linux tree exists only while linux_common.ko is loaded.
            loaded: osrelease.is_some(),
            emul_path: PathBuf::from(emul_path.unwrap_or_else(|| DEFAULT_EMUL_PATH.to_owned())),
            osrelease,
        }
    }

    /// Emulated kernel version, such as `(5, 15, 0)`.
    pub fn kernel_version(&self) -> Option<(u32, u32, u32)> {
        let text = self.osrelease.as_deref()?;
        let mut numbers = text.split(['.', '-']).map(|part| part.parse().ok());
        Some((
            numbers.next()??,
            numbers.next()??,
            numbers.next().flatten().unwrap_or(0),
        ))
    }
}

/// Finds Linux userlands, the emulation path first. `prefix` is `/` except in tests.
pub fn discover(prefix: &Path, emul_path: &Path) -> Vec<Runtime> {
    let mut candidates = vec![emul_path];
    candidates.extend(
        OTHER_ROOTS
            .iter()
            .map(Path::new)
            .filter(|p| *p != emul_path),
    );
    candidates
        .into_iter()
        .filter_map(|path| {
            let root = prefix.join(path.strip_prefix("/").unwrap_or(path));
            if !looks_like_userland(&root) {
                return None;
            }
            let distribution = read_os_release(&root);
            Some(Runtime {
                root,
                is_emul_path: path == emul_path,
                distribution,
            })
        })
        .collect()
}

/// A root without any libraries is an empty mount point, not a runtime.
fn looks_like_userland(root: &Path) -> bool {
    ["/lib", "/lib64", "/usr/lib"].iter().any(|dir| {
        path_in_root(root, dir)
            .and_then(|path| path.read_dir().ok())
            .is_some_and(|mut entries| entries.next().is_some())
    })
}

fn read_os_release(root: &Path) -> Option<Distribution> {
    let path = path_in_root(root, "/etc/os-release")?;
    let text = std::fs::read_to_string(path).ok()?;
    let field = |key: &str| {
        text.lines()
            .filter_map(|line| line.split_once('='))
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.trim().trim_matches('"').to_owned())
    };
    let id = field("ID")?;
    let name = field("PRETTY_NAME").unwrap_or_else(|| id.clone());
    Some(Distribution { id, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;
    use std::fs;

    #[test]
    fn unloaded_linuxulator() {
        let state = Linuxulator::detect(&FakeRunner::new());
        assert!(!state.loaded);
        assert_eq!(state.emul_path, Path::new(DEFAULT_EMUL_PATH));
        assert_eq!(state.kernel_version(), None);
    }

    #[test]
    fn loaded_linuxulator() {
        let runner = FakeRunner::new()
            .with("/sbin/sysctl -n compat.linux.osrelease", "5.15.0\n")
            .with("/sbin/sysctl -n compat.linux.emul_path", "/compat/linux\n");
        let state = Linuxulator::detect(&runner);
        assert!(state.loaded);
        assert_eq!(state.kernel_version(), Some((5, 15, 0)));
    }

    #[test]
    fn discovers_populated_roots_only() {
        let dir = tempfile::tempdir().unwrap();
        let ubuntu = dir.path().join("compat/ubuntu");
        fs::create_dir_all(ubuntu.join("usr/lib")).unwrap();
        fs::write(ubuntu.join("usr/lib/libc.so.6"), b"").unwrap();
        fs::create_dir_all(ubuntu.join("etc")).unwrap();
        fs::write(
            ubuntu.join("etc/os-release"),
            "ID=ubuntu\nPRETTY_NAME=\"Ubuntu 22.04\"\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("compat/linux/usr/lib")).unwrap();
        let found = discover(dir.path(), Path::new("/compat/linux"));
        assert_eq!(found.len(), 1);
        assert!(!found[0].is_emul_path);
        assert_eq!(found[0].distribution.as_ref().unwrap().name, "Ubuntu 22.04");
    }
}
