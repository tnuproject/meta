//! Facts about the running system that driver decisions depend on.

use std::collections::BTreeSet;
use std::path::Path;

use meta_common::{CommandRunner, Platform};
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct HostFacts {
    pub arch: String,
    pub osreldate: Option<u32>,
    /// Names from kldstat(8): modules in files and in the kernel.
    pub loaded: BTreeSet<String>,
    /// Module files that could be loaded.
    pub available: BTreeSet<String>,
    /// Installed packages.
    pub packages: BTreeSet<String>,
}

const KLDSTAT: &str = "/sbin/kldstat";
pub const PKG: &str = "/usr/local/sbin/pkg";

impl HostFacts {
    pub fn collect(platform: &Platform, runner: &dyn CommandRunner) -> Self {
        let lines = |program: &str, args: &[&str]| {
            runner
                .stdout(program, args)
                .map(|text| text.lines().map(str::to_owned).collect::<Vec<_>>())
        };
        let mut available = BTreeSet::new();
        for directory in ["/boot/kernel", "/boot/kernel.META", "/boot/modules"] {
            available.extend(module_files(Path::new(directory)));
        }
        Self {
            arch: platform.arch.clone(),
            osreldate: platform.osreldate,
            loaded: lines(KLDSTAT, &["-v"])
                .map(|l| parse_kldstat(&l))
                .unwrap_or_default(),
            available,
            packages: lines(PKG, &["query", "%n"])
                .unwrap_or_default()
                .into_iter()
                .collect(),
        }
    }

    pub fn has_module(&self, name: &str) -> bool {
        self.loaded.contains(name) || self.available.contains(name)
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.loaded.contains(name)
    }
}

fn module_files(directory: &Path) -> impl Iterator<Item = String> {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".ko").map(str::to_owned))
}

/// Parses `kldstat -v`: file lines such as `1 52 0xffff... 23520f8 kernel (/boot/...)`
/// and indented module lines such as `  123 pci/em`.
fn parse_kldstat(lines: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [_, _, address, _, file, ..] if address.starts_with("0x") => {
                names.insert(file.trim_end_matches(".ko").to_owned());
            }
            [id, module] if id.bytes().all(|b| b.is_ascii_digit()) => {
                names.insert((*module).to_owned());
                if let Some((_, driver)) = module.rsplit_once('/') {
                    names.insert(driver.to_owned());
                }
            }
            _ => {}
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kldstat_files_and_modules() {
        let text = "Id Refs Address                Size Name\n \
                    1   23 0xffffffff80200000  23520f8 kernel (/boot/kernel/kernel)\n\
                    \tContains modules:\n\t\t Id Name\n\t\t201 pci/em\n\t\t 7 virtio_pci/vtnet\n \
                    2    1 0xffffffff82553000   628ef8 zfs.ko (/boot/kernel/zfs.ko)\n";
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let names = parse_kldstat(&lines);
        for expected in ["kernel", "zfs", "em", "vtnet", "pci/em"] {
            assert!(names.contains(expected), "{expected} in {names:?}");
        }
        assert!(!names.contains("Name"));
    }
}
