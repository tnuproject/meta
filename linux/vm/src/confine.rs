//! Running bhyve without privileges.
//!
//! bhyve emulates the devices a guest talks to, so it is what a hostile guest
//! attacks. It normally runs as root. Here it runs as the machine's owner, in a jail
//! whose root holds nothing but the base libraries, the machine's own disk and its
//! shared directories. devfs(8) rules make only this machine's devices visible, and
//! give them to the owner.
//!
//! This also makes bhyve's restricted network usable: in FreeBSD 15.1 the helper
//! enters Capsicum before it looks up the user to change to, which fails for root
//! (usr.sbin/bhyve/slirp/slirp-helper.c, corrected upstream since).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use meta_common::CommandRunner;
use meta_compat::mounts;

use crate::bhyve::FIRMWARE;
use crate::config::{Network, VmConfig, VmError};

const DEVFS: &str = "/sbin/devfs";
const JAIL: &str = "/usr/sbin/jail";
pub const JEXEC: &str = "/usr/sbin/jexec";
const CHOWN: &str = "/usr/sbin/chown";
const PW: &str = "/usr/sbin/pw";
const PWD_MKDB: &str = "/usr/sbin/pwd_mkdb";
const BASE: &str = "/var/run/meta-linux";

/// What bhyve, its network helper and the ACPI compiler it runs are made of.
const PROGRAMS: &[&str] = &[
    "/bin",
    "/lib",
    "/libexec",
    "/usr/lib",
    "/usr/libexec",
    "/usr/sbin",
    "/usr/local/lib",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    source: String,
    inside: String,
    options: &'static str,
}

pub struct Confinement<'a> {
    runner: &'a dyn CommandRunner,
    config: &'a VmConfig,
    /// Directory with the machine's disk.
    data: PathBuf,
    root: PathBuf,
}

impl std::fmt::Debug for Confinement<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Confinement")
            .field("root", &self.root)
            .finish()
    }
}

impl<'a> Confinement<'a> {
    pub fn new(runner: &'a dyn CommandRunner, config: &'a VmConfig, data: PathBuf) -> Self {
        Self {
            runner,
            config,
            data,
            root: Path::new(BASE).join(&config.name),
        }
    }

    pub fn jail_name(&self) -> String {
        format!("meta-vm-{}", self.config.name)
    }

    /// Its own devfs(8) ruleset. The port is as unique to the machine as a ruleset
    /// has to be.
    fn ruleset(&self) -> String {
        self.config.ssh_port.to_string()
    }

    /// Rules that show only this machine's devices.
    fn rules(&self) -> Vec<Vec<String>> {
        let vm = self.config.bhyve_name();
        let user = self.config.owner_uid.to_string();
        let mut rules = vec![vec!["hide".to_owned()]];
        let mut show = |path: &str, owned: bool| {
            let mut rule: Vec<String> = ["path", path, "unhide"].map(str::to_owned).to_vec();
            if owned {
                rule.extend(["user", &user, "mode", "0600"].map(str::to_owned));
            }
            rules.push(rule);
        };
        for path in ["null", "zero", "random", "urandom", "vmm", "vmm.io"] {
            show(path, false);
        }
        show("vmmctl", true);
        show(&format!("vmm/{vm}"), true);
        show(&format!("vmm.io/{vm}.*"), true);
        rules
    }

    fn mounts(&self) -> Vec<Mount> {
        let mut all: Vec<Mount> = PROGRAMS
            .iter()
            .map(|path| Mount {
                source: (*path).to_owned(),
                inside: (*path).to_owned(),
                options: "ro,nosuid",
            })
            .collect();
        all.push(Mount {
            source: self.data.display().to_string(),
            inside: "/vm".into(),
            options: "nosuid,noexec",
        });
        all.extend(self.config.shares.iter().map(|share| Mount {
            source: share.host.clone(),
            inside: format!("/share/{}", share.tag),
            options: if share.read_only {
                "ro,nosuid,noexec"
            } else {
                "nosuid,noexec"
            },
        }));
        all
    }

    fn inside(&self, path: &str) -> PathBuf {
        self.root.join(path.trim_start_matches('/'))
    }

    /// Builds the root and starts the jail.
    pub fn set_up(&self) -> Result<(), VmError> {
        self.tear_down()?;
        self.build()
    }

    fn build(&self) -> Result<(), VmError> {
        let make = |path: &Path| {
            fs::create_dir_all(path).map_err(|source| VmError::Io {
                path: path.display().to_string(),
                source,
            })
        };
        let copy = |from: &str, inside: &str| {
            fs::copy(from, self.inside(inside))
                .map(drop)
                .map_err(|source| VmError::Io {
                    path: from.to_owned(),
                    source,
                })
        };
        for path in ["/dev", "/tmp", "/etc", "/var/run"] {
            make(&self.inside(path))?;
        }
        // bhyve compiles ACPI tables in /tmp.
        let tmp = self.inside("/tmp").display().to_string();
        self.runner
            .stdout(CHOWN, &[&self.config.owner_uid.to_string(), &tmp])?;
        copy(FIRMWARE, "/firmware.fd")?;
        copy("/var/run/ld-elf.so.hints", "/var/run/ld-elf.so.hints")?;
        if self.config.network == Network::Open && Path::new("/etc/resolv.conf").exists() {
            copy("/etc/resolv.conf", "/etc/resolv.conf")?;
        }
        self.write_users()?;
        for mount in self.mounts() {
            let target = self.inside(&mount.inside);
            make(&target)?;
            let target = target.display().to_string();
            self.runner.stdout(
                mounts::MOUNT,
                &["-t", "nullfs", "-o", mount.options, &mount.source, &target],
            )?;
        }
        let ruleset = self.ruleset();
        self.runner
            .stdout(DEVFS, &["rule", "-s", &ruleset, "delset"])?;
        for rule in self.rules() {
            let mut args = vec!["rule", "-s", &ruleset, "add"];
            args.extend(rule.iter().map(String::as_str));
            self.runner.stdout(DEVFS, &args)?;
        }
        let dev = self.inside("/dev").display().to_string();
        let option = format!("ruleset={ruleset}");
        self.runner.stdout(
            mounts::MOUNT,
            &["-t", "devfs", "-o", &option, "devfs", &dev],
        )?;
        let (name, path) = (
            format!("name={}", self.jail_name()),
            format!("path={}", self.root.display()),
        );
        self.runner.stdout(
            JAIL,
            &[
                "-c",
                &name,
                &path,
                "ip4=inherit",
                "ip6=disable",
                "allow.vmm",
                "enforce_statfs=2",
                "children.max=0",
                "persist",
            ],
        )?;
        Ok(())
    }

    /// The users bhyve's file server looks up. It refuses requests of guest users that
    /// are not here, so only root and the owner can use a share. Linux attaches as
    /// nobody first unless told otherwise.
    fn write_users(&self) -> Result<(), VmError> {
        let line = self
            .runner
            .stdout(PW, &["usershow", "-n", &self.config.owner])?;
        let fields: Vec<&str> = line.trim().split(':').collect();
        let number = |index: usize| fields.get(index).and_then(|f| f.parse::<u32>().ok());
        let (Some(uid), Some(gid)) = (number(2), number(3)) else {
            return Err(VmError::Guest(format!(
                "{} has no user entry",
                self.config.owner
            )));
        };
        if uid != self.config.owner_uid {
            return Err(VmError::Guest(format!(
                "{} has another number now",
                self.config.owner
            )));
        }
        let etc = self.inside("/etc");
        let write = |name: &str, text: String| {
            let path = etc.join(name);
            fs::write(&path, text).map_err(|source| VmError::Io {
                path: path.display().to_string(),
                source,
            })?;
            Ok::<String, VmError>(path.display().to_string())
        };
        write("group", "wheel:*:0:\nnobody:*:65534:\n".into())?;
        let owner = &self.config.owner;
        let users = write(
            "master.passwd",
            format!(
                "root:*:0:0::0:0::/:/nonexistent\nnobody:*:65534:65534::0:0::/:/nonexistent\n{owner}:*:{uid}:{gid}::0:0::/:/nonexistent\n"
            ),
        )?;
        let etc = etc.display().to_string();
        self.runner.stdout(PWD_MKDB, &["-d", &etc, &users])?;
        Ok(())
    }

    /// Arguments for jexec(8) that run a program of the jail as the owner.
    pub fn command(&self, program: &str, arguments: &[String]) -> Vec<String> {
        let mut all: Vec<String> = ["-u", &self.config.owner, &self.jail_name(), program]
            .map(str::to_owned)
            .to_vec();
        all.extend(arguments.iter().cloned());
        all
    }

    /// Removes the jail and its mounts. A filesystem that cannot be unmounted stops
    /// this before anything is deleted, because a deletion would reach into it.
    pub fn tear_down(&self) -> Result<(), VmError> {
        let _ = self.runner.run(JAIL, &["-r", &self.jail_name()]);
        let root = self.root.display().to_string();
        for attempt in 0..20 {
            let points = mounts::current(self.runner, &root)?;
            if points.is_empty() {
                break;
            }
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500));
            }
            for point in points {
                self.runner.run(mounts::UMOUNT, &[&point])?;
            }
        }
        let left = mounts::current(self.runner, &root)?;
        if !left.is_empty() {
            return Err(VmError::Guest(format!(
                "{} cannot be unmounted",
                left.join(", ")
            )));
        }
        match fs::remove_dir_all(&self.root) {
            Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
                Err(VmError::Io { path: root, source })
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::sample;
    use meta_common::command::FakeRunner;

    #[test]
    fn only_the_machines_own_devices_are_visible() {
        let runner = FakeRunner::new();
        let config = sample();
        let confinement = Confinement::new(&runner, &config, "/d/data".into());
        let rules: Vec<String> = confinement.rules().iter().map(|r| r.join(" ")).collect();
        assert_eq!(rules[0], "hide");
        assert!(rules.contains(&"path vmm/meta-debian unhide user 1001 mode 0600".to_owned()));
        for path in ["mem", "pci", "bpf", "zfs", "vmm/*"] {
            assert!(
                !rules.iter().any(|r| r.contains(&format!("path {path} "))),
                "{path}"
            );
        }
    }

    #[test]
    fn the_host_is_not_visible() {
        let runner = FakeRunner::new();
        let mut config = sample();
        config.shares[0].read_only = true;
        let confinement = Confinement::new(&runner, &config, "/d/data".into());
        let mounts = confinement.mounts();
        assert!(mounts.iter().all(|m| m.options.contains("nosuid")));
        for mount in &mounts {
            let writable = !mount.options.starts_with("ro");
            assert_eq!(writable, mount.source == "/d/data", "{mount:?}");
        }
        for path in ["/", "/etc", "/home", "/root", "/var", "/usr"] {
            assert!(mounts.iter().all(|m| m.source != path), "{path}");
        }
    }

    #[test]
    fn programs_run_as_the_owner() {
        let runner = FakeRunner::new();
        let config = sample();
        let confinement = Confinement::new(&runner, &config, "/d/data".into());
        let args = confinement.command("/usr/sbin/bhyve", &["-x".into()]);
        assert_eq!(args.join(" "), "-u alice meta-vm-debian /usr/sbin/bhyve -x");
    }
}
