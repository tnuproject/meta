//! Life cycle of a root.

use std::fs;
use std::path::Path;

use meta_common::{CommandError, CommandRunner};
use meta_pkg_policy::{Edition, check_isolation};

use crate::bootstrap;
use crate::mounts::{self, MOUNT, UMOUNT};

const ZFS: &str = "/sbin/zfs";
const JAIL: &str = "/usr/sbin/jail";
const JLS: &str = "/usr/sbin/jls";
pub const JEXEC: &str = "/usr/sbin/jexec";

#[derive(Debug, thiserror::Error)]
pub enum RootError {
    #[error("unknown edition {0:?}")]
    UnknownEdition(String),
    #[error("{0} already exists")]
    Exists(String),
    #[error("{0} has not been created")]
    Missing(String),
    #[error("filesystems are still mounted below {root}: {}", points.join(", "))]
    StillMounted { root: String, points: Vec<String> },
    #[error("{0} is not a ZFS dataset of its own, so it cannot be removed safely")]
    NotDataset(String),
    #[error("the root filesystem is not ZFS")]
    NoPool,
    #[error("edition definitions overlap: {0}")]
    Isolation(String),
    #[error("Linux support is not enabled; run: sysrc linux_enable=YES && service linux start")]
    NoLinux,
    #[error("no snapshot named {0:?}")]
    NoSnapshot(String),
    #[error("{0}")]
    Bootstrap(String),
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Command(#[from] CommandError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Absent,
    Stopped,
    Running,
}

pub struct Root<'a> {
    pub edition: Edition,
    runner: &'a dyn CommandRunner,
}

impl std::fmt::Debug for Root<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Root")
            .field("edition", &self.edition)
            .finish()
    }
}

impl<'a> Root<'a> {
    pub fn all(runner: &'a dyn CommandRunner) -> Result<Vec<Self>, RootError> {
        let editions = Edition::builtin();
        check_isolation(&editions).map_err(|p| RootError::Isolation(p.join("; ")))?;
        Ok(editions
            .into_iter()
            .map(|edition| Self { edition, runner })
            .collect())
    }

    pub fn open(id: &str, runner: &'a dyn CommandRunner) -> Result<Self, RootError> {
        Self::all(runner)?
            .into_iter()
            .find(|root| root.edition.id == id)
            .ok_or_else(|| RootError::UnknownEdition(id.to_owned()))
    }

    pub fn path(&self) -> &str {
        &self.edition.root
    }

    pub fn jail_name(&self) -> String {
        format!("meta_{}", self.edition.id)
    }

    pub fn state(&self) -> State {
        if !Path::new(self.path()).join("etc").exists() {
            return State::Absent;
        }
        match self.runner.run(JLS, &["-j", &self.jail_name(), "jid"]) {
            Ok(output) if output.success() => State::Running,
            _ => State::Stopped,
        }
    }

    /// The dataset mounted exactly at the root.
    fn dataset(&self) -> Result<String, RootError> {
        let not_dataset = || RootError::NotDataset(self.path().to_owned());
        let output = self.runner.stdout(
            ZFS,
            &["list", "-H", "-t", "filesystem", "-o", "name,mountpoint"],
        )?;
        output
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(_, mountpoint)| *mountpoint == self.path())
            .map(|(name, _)| name.to_owned())
            .ok_or_else(not_dataset)
    }

    /// Name for a new dataset, in the pool of the running system.
    fn new_dataset(&self) -> Result<String, RootError> {
        let pool = meta_common::platform::root_pool(self.runner).ok_or(RootError::NoPool)?;
        Ok(format!("{pool}/meta/compat/{}", self.edition.id))
    }

    /// Creates the root from the distribution's signed archive.
    pub fn create(&self) -> Result<(), RootError> {
        if self.state() != State::Absent {
            return Err(RootError::Exists(self.path().to_owned()));
        }
        if meta_common::platform::sysctl(self.runner, "compat.linux.osrelease").is_none() {
            return Err(RootError::NoLinux);
        }
        // Everything that can be verified is verified before a dataset exists.
        let work = std::env::temp_dir().join(format!("meta-compat.{}", std::process::id()));
        fs::create_dir(&work).map_err(|source| RootError::Io {
            path: work.display().to_string(),
            source,
        })?;
        let mut created = false;
        let result = self.create_in(&work, &mut created);
        let _ = fs::remove_dir_all(&work);
        // Leaves nothing half made, but never destroys a dataset that was there
        // before. A failure here is reported by the next create.
        if result.is_err() && created {
            let _ = self.stop();
            if let Ok(dataset) = self.dataset() {
                let _ = self.runner.run(ZFS, &["destroy", "-r", &dataset]);
            }
        }
        result
    }

    fn create_in(&self, work: &Path, created: &mut bool) -> Result<(), RootError> {
        let material = bootstrap::fetch(self.runner, &self.edition, work)?;
        let dataset = self.new_dataset()?;
        let mountpoint = format!("mountpoint={}", self.path());
        self.runner
            .stdout(ZFS, &["create", "-p", "-o", &mountpoint, &dataset])?;
        *created = true;
        bootstrap::install(self.runner, &self.edition, &material)?;
        bootstrap::protect(&self.edition)?;
        // Bootstrapping may leave filesystems behind; start from a known state.
        self.unmount_all()?;
        self.start()?;
        bootstrap::finish(self.runner, &self.edition, &self.jail_name())
    }

    pub fn start(&self) -> Result<(), RootError> {
        match self.state() {
            State::Absent => return Err(RootError::Missing(self.path().to_owned())),
            State::Running => return Ok(()),
            State::Stopped => {}
        }
        mounts::define_ruleset(self.runner)?;
        let mounted = mounts::current(self.runner, self.path())?;
        for mount in mounts::wanted(&self.edition) {
            let target = format!("{}{}", self.path(), mount.inside);
            // A shared directory may belong to a service that is not installed.
            let missing = mount.kind == "nullfs" && !Path::new(&mount.source).is_dir();
            if missing || mounted.contains(&target) {
                continue;
            }
            // Directories below /dev belong to devfs and cannot be created.
            if !mount.inside.starts_with("/dev/") {
                fs::create_dir_all(&target).map_err(|source| RootError::Io {
                    path: target.clone(),
                    source,
                })?;
            }
            let mut args = vec!["-t", mount.kind];
            if let Some(options) = mount.options {
                args.extend(["-o", options]);
            }
            args.extend([mount.source.as_str(), &target]);
            self.runner.stdout(MOUNT, &args)?;
        }
        self.share_resolver()?;
        let name = format!("name={}", self.jail_name());
        let path = format!("path={}", self.path());
        let hostname = format!("host.hostname=meta-{}", self.edition.id);
        self.runner.stdout(
            JAIL,
            &[
                "-c",
                &name,
                &path,
                &hostname,
                "ip4=inherit",
                "ip6=inherit",
                "linux=new",
                "linux.osname=Linux",
                "enforce_statfs=1",
                "children.max=0",
                "persist",
            ],
        )?;
        Ok(())
    }

    /// The jail uses the host's network, so it uses the host's name servers.
    fn share_resolver(&self) -> Result<(), RootError> {
        let Ok(contents) = fs::read("/etc/resolv.conf") else {
            return Ok(());
        };
        let path = Path::new(self.path()).join("etc/resolv.conf");
        // Distributions often ship a link that points outside /etc.
        let _ = fs::remove_file(&path);
        fs::write(&path, contents).map_err(|source| RootError::Io {
            path: path.display().to_string(),
            source,
        })
    }

    pub fn stop(&self) -> Result<(), RootError> {
        if self.state() == State::Running {
            self.runner.stdout(JAIL, &["-r", &self.jail_name()])?;
        }
        self.unmount_all()
    }

    fn unmount_all(&self) -> Result<(), RootError> {
        for point in mounts::current(self.runner, self.path())? {
            self.runner.stdout(UMOUNT, &[&point])?;
        }
        let points = mounts::current(self.runner, self.path())?;
        if points.is_empty() {
            Ok(())
        } else {
            Err(RootError::StillMounted {
                root: self.path().to_owned(),
                points,
            })
        }
    }

    /// Takes a snapshot and returns its name.
    pub fn snapshot(&self, label: &str) -> Result<String, RootError> {
        let name = format!("{}@meta-{label}", self.dataset()?);
        self.runner.stdout(ZFS, &["snapshot", &name])?;
        Ok(name)
    }

    pub fn snapshots(&self) -> Result<Vec<String>, RootError> {
        let dataset = self.dataset()?;
        let output = self.runner.stdout(
            ZFS,
            &[
                "list", "-H", "-t", "snapshot", "-d", "1", "-o", "name", "-s", "creation", &dataset,
            ],
        )?;
        Ok(output
            .lines()
            .filter_map(|line| line.split_once('@').map(|(_, name)| name.to_owned()))
            .collect())
    }

    /// Returns the root to a snapshot, discarding everything after it.
    pub fn rollback(&self, snapshot: &str) -> Result<(), RootError> {
        if !self.snapshots()?.iter().any(|name| name == snapshot) {
            return Err(RootError::NoSnapshot(snapshot.to_owned()));
        }
        self.stop()?;
        let name = format!("{}@{snapshot}", self.dataset()?);
        self.runner.stdout(ZFS, &["rollback", "-r", &name])?;
        Ok(())
    }

    /// Updates the distribution after taking a snapshot.
    pub fn update(&self, label: &str) -> Result<String, RootError> {
        let snapshot = self.snapshot(label)?;
        self.start()?;
        bootstrap::update(self.runner, &self.edition, &self.jail_name())?;
        Ok(snapshot)
    }

    /// Destroys the root.
    ///
    /// Only a dataset is ever destroyed, and only once nothing is mounted below
    /// it, so this cannot follow a mount into the user's files.
    pub fn remove(&self) -> Result<(), RootError> {
        if self.state() == State::Absent {
            return Err(RootError::Missing(self.path().to_owned()));
        }
        let dataset = self.dataset()?;
        self.stop()?;
        // A removed jail keeps its root until its last network connections have
        // timed out, which takes up to a minute. Destroying an unmounted dataset
        // is all or nothing, so trying again is safe.
        for _ in 0..30 {
            if self
                .runner
                .run(ZFS, &["destroy", "-r", &dataset])?
                .success()
            {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        self.runner.stdout(ZFS, &["destroy", "-r", &dataset])?;
        Ok(())
    }

    /// Arguments for jexec(8) that run `command` as `user` in `directory`.
    pub fn exec_arguments(&self, user: &str, directory: &str, command: &[String]) -> Vec<String> {
        let mut args: Vec<String> = ["-u", user].map(str::to_owned).to_vec();
        args.push(self.jail_name());
        args.extend(["/usr/bin/env", "-i"].map(str::to_owned));
        args.push("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".to_owned());
        for name in [
            "HOME",
            "TERM",
            "LANG",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
        ] {
            if let Ok(value) = std::env::var(name) {
                args.push(format!("{name}={value}"));
            }
        }
        // The directory may not exist inside; starting in / is then correct.
        let script = "cd \"$1\" 2>/dev/null || cd /; shift; exec \"$@\"";
        args.extend(["/bin/sh", "-c", script, "sh", directory].map(str::to_owned));
        args.extend(command.iter().cloned());
        args
    }

    /// Path of a host file inside the root, if it is visible there.
    pub fn inside(&self, host_path: &str) -> Option<String> {
        if let Some(rest) = host_path.strip_prefix(self.path())
            && rest.starts_with('/')
        {
            return Some(rest.to_owned());
        }
        self.edition.shared.iter().find_map(|shared| {
            let rest = host_path.strip_prefix(&shared.host)?;
            rest.starts_with('/')
                .then(|| format!("{}{rest}", shared.inside))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_common::command::FakeRunner;

    const LIST: &str = "/sbin/zfs list -H -t filesystem -o name,mountpoint";

    #[test]
    fn dataset_must_be_mounted_exactly_at_the_root() {
        let runner = FakeRunner::new().with(LIST, "zroot\tnone\nzroot/x\t/compat/meta\n");
        let root = Root::open("debian", &runner).unwrap();
        assert!(matches!(root.dataset(), Err(RootError::NotDataset(_))));
        let runner =
            FakeRunner::new().with(LIST, "zroot/meta/compat/debian\t/compat/meta/debian\n");
        let root = Root::open("debian", &runner).unwrap();
        assert_eq!(root.dataset().unwrap(), "zroot/meta/compat/debian");
    }

    #[test]
    fn new_dataset_uses_the_running_pool() {
        let runner = FakeRunner::new().with("/sbin/mount -p", "tank/ROOT/be\t/\tzfs\trw\t0 0\n");
        let root = Root::open("arch", &runner).unwrap();
        assert_eq!(root.new_dataset().unwrap(), "tank/meta/compat/arch");
        let runner = FakeRunner::new().with("/sbin/mount -p", "/dev/ada0p2 / ufs rw 1 1\n");
        assert!(matches!(
            Root::open("arch", &runner).unwrap().new_dataset(),
            Err(RootError::NoPool)
        ));
    }

    #[test]
    fn host_paths_are_translated() {
        let runner = FakeRunner::new();
        let root = Root::open("debian", &runner).unwrap();
        assert_eq!(
            root.inside("/compat/meta/debian/usr/bin/x").as_deref(),
            Some("/usr/bin/x")
        );
        assert_eq!(root.inside("/home/a/x").as_deref(), Some("/home/a/x"));
        assert_eq!(root.inside("/opt/x"), None);
        assert_eq!(root.inside("/compat/meta/debian-old/x"), None);
        assert_eq!(root.inside("/homeless/x"), None);
    }

    #[test]
    fn commands_are_not_interpreted_by_a_shell() {
        let runner = FakeRunner::new();
        let root = Root::open("debian", &runner).unwrap();
        let command = vec!["echo".to_owned(), "$(reboot); x".to_owned()];
        let args = root.exec_arguments("alice", "/home/alice", &command);
        assert_eq!(&args[..3], ["-u", "alice", "meta_debian"]);
        assert_eq!(args.last().unwrap(), "$(reboot); x");
    }

    #[test]
    fn unknown_editions_are_refused() {
        let runner = FakeRunner::new();
        assert!(matches!(
            Root::open("../x", &runner),
            Err(RootError::UnknownEdition(_))
        ));
    }
}
