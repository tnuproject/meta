//! Life cycle of machines.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use meta_common::CommandRunner;
use meta_common::platform::{root_pool, sysctl};
use serde::Serialize;

use crate::bhyve::{self, BHYVE, BHYVECTL, Exit, FIRMWARE, FIRMWARE_VARS};
use crate::config::{Cgroups, Forward, Network, Share, VmConfig, VmError, valid_name, valid_user};
use crate::confine::{Confinement, JEXEC};
use crate::guest::Connection;
use crate::images::{Images, QEMU_IMG};
use crate::seed::{self, Keys};
use crate::{audit, share};

const KLDLOAD: &str = "/sbin/kldload";
const ZFS: &str = "/sbin/zfs";
const MAKEFS: &str = "/usr/sbin/makefs";
const SSH_KEYGEN: &str = "/usr/bin/ssh-keygen";
const DAEMON: &str = "/usr/sbin/daemon";
const KILL: &str = "/bin/kill";
const CHOWN: &str = "/usr/sbin/chown";
const PW: &str = "/usr/sbin/pw";
const LIBSLIRP: &str = "/usr/local/lib/libslirp.so";
const FIRST_PORT: u16 = 22010;
const CONSOLE_LIMIT: u64 = 8 << 20;
/// Restarting this often within [`RESTART_WINDOW`] means that something is wrong.
const MAX_RESTARTS: usize = 5;
const RESTART_WINDOW: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VmState {
    Stopped,
    Running,
}

#[derive(Debug, Clone, Serialize)]
pub struct VmStatus {
    pub name: String,
    pub image: String,
    pub state: VmState,
    pub owner: String,
    pub cpus: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub network: Network,
    pub cgroups: Cgroups,
    pub ssh_port: u16,
    /// How it ended the last time.
    pub last_exit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub ok: bool,
    pub check: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub name: String,
    pub image: String,
    pub cpus: u32,
    pub memory_mib: u32,
    pub disk_gib: u32,
    pub network: Network,
    pub cgroups: Cgroups,
    pub owner: String,
    pub forwards: Vec<Forward>,
    /// Further directories; a private one is always created.
    pub shares: Vec<Share>,
}

/// Changes to a stopped machine.
#[derive(Debug, Clone, Default)]
pub struct Change {
    pub cpus: Option<u32>,
    pub memory_mib: Option<u32>,
    pub disk_gib: Option<u32>,
    pub network: Option<Network>,
    pub forwards: Option<Vec<Forward>>,
}

pub struct Manager<'a> {
    runner: &'a dyn CommandRunner,
    /// Machines and images.
    pub root: PathBuf,
    pub share_root: String,
}

impl std::fmt::Debug for Manager<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Manager").field("root", &self.root).finish()
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> VmError + '_ {
    move |source| VmError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn utf8(path: &Path) -> Result<String, VmError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| VmError::Guest(format!("{} is not UTF-8", path.display())))
}

impl<'a> Manager<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self {
            runner,
            root: PathBuf::from("/var/db/meta-linux"),
            share_root: "/var/meta-linux/share".into(),
        }
    }

    fn vms(&self) -> PathBuf {
        self.root.join("vms")
    }

    pub fn directory(&self, name: &str) -> PathBuf {
        self.vms().join(name)
    }

    pub fn load(&self, name: &str) -> Result<VmConfig, VmError> {
        if !valid_name(name) {
            return Err(VmError::Missing(name.to_owned()));
        }
        let path = self.directory(name).join("config.toml");
        let text = fs::read_to_string(&path).map_err(|_| VmError::Missing(name.to_owned()))?;
        let config: VmConfig = toml::from_str(&text)
            .map_err(|e| VmError::Invalid(vec![format!("{}: {e}", path.display())]))?;
        config.validate()?;
        if config.name != name {
            return Err(VmError::Invalid(vec![format!(
                "{} names another machine",
                path.display()
            )]));
        }
        Ok(config)
    }

    pub fn list(&self) -> Vec<VmConfig> {
        let Ok(entries) = fs::read_dir(self.vms()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        names
            .iter()
            .filter_map(|name| self.load(name).ok())
            .collect()
    }

    pub fn state(&self, config: &VmConfig) -> VmState {
        if Path::new("/dev/vmm").join(config.bhyve_name()).exists() {
            VmState::Running
        } else {
            VmState::Stopped
        }
    }

    pub fn status(&self, config: &VmConfig) -> VmStatus {
        VmStatus {
            name: config.name.clone(),
            image: config.image.clone(),
            state: self.state(config),
            owner: config.owner.clone(),
            cpus: config.cpus,
            memory_mib: config.memory_mib,
            disk_gib: config.disk_gib,
            network: config.network,
            cgroups: config.cgroups,
            ssh_port: config.ssh_port,
            last_exit: fs::read_to_string(self.directory(&config.name).join("last-exit"))
                .ok()
                .map(|text| text.trim().to_owned()),
        }
    }

    pub fn connection(&self, config: &VmConfig) -> Connection {
        Connection::new(&self.directory(&config.name), config)
    }

    /// Whether bhyve can run guests here, and what is missing when it cannot.
    pub fn support(&self) -> Vec<Finding> {
        let mut findings = Vec::new();
        let mut add = |ok: bool, check: &str, detail: String| {
            findings.push(Finding {
                ok,
                check: check.to_owned(),
                detail,
            });
        };
        let arch = sysctl(self.runner, "hw.machine_arch").unwrap_or_default();
        add(
            arch == "amd64",
            "architecture",
            format!("{arch}; guests are supported on amd64"),
        );
        let loaded = Path::new("/dev/vmm").exists() || Path::new("/dev/vmmctl").exists();
        let detail = if loaded {
            "vmm is loaded"
        } else {
            "vmm is not loaded; vm start loads it"
        };
        add(true, "kernel module", detail.into());
        let vmx = sysctl(self.runner, "hw.vmm.vmx.initialized").is_some_and(|v| v == "1");
        let svm = sysctl(self.runner, "hw.vmm.svm.features").is_some_and(|v| v != "0");
        if loaded {
            let detail = match (vmx, svm) {
                (true, _) => "Intel VT-x",
                (_, true) => "AMD SVM",
                _ => "not offered by this processor or its hypervisor",
            };
            add(vmx || svm, "virtualization", detail.into());
        }
        for (path, package) in [
            (FIRMWARE, "edk2-bhyve"),
            (FIRMWARE_VARS, "edk2-bhyve"),
            (LIBSLIRP, "libslirp"),
            (SSH_KEYGEN, "FreeBSD-ssh"),
            (MAKEFS, "FreeBSD-toolchain"),
            (QEMU_IMG, "qemu-tools"),
        ] {
            let exists = Path::new(path).exists();
            let detail = if exists {
                path.to_owned()
            } else {
                format!("missing: pkg install {package}")
            };
            add(exists, package, detail);
        }
        findings
    }

    fn require_support(&self) -> Result<(), VmError> {
        let missing: Vec<String> = self
            .support()
            .into_iter()
            .filter(|finding| !finding.ok)
            .map(|finding| format!("{}: {}", finding.check, finding.detail))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(VmError::Unsupported(missing.join("; ")))
        }
    }

    fn user_id(&self, user: &str) -> Result<u32, VmError> {
        if !valid_user(user) {
            return Err(VmError::Invalid(vec![format!("invalid owner {user:?}")]));
        }
        let line = self.runner.stdout(PW, &["usershow", "-n", user])?;
        line.split(':')
            .nth(2)
            .and_then(|uid| uid.parse().ok())
            .ok_or_else(|| VmError::Invalid(vec![format!("unknown user {user}")]))
    }

    fn free_port(&self) -> Result<u16, VmError> {
        let used: Vec<u16> = self
            .list()
            .iter()
            .flat_map(|c| {
                c.forwards
                    .iter()
                    .map(|f| f.host_port)
                    .chain(std::iter::once(c.ssh_port))
            })
            .collect();
        (FIRST_PORT..FIRST_PORT + 1000)
            .find(|port| !used.contains(port) && TcpListener::bind(("127.0.0.1", *port)).is_ok())
            .ok_or_else(|| VmError::Invalid(vec!["no free port".into()]))
    }

    /// Creates a machine. Nothing is left behind when a step fails.
    pub fn create(&self, request: Request, caller: &str) -> Result<VmConfig, VmError> {
        self.require_support()?;
        if !valid_name(&request.name) {
            return Err(VmError::Invalid(vec![format!(
                "invalid name {:?}",
                request.name
            )]));
        }
        let images = Images::builtin();
        let image = images.get(&request.image).ok_or_else(|| VmError::Image {
            image: request.image.clone(),
            reason: "not in the list; see meta-linux image list".into(),
        })?;
        if !image.cgroups.contains(&request.cgroups) {
            return Err(VmError::Image {
                image: image.id.clone(),
                reason: "its init system does not support that control group version".into(),
            });
        }
        let directory = self.directory(&request.name);
        if directory.exists() {
            return Err(VmError::Exists(request.name));
        }
        let owner_uid = self.user_id(&request.owner)?;
        let mut shares = vec![Share {
            tag: "share".into(),
            host: format!("{}/{}", self.share_root, request.name),
            read_only: false,
        }];
        shares.extend(request.shares);
        let config = VmConfig {
            name: request.name,
            image: request.image,
            cpus: request.cpus,
            memory_mib: request.memory_mib,
            disk_gib: request.disk_gib,
            network: request.network,
            cgroups: request.cgroups,
            owner: request.owner,
            owner_uid,
            uuid: self.runner.stdout("/bin/uuidgen", &[])?.trim().to_owned(),
            ssh_port: self.free_port()?,
            forwards: request.forwards,
            shares,
        };
        config.validate()?;
        let mut created_share = false;
        let result = self.create_files(&config, &directory, image, &mut created_share);
        audit::record(&self.root, caller, "create", &config.name, result.is_ok());
        if result.is_err() {
            let _ = fs::remove_dir_all(&directory);
            if created_share {
                let _ = self.destroy_share(&config);
            }
        }
        result.map(|()| config)
    }

    /// What [`Self::create`] would do.
    pub fn describe(&self, request: &Request) -> Result<Vec<String>, VmError> {
        let images = Images::builtin();
        let image = images.get(&request.image).ok_or_else(|| VmError::Image {
            image: request.image.clone(),
            reason: "not in the list; see meta-linux image list".into(),
        })?;
        let mut lines = vec![
            format!("image     {}", image.url),
            format!("verified  {}", image.digest),
            format!("files     {}", self.directory(&request.name).display()),
            format!(
                "share     {}/{} (a new dataset)",
                self.share_root, request.name
            ),
            format!(
                "machine   {} processors, {} MiB memory, {} GiB disk",
                request.cpus, request.memory_mib, request.disk_gib
            ),
            format!("owner     {}", request.owner),
            match request.network {
                Network::Isolated => "network   no connections out".to_owned(),
                Network::Open => {
                    "network   connections out, also to the host's loopback services".to_owned()
                }
            },
        ];
        for forward in &request.forwards {
            lines.push(format!(
                "forward   127.0.0.1:{} to port {}",
                forward.host_port, forward.guest_port
            ));
        }
        for share in &request.shares {
            let mode = if share.read_only {
                "read only"
            } else {
                "writable"
            };
            lines.push(format!("share     {} ({mode})", share.host));
        }
        Ok(lines)
    }

    /// Changes a stopped machine.
    pub fn change(&self, config: &VmConfig, change: Change, caller: &str) -> Result<(), VmError> {
        if self.state(config) == VmState::Running {
            return Err(VmError::Running(config.name.clone()));
        }
        let mut new = config.clone();
        new.cpus = change.cpus.unwrap_or(new.cpus);
        new.memory_mib = change.memory_mib.unwrap_or(new.memory_mib);
        new.disk_gib = change.disk_gib.unwrap_or(new.disk_gib);
        new.network = change.network.unwrap_or(new.network);
        new.forwards = change.forwards.unwrap_or(new.forwards);
        new.validate()?;
        if new.disk_gib < config.disk_gib {
            return Err(VmError::Invalid(vec!["a disk can only grow".into()]));
        }
        let directory = self.directory(&config.name);
        let result = (|| {
            if new.disk_gib > config.disk_gib {
                let disk = directory.join("data/disk.raw");
                let file = OpenOptions::new()
                    .write(true)
                    .open(&disk)
                    .map_err(io(&disk))?;
                file.set_len(u64::from(new.disk_gib) << 30)
                    .map_err(io(&disk))?;
            }
            write_config(&directory, &new)
        })();
        audit::record(&self.root, caller, "change", &config.name, result.is_ok());
        result
    }

    fn create_files(
        &self,
        config: &VmConfig,
        directory: &Path,
        image: &crate::images::Image,
        created_share: &mut bool,
    ) -> Result<(), VmError> {
        let raw = image.prepare(&self.root.join("images"), self.runner)?;
        fs::create_dir_all(directory).map_err(io(directory))?;
        // Keys belong to the owner; the rest is root's.
        private(directory, 0o711)?;
        let data = directory.join("data");
        fs::create_dir(&data).map_err(io(&data))?;
        private(&data, 0o711)?;
        let disk = data.join("disk.raw");
        // cp(1) clones blocks on ZFS, so a machine starts out taking no space.
        self.runner
            .stdout("/bin/cp", &[&utf8(&raw)?, &utf8(&disk)?])?;
        let file = OpenOptions::new()
            .write(true)
            .open(&disk)
            .map_err(io(&disk))?;
        let size = u64::from(config.disk_gib) << 30;
        if file.metadata().map_err(io(&disk))?.len() > size {
            return Err(VmError::Invalid(vec![
                "the disk is smaller than its image".into(),
            ]));
        }
        file.set_len(size).map_err(io(&disk))?;
        let variables = data.join("vars.fd");
        fs::copy(FIRMWARE_VARS, &variables).map_err(io(&variables))?;
        self.create_share(config)?;
        *created_share = true;
        let keys = self.create_keys(config, directory)?;
        self.create_seed(config, directory, &keys)?;
        // bhyve runs as the owner and has to open these.
        for path in [&disk, &variables, &data.join("seed.iso")] {
            private(path, 0o600)?;
            self.runner
                .stdout(CHOWN, &[&config.owner_uid.to_string(), &utf8(path)?])?;
        }
        write_config(directory, config)
    }

    /// A dataset of its own that cannot carry set-user-ID programs or devices.
    fn create_share(&self, config: &VmConfig) -> Result<(), VmError> {
        let mountpoint = format!("{}/{}", self.share_root, config.name);
        let pool = root_pool(self.runner)
            .ok_or_else(|| VmError::Unsupported("the root filesystem is not ZFS".into()))?;
        let dataset = format!("{pool}/meta/share/{}", config.name);
        self.runner.stdout(
            ZFS,
            &[
                "create",
                "-p",
                "-o",
                &format!("mountpoint={mountpoint}"),
                "-o",
                "setuid=off",
                "-o",
                "devices=off",
                &dataset,
            ],
        )?;
        self.runner.stdout(CHOWN, &[&config.owner, &mountpoint])?;
        Ok(())
    }

    fn destroy_share(&self, config: &VmConfig) -> Result<(), VmError> {
        let mountpoint = format!("{}/{}", self.share_root, config.name);
        let list = self.runner.stdout(
            ZFS,
            &["list", "-H", "-t", "filesystem", "-o", "name,mountpoint"],
        )?;
        let dataset = list
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(name, mount)| *mount == mountpoint && name.contains("/meta/share/"))
            .map(|(name, _)| name.to_owned());
        if let Some(dataset) = dataset {
            self.runner.stdout(ZFS, &["destroy", "-r", &dataset])?;
        }
        Ok(())
    }

    fn create_keys(&self, config: &VmConfig, directory: &Path) -> Result<Keys, VmError> {
        for name in ["client_key", "host_key"] {
            let path = utf8(&directory.join(name))?;
            self.runner.stdout(
                SSH_KEYGEN,
                &[
                    "-q",
                    "-t",
                    "ed25519",
                    "-N",
                    "",
                    "-C",
                    &config.bhyve_name(),
                    "-f",
                    &path,
                ],
            )?;
        }
        let read = |name: &str| {
            let path = directory.join(name);
            fs::read_to_string(&path).map_err(io(&path))
        };
        let keys = Keys {
            client_public: read("client_key.pub")?,
            host_private: read("host_key")?,
            host_public: read("host_key.pub")?,
        };
        let known = format!("meta-linux-{} {}", config.name, keys.host_public);
        let path = directory.join("known_hosts");
        fs::write(&path, known).map_err(io(&path))?;
        private(&path, 0o644)?;
        let client = directory.join("client_key");
        private(&client, 0o600)?;
        self.runner
            .stdout(CHOWN, &[&config.owner, &utf8(&client)?])?;
        Ok(keys)
    }

    fn create_seed(&self, config: &VmConfig, directory: &Path, keys: &Keys) -> Result<(), VmError> {
        let source = directory.join("seed");
        fs::create_dir(&source).map_err(io(&source))?;
        private(&source, 0o700)?;
        for (name, text) in [
            ("meta-data", seed::meta_data(config)),
            ("user-data", seed::user_data(config, keys)),
        ] {
            let path = source.join(name);
            fs::write(&path, text).map_err(io(&path))?;
        }
        let image = directory.join("data/seed.iso");
        self.runner.stdout(
            MAKEFS,
            &[
                "-t",
                "cd9660",
                "-o",
                "rockridge,label=cidata",
                &utf8(&image)?,
                &utf8(&source)?,
            ],
        )?;
        // Both hold the guest's private host key.
        private(&image, 0o600)?;
        fs::remove_dir_all(&source).map_err(io(&source))?;
        let key = directory.join("host_key");
        fs::remove_file(&key).map_err(io(&key))
    }

    /// Starts a machine in the background.
    pub fn start(&self, config: &VmConfig, caller: &str) -> Result<(), VmError> {
        if self.state(config) == VmState::Running {
            return Err(VmError::Running(config.name.clone()));
        }
        // Part of FreeBSD's base system, and named explicitly.
        self.runner.stdout(KLDLOAD, &["-n", "vmm"])?;
        self.require_support()?;
        let table = self.runner.stdout("/sbin/mount", &["-p"])?;
        for shared in &config.shares {
            share::check(&shared.host, config.owner_uid, &table).map_err(|reason| {
                VmError::Share {
                    path: shared.host.clone(),
                    reason,
                }
            })?;
        }
        let directory = self.directory(&config.name);
        let console = directory.join("console.log");
        if console.exists() {
            let _ = fs::rename(&console, directory.join("console.log.old"));
        }
        let _ = fs::remove_file(directory.join("last-exit"));
        let own = std::env::current_exe().map_err(io(Path::new("meta-linux")))?;
        let pidfile = utf8(&directory.join("supervisor.pid"))?;
        let title = config.bhyve_name();
        self.runner.stdout(
            DAEMON,
            &[
                "-f",
                "-p",
                &pidfile,
                "-t",
                &title,
                &utf8(&own)?,
                "vm",
                "supervise",
                &config.name,
            ],
        )?;
        let started = wait(Duration::from_secs(10), || {
            self.state(config) == VmState::Running
        });
        audit::record(&self.root, caller, "start", &config.name, started);
        if started {
            Ok(())
        } else {
            Err(VmError::NotReady(
                config.name.clone(),
                self.last_words(config),
            ))
        }
    }

    fn last_words(&self, config: &VmConfig) -> String {
        let directory = self.directory(&config.name);
        let console = self.console(config, 400).unwrap_or_default();
        let last = fs::read_to_string(directory.join("last-exit")).unwrap_or_default();
        let text = format!("{} {}", last.trim(), console.trim());
        if text.trim().is_empty() {
            "bhyve said nothing".into()
        } else {
            text
        }
    }

    /// Runs bhyve until the guest powers off, starting it again when it reboots.
    /// Called in the background by [`Self::start`].
    pub fn supervise(&self, config: &VmConfig) -> Result<(), VmError> {
        let directory = self.directory(&config.name);
        let confinement = Confinement::new(self.runner, config, directory.join("data"));
        self.destroy_vmm(config);
        if let Err(error) = confinement.set_up() {
            // Nobody sees the output of a background process.
            let _ = fs::write(directory.join("last-exit"), error.to_string());
            let _ = confinement.tear_down();
            return Err(error);
        }
        let result = self.run_bhyve(config, &directory, &confinement);
        self.destroy_vmm(config);
        result.and(confinement.tear_down())
    }

    fn run_bhyve(
        &self,
        config: &VmConfig,
        directory: &Path,
        confinement: &Confinement<'_>,
    ) -> Result<(), VmError> {
        let arguments = confinement.command(BHYVE, &bhyve::arguments(config));
        let path = directory.join("console.log");
        let mut restarts: Vec<Instant> = Vec::new();
        loop {
            self.destroy_vmm(config);
            let mut console = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(io(&path))?;
            if console.metadata().is_ok_and(|m| m.len() > CONSOLE_LIMIT) {
                console.set_len(0).map_err(io(&path))?;
            }
            let _ = writeln!(console, "\n--- starting ---");
            let errors = console.try_clone().map_err(io(&path))?;
            let mut child = Command::new(JEXEC)
                .args(&arguments)
                .env_clear()
                .current_dir("/")
                .stdin(Stdio::null())
                .stdout(console)
                .stderr(errors)
                .spawn()
                .map_err(io(Path::new(JEXEC)))?;
            let pid = directory.join("bhyve.pid");
            fs::write(&pid, child.id().to_string()).map_err(io(&pid))?;
            let status = child.wait().map_err(io(Path::new(JEXEC)))?;
            let _ = fs::remove_file(&pid);
            let exit = Exit::from_code(status.code().unwrap_or(-1));
            let _ = fs::write(directory.join("last-exit"), exit.as_str());
            if exit != Exit::Rebooted {
                break;
            }
            restarts.retain(|time| time.elapsed() < RESTART_WINDOW);
            restarts.push(Instant::now());
            if restarts.len() >= MAX_RESTARTS {
                let _ = fs::write(directory.join("last-exit"), "rebooted too often");
                break;
            }
        }
        Ok(())
    }

    /// Releases the kernel's side of a machine.
    fn destroy_vmm(&self, config: &VmConfig) {
        if self.state(config) == VmState::Running {
            let _ = self.runner.run(
                BHYVECTL,
                &[&format!("--vm={}", config.bhyve_name()), "--destroy"],
            );
        }
    }

    /// Asks the guest to power off, and takes the power away when it has not done so
    /// within `grace`, or at once without one.
    pub fn stop(
        &self,
        config: &VmConfig,
        grace: Option<Duration>,
        caller: &str,
    ) -> Result<(), VmError> {
        if self.state(config) == VmState::Stopped {
            return Err(VmError::NotRunning(config.name.clone()));
        }
        let pid = fs::read_to_string(self.directory(&config.name).join("bhyve.pid"))
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        let stopped = || self.state(config) == VmState::Stopped;
        let (mut done, mut forced) = (false, false);
        if let (Some(pid), Some(grace)) = (pid, grace) {
            // bhyve presses the guest's power button on this signal.
            self.runner.run(KILL, &["-TERM", &pid.to_string()])?;
            done = wait(grace, stopped);
        }
        if !done {
            forced = true;
            let vm = format!("--vm={}", config.bhyve_name());
            self.runner.run(BHYVECTL, &[&vm, "--force-poweroff"])?;
            if !wait(Duration::from_secs(10), stopped) {
                self.runner.run(BHYVECTL, &[&vm, "--destroy"])?;
            }
            done = wait(Duration::from_secs(5), stopped);
        }
        if done && forced {
            // bhyve cannot tell a guest that powered off from one that was cut off.
            let _ = fs::write(self.directory(&config.name).join("last-exit"), "forced off");
        }
        audit::record(&self.root, caller, "stop", &config.name, done);
        if done {
            Ok(())
        } else {
            Err(VmError::Guest(format!("{} cannot be stopped", config.name)))
        }
    }

    /// Waits until the guest has finished starting.
    pub fn wait_ready(&self, config: &VmConfig, timeout: Duration) -> Result<(), VmError> {
        let connection = self.connection(config);
        let deadline = Instant::now() + timeout;
        let mut last = String::from("no answer");
        while Instant::now() < deadline {
            if self.state(config) == VmState::Stopped && !self.rebooting(config) {
                return Err(VmError::NotReady(
                    config.name.clone(),
                    self.last_words(config),
                ));
            }
            // cloud-init finishes after everything it was asked to do; systemd says
            // whether services are up, "degraded" meaning that some are not.
            let probe = "cloud-init status 2>/dev/null | grep -q 'status: done\\|status: disabled\\|status: error'; \
                         a=$?; s=$(systemctl is-system-running 2>/dev/null); echo \"$a $s\"";
            match connection.capture(5, &["sh", "-c", probe]) {
                Ok((0, text)) => {
                    let text = text.trim().to_owned();
                    if text == "0 running" || text == "0 degraded" {
                        return Ok(());
                    }
                    last = text;
                }
                Ok((_, text)) => last = text.trim().to_owned(),
                Err(error) => return Err(error),
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        Err(VmError::NotReady(config.name.clone(), last))
    }

    /// A moment without a machine while its supervisor starts it again.
    fn rebooting(&self, config: &VmConfig) -> bool {
        let directory = self.directory(&config.name);
        match fs::read_to_string(directory.join("last-exit")) {
            Ok(text) => text.trim() == Exit::Rebooted.as_str(),
            Err(_) => true,
        }
    }

    /// The end of the console output.
    pub fn console(&self, config: &VmConfig, bytes: u64) -> Result<String, VmError> {
        let path = self.directory(&config.name).join("console.log");
        let mut file = File::open(&path).map_err(io(&path))?;
        let length = file.metadata().map_err(io(&path))?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(bytes)))
            .map_err(io(&path))?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(io(&path))?;
        // A guest controls this text; control characters could confuse a terminal.
        Ok(String::from_utf8_lossy(&data)
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect())
    }

    pub fn diagnose(&self, config: &VmConfig) -> Vec<Finding> {
        let mut findings = self.support();
        let mut add = |ok: bool, check: &str, detail: String| {
            findings.push(Finding {
                ok,
                check: check.to_owned(),
                detail,
            });
        };
        let directory = self.directory(&config.name);
        let disk = fs::metadata(directory.join("data/disk.raw")).map_or(0, |m| m.len());
        add(disk > 0, "disk", format!("{} GiB", disk >> 30));
        let table = self
            .runner
            .stdout("/sbin/mount", &["-p"])
            .unwrap_or_default();
        for shared in &config.shares {
            let result = share::check(&shared.host, config.owner_uid, &table);
            let detail = result.clone().err().unwrap_or_else(|| shared.host.clone());
            add(result.is_ok(), &format!("share {}", shared.tag), detail);
        }
        let state = self.state(config);
        add(
            state == VmState::Running,
            "state",
            format!("{state:?}").to_lowercase(),
        );
        if let Ok(last) = fs::read_to_string(directory.join("last-exit")) {
            let last = last.trim().to_owned();
            let ok = matches!(
                last.as_str(),
                "powered off" | "rebooted" | "halted" | "forced off"
            );
            add(ok, "last exit", last);
        }
        if state == VmState::Running {
            match self.connection(config).capture(5, &["uname", "-sr"]) {
                Ok((0, text)) => add(true, "guest", text.trim().to_owned()),
                Ok((_, text)) => add(false, "guest", text.trim().to_owned()),
                Err(error) => add(false, "guest", error.to_string()),
            }
            if let Ok((_, text)) = self
                .connection(config)
                .capture(5, &["systemctl", "is-system-running"])
            {
                let text = text.trim().to_owned();
                add(text == "running", "systemd", text);
            }
        }
        findings
    }

    /// Removes a stopped machine with its disk. Its share is kept unless asked for.
    pub fn remove(&self, config: &VmConfig, with_share: bool, caller: &str) -> Result<(), VmError> {
        if self.state(config) == VmState::Running {
            return Err(VmError::Running(config.name.clone()));
        }
        let directory = self.directory(&config.name);
        // A host that lost power may have left mounts behind.
        Confinement::new(self.runner, config, directory.join("data")).tear_down()?;
        let result = fs::remove_dir_all(&directory)
            .map_err(io(&directory))
            .and_then(|()| {
                if with_share {
                    self.destroy_share(config)
                } else {
                    Ok(())
                }
            });
        audit::record(&self.root, caller, "remove", &config.name, result.is_ok());
        result
    }
}

/// Replaces the configuration as a whole. It is readable, so that the owner can use
/// the machine without privileges.
fn write_config(directory: &Path, config: &VmConfig) -> Result<(), VmError> {
    let text = toml::to_string(config).map_err(|e| VmError::Invalid(vec![e.to_string()]))?;
    let new = directory.join("config.toml.new");
    fs::write(&new, text).map_err(io(&new))?;
    private(&new, 0o644)?;
    let path = directory.join("config.toml");
    fs::rename(&new, &path).map_err(io(&path))
}

fn wait(limit: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(unix)]
fn private(path: &Path, mode: u32) -> Result<(), VmError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(io(path))
}

#[cfg(not(unix))]
fn private(_path: &Path, _mode: u32) -> Result<(), VmError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::sample;
    use meta_common::command::FakeRunner;

    fn manager<'a>(dir: &Path, runner: &'a FakeRunner) -> Manager<'a> {
        Manager {
            root: dir.to_owned(),
            ..Manager::new(runner)
        }
    }

    fn save(dir: &Path, name: &str, config: &VmConfig) {
        let directory = dir.join("vms").join(name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("config.toml"),
            toml::to_string(config).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn configurations_are_loaded_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        let manager = manager(dir.path(), &runner);
        save(dir.path(), "debian", &sample());
        assert_eq!(manager.load("debian").unwrap(), sample());
        assert_eq!(manager.list().len(), 1);
        for name in ["..", "../vms/debian", "/etc", "", "nope"] {
            assert!(
                matches!(manager.load(name), Err(VmError::Missing(_))),
                "{name}"
            );
        }
        // A configuration moved or edited by hand is refused, not used.
        save(dir.path(), "other", &sample());
        assert!(manager.load("other").is_err());
        let mut hostile = sample();
        hostile.owner_uid = 0;
        save(dir.path(), "debian", &hostile);
        assert!(manager.load("debian").is_err());
    }

    #[test]
    fn nothing_is_created_without_support() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        let manager = manager(dir.path(), &runner);
        let request = Request {
            name: "x".into(),
            image: "debian-12".into(),
            cpus: 1,
            memory_mib: 1024,
            disk_gib: 10,
            network: Network::Isolated,
            cgroups: Cgroups::V2,
            owner: "alice".into(),
            forwards: Vec::new(),
            shares: Vec::new(),
        };
        assert!(matches!(
            manager.create(request, "alice"),
            Err(VmError::Unsupported(_))
        ));
        assert!(!dir.path().join("vms").exists());
    }

    #[test]
    fn changes_are_checked_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        let manager = manager(dir.path(), &runner);
        save(dir.path(), "debian", &sample());
        let change = |change| manager.change(&sample(), change, "a");
        assert!(
            change(Change {
                cpus: Some(0),
                ..Change::default()
            })
            .is_err()
        );
        assert!(
            change(Change {
                disk_gib: Some(5),
                ..Change::default()
            })
            .is_err()
        );
        change(Change {
            cpus: Some(4),
            network: Some(Network::Open),
            ..Change::default()
        })
        .unwrap();
        let new = manager.load("debian").unwrap();
        assert_eq!((new.cpus, new.network), (4, Network::Open));
        assert_eq!(new.memory_mib, sample().memory_mib);
    }

    #[test]
    fn console_output_cannot_control_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        let manager = manager(dir.path(), &runner);
        save(dir.path(), "debian", &sample());
        fs::write(
            manager.directory("debian").join("console.log"),
            "a\x1b]0;evil\x07\nb",
        )
        .unwrap();
        assert_eq!(manager.console(&sample(), 100).unwrap(), "a]0;evil\nb");
        assert_eq!(manager.console(&sample(), 1).unwrap(), "b");
    }

    #[test]
    fn stopping_and_removing_follow_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::new();
        let manager = manager(dir.path(), &runner);
        save(dir.path(), "debian", &sample());
        assert!(matches!(
            manager.stop(&sample(), None, "a"),
            Err(VmError::NotRunning(_))
        ));
        // Without knowing what is mounted, nothing is deleted.
        assert!(manager.remove(&sample(), false, "a").is_err());
        assert!(manager.load("debian").is_ok());
        let runner = FakeRunner::new()
            .with("/usr/sbin/jail -r meta-vm-debian", "")
            .with("/sbin/mount -p", "");
        let manager = Manager {
            root: dir.path().to_owned(),
            ..Manager::new(&runner)
        };
        manager.remove(&sample(), false, "a").unwrap();
        assert!(matches!(manager.load("debian"), Err(VmError::Missing(_))));
        let log = fs::read_to_string(dir.path().join("audit.log")).unwrap();
        assert!(log.contains("remove\tdebian\tok"));
    }
}
