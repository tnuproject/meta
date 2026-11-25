//! Read-only disk detection.

use meta_common::{CommandRunner, Platform};

const GEOM: &str = "/sbin/geom";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub name: String,
    pub size_bytes: u64,
    pub description: String,
    /// Open for writing somewhere, for example a mounted or imported disk.
    pub in_use: bool,
}

impl Disk {
    pub fn summary(&self) -> String {
        let gib = self.size_bytes / (1 << 30);
        let description = if self.description.is_empty() {
            "no description"
        } else {
            &self.description
        };
        let use_note = if self.in_use { ", in use" } else { "" };
        format!("{}  {gib} GiB  {description}{use_note}", self.name)
    }
}

/// Lists disks, or explains why none can be listed.
pub fn detect(platform: &Platform, runner: &dyn CommandRunner) -> (Vec<Disk>, Option<String>) {
    if let Err(reason) = platform.require_freebsd("disk detection") {
        return (Vec::new(), Some(reason));
    }
    match runner.stdout(GEOM, &["disk", "list"]) {
        Ok(text) => (parse_geom(&text), None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}

/// Smallest disk offered as an installation target. Also excludes optical media.
const MINIMUM_SIZE: u64 = 8 << 30;

pub fn parse_geom(text: &str) -> Vec<Disk> {
    let mut disks: Vec<Disk> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix("Geom name: ") {
            disks.push(Disk {
                name: name.to_owned(),
                size_bytes: 0,
                description: String::new(),
                in_use: false,
            });
        } else if let (Some(disk), Some((key, value))) = (disks.last_mut(), line.split_once(':')) {
            let value = value.trim();
            match key {
                "Mediasize" => {
                    disk.size_bytes = value
                        .split_whitespace()
                        .next()
                        .and_then(|bytes| bytes.parse().ok())
                        .unwrap_or(0);
                }
                // Access counts such as `r3w3e7`.
                "Mode" => disk.in_use = value != "r0w0e0",
                "descr" => disk.description = value.to_owned(),
                _ => {}
            }
        }
    }
    disks.retain(|disk| disk.size_bytes >= MINIMUM_SIZE);
    disks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_output() {
        let disks = parse_geom(include_str!("../fixtures/geom-disk-list-qemu-15.1.txt"));
        assert_eq!(disks.len(), 1, "the optical drive is not a target");
        assert_eq!(disks[0].name, "vtbd0");
        assert_eq!(disks[0].size_bytes, 42_949_672_960);
        assert!(disks[0].in_use);
    }
}
