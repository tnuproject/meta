//! Carrying out an installation plan.
//!
//! The only code in META that writes to disks. It refuses a disk that is in use,
//! needs the disk's name repeated by the caller, and uses FreeBSD's own tools.

use std::fs;
use std::path::{Path, PathBuf};

use meta_common::{CommandError, CommandRunner};

use crate::catalog::{Desktop, desktops};
use crate::config::{Filesystem, InstallConfig, NetworkMode, Password};
use crate::disks;
use crate::plan::{self, Plan};

const ZPOOL: &str = "/sbin/zpool";
const ZFS: &str = "/sbin/zfs";
const GPART: &str = "/sbin/gpart";
const PKG: &str = "/usr/local/sbin/pkg";
const PW: &str = "/usr/sbin/pw";
const MOUNT: &str = "/sbin/mount";
const UMOUNT: &str = "/sbin/umount";
const CHROOT: &str = "/usr/sbin/chroot";
/// Login class carrying the chosen language.
const LOGIN_CLASS: &str = "meta";

/// Packages every installation gets, beyond the desktop.
const PACKAGES: &[&str] = &[
    "FreeBSD-set-minimal",
    "FreeBSD-kernel-generic",
    "FreeBSD-jail",
    "pkg",
    "sudo",
    "meta-release",
    "meta-tools",
];

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("the plan cannot be carried out: {}", .0.join("; "))]
    Plan(Vec<String>),
    #[error("not supported by this installer: {}", .0.join("; "))]
    Unsupported(Vec<String>),
    #[error("there is no disk named {0}")]
    NoDisk(String),
    #[error("{0} is in use; unmount its filesystems and export its pools first")]
    InUse(String),
    #[error("the confirmation {found:?} does not match the disk {expected:?}")]
    NotConfirmed { expected: String, found: String },
    #[error("a pool named {0} already exists on this system; choose another name")]
    PoolExists(String),
    #[error("invalid pool name {0:?}")]
    PoolName(String),
    #[error("the user needs a password")]
    NoPassword,
    #[error("{0} is not an empty directory")]
    TargetBusy(PathBuf),
    #[error("{0} does not exist in the new system")]
    MissingChoice(String),
    #[error("{what}: {source}")]
    Step {
        what: &'static str,
        source: CommandError,
    },
    #[error("{path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

#[derive(Debug)]
pub struct Options {
    /// Directory with pkg repository configuration for the installation.
    pub repositories: PathBuf,
    pub pool: String,
    /// Where the new system is mounted while it is being installed.
    pub target: PathBuf,
    /// Repeats the disk's name.
    pub confirmation: String,
    /// Also use the first serial port as a console.
    pub serial_console: bool,
    /// Distinguishes this installation's partition labels from others.
    pub label: String,
    /// Directory with the trust anchor and package key to install, if any.
    pub trust: Option<PathBuf>,
}

/// Installs. `progress` is told each step before it starts.
pub fn install(
    config: &InstallConfig,
    password: &Password,
    options: &Options,
    runner: &dyn CommandRunner,
    progress: &mut dyn FnMut(&str),
) -> Result<(), InstallError> {
    let disk = preflight(config, password, options, runner)?;
    let mut session = Session {
        config,
        options,
        runner,
        disk,
        mounted: Vec::new(),
        pool: false,
    };
    let result = session.run(password, progress);
    // Runs after success and failure alike, so the disk is never left in use.
    let closed = session.close();
    result.and(closed)
}

/// Everything that can be checked before the disk is touched.
pub fn preflight(
    config: &InstallConfig,
    password: &Password,
    options: &Options,
    runner: &dyn CommandRunner,
) -> Result<String, InstallError> {
    Plan::build(config).map_err(InstallError::Plan)?;
    let unsupported = plan::unsupported(config);
    if !unsupported.is_empty() {
        return Err(InstallError::Unsupported(unsupported));
    }
    if password.is_empty() {
        return Err(InstallError::NoPassword);
    }
    let valid_pool = (1..=32).contains(&options.pool.len())
        && options.pool.starts_with(|c: char| c.is_ascii_lowercase())
        && options
            .pool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    // Names that start like vdev types are not accepted by ZFS.
    let reserved = ["mirror", "raidz", "draid", "spare", "log"];
    if !valid_pool || reserved.iter().any(|name| options.pool.starts_with(name)) {
        return Err(InstallError::PoolName(options.pool.clone()));
    }
    let disk = config.disk.clone().unwrap_or_default();
    if options.confirmation != disk {
        return Err(InstallError::NotConfirmed {
            expected: disk,
            found: options.confirmation.clone(),
        });
    }
    let list = step(runner, "cannot list disks", "/sbin/geom", &["disk", "list"])?;
    let found = disks::parse_geom(&list)
        .into_iter()
        .find(|candidate| candidate.name == disk)
        .ok_or_else(|| InstallError::NoDisk(disk.clone()))?;
    if found.in_use {
        return Err(InstallError::InUse(disk));
    }
    let pools = step(
        runner,
        "cannot list pools",
        ZPOOL,
        &["list", "-H", "-o", "name"],
    )?;
    if pools.lines().any(|name| name == options.pool) {
        return Err(InstallError::PoolExists(options.pool.clone()));
    }
    if let Ok(mut entries) = fs::read_dir(&options.target)
        && entries.next().is_some()
    {
        return Err(InstallError::TargetBusy(options.target.clone()));
    }
    Ok(disk)
}

fn step(
    runner: &dyn CommandRunner,
    what: &'static str,
    program: &str,
    args: &[&str],
) -> Result<String, InstallError> {
    runner
        .stdout(program, args)
        .map_err(|source| InstallError::Step { what, source })
}

struct Session<'a> {
    config: &'a InstallConfig,
    options: &'a Options,
    runner: &'a dyn CommandRunner,
    disk: String,
    mounted: Vec<String>,
    pool: bool,
}

struct Partitions {
    efi: String,
    zfs: String,
}

impl Session<'_> {
    fn run(
        &mut self,
        password: &Password,
        progress: &mut dyn FnMut(&str),
    ) -> Result<(), InstallError> {
        progress("partitioning");
        let partitions = self.partition()?;
        progress("creating the ZFS pool");
        self.create_pool(&partitions.zfs)?;
        progress("installing packages");
        self.install_packages()?;
        self.run_triggers()?;
        progress("installing the boot loader");
        self.install_loader(&partitions.efi)?;
        progress("configuring");
        self.configure()?;
        progress("creating the user");
        self.create_user(password)?;
        Ok(())
    }

    fn step(
        &self,
        what: &'static str,
        program: &str,
        args: &[&str],
    ) -> Result<String, InstallError> {
        step(self.runner, what, program, args)
    }

    fn root(&self) -> String {
        self.options.target.to_string_lossy().into_owned()
    }

    fn path(&self, inside: &str) -> PathBuf {
        self.options.target.join(inside)
    }

    fn write(&self, inside: &str, contents: &str) -> Result<(), InstallError> {
        let path = self.path(inside);
        let io = |source| InstallError::Io {
            path: path.display().to_string(),
            source,
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let io = |source| InstallError::Io {
            path: path.display().to_string(),
            source,
        };
        fs::write(&path, contents).map_err(io)
    }

    fn append(&self, inside: &str, contents: &str) -> Result<(), InstallError> {
        let current = fs::read_to_string(self.path(inside)).unwrap_or_default();
        self.write(inside, &format!("{current}{contents}"))
    }

    fn partition(&self) -> Result<Partitions, InstallError> {
        let disk = self.disk.as_str();
        let label = &self.options.label;
        // A disk without a partition table makes `gpart show` fail.
        let has_table = self
            .runner
            .run(GPART, &["show", disk])
            .is_ok_and(|output| output.success());
        if has_table {
            self.step(
                "cannot erase the partition table",
                GPART,
                &["destroy", "-F", disk],
            )?;
        }
        self.step(
            "cannot create a partition table",
            GPART,
            &["create", "-s", "gpt", disk],
        )?;
        let mut index = 0;
        let mut add = |align: &str, kind: &str, name: &str, size: Option<&str>| {
            index += 1;
            let label = format!("meta-{name}-{label}");
            let mut args = vec!["add", "-a", align, "-t", kind, "-l", label.as_str()];
            if let Some(size) = size {
                args.extend(["-s", size]);
            }
            args.push(disk);
            self.step("cannot create a partition", GPART, &args)?;
            Ok::<String, InstallError>(format!("{disk}p{index}"))
        };
        let efi = add("4k", "efi", "efi", Some("260M"))?;
        add("4k", "freebsd-boot", "boot", Some("512K"))?;
        if self.config.swap_gib > 0 {
            add(
                "1m",
                "freebsd-swap",
                "swap",
                Some(&format!("{}G", self.config.swap_gib)),
            )?;
        }
        let zfs = add("1m", "freebsd-zfs", "zfs", None)?;
        self.step(
            "cannot install BIOS boot code",
            GPART,
            &[
                "bootcode",
                "-b",
                "/boot/pmbr",
                "-p",
                "/boot/gptzfsboot",
                "-i",
                "2",
                disk,
            ],
        )?;
        Ok(Partitions { efi, zfs })
    }

    fn create_pool(&mut self, partition: &str) -> Result<(), InstallError> {
        let pool = self.options.pool.as_str();
        let altroot = format!("altroot={}", self.root());
        self.step(
            "cannot create the pool",
            ZPOOL,
            &[
                "create",
                "-f",
                "-o",
                &altroot,
                "-o",
                "cachefile=none",
                "-O",
                "compression=lz4",
                "-O",
                "atime=off",
                "-m",
                "none",
                pool,
                partition,
            ],
        )?;
        self.pool = true;
        let zfs = |args: &[&str]| self.step("cannot create a dataset", ZFS, args).map(drop);
        let name = |suffix: &str| format!("{pool}/{suffix}");
        zfs(&["create", "-o", "mountpoint=none", &name("ROOT")])?;
        let root = name("ROOT/default");
        zfs(&[
            "create",
            "-o",
            "mountpoint=/",
            "-o",
            "canmount=noauto",
            &root,
        ])?;
        zfs(&["mount", &root])?;
        zfs(&["create", "-o", "mountpoint=/home", &name("home")])?;
        zfs(&[
            "create",
            "-o",
            "mountpoint=/tmp",
            "-o",
            "setuid=off",
            &name("tmp"),
        ])?;
        zfs(&[
            "create",
            "-o",
            "mountpoint=/var",
            "-o",
            "canmount=off",
            &name("var"),
        ])?;
        for dataset in ["audit", "crash", "log", "mail", "tmp"] {
            zfs(&[
                "create",
                "-o",
                "setuid=off",
                &name(&format!("var/{dataset}")),
            ])?;
        }
        zfs(&["create", "-o", "mountpoint=none", &name("meta")])?;
        // Outside the boot environment, so that a rollback keeps downgrade
        // protection and the update history.
        zfs(&[
            "create",
            "-o",
            "mountpoint=/var/db/metaupd",
            &name("meta/state"),
        ])?;
        zfs(&["create", "-o", "mountpoint=none", &name("meta/compat")])?;
        let bootfs = format!("bootfs={root}");
        self.step(
            "cannot select the boot dataset",
            ZPOOL,
            &["set", &bootfs, pool],
        )?;
        for directory in ["tmp", "var/tmp"] {
            let path = self.path(directory).display().to_string();
            self.step("cannot set permissions", "/bin/chmod", &["1777", &path])?;
        }
        Ok(())
    }

    fn desktop(&self) -> Option<Desktop> {
        desktops()
            .into_iter()
            .find(|d| self.config.desktop.as_deref() == Some(d.id.as_str()))
    }

    fn install_packages(&self) -> Result<(), InstallError> {
        // pkg looks for repository keys inside the new system.
        for directory in ["usr/share/keys", "usr/local/etc/meta/trust"] {
            let source = Path::new("/").join(directory);
            if source.is_dir() {
                let target = self.path(directory);
                fs::create_dir_all(&target).map_err(|source| InstallError::Io {
                    path: target.display().to_string(),
                    source,
                })?;
                let from = format!("{}/", source.display());
                let to = format!("{}/", target.display());
                self.step("cannot copy package keys", "/bin/cp", &["-R", &from, &to])?;
            }
        }
        if let Some(trust) = &self.options.trust {
            let from = format!("{}/", trust.display());
            let to = format!("{}/", self.path("usr/local/etc/meta/trust").display());
            self.step(
                "cannot copy the trust anchor",
                "/bin/cp",
                &["-R", &from, &to],
            )?;
        }
        let root = self.root();
        let repositories = self.options.repositories.to_string_lossy().into_owned();
        let base = [
            "-r",
            root.as_str(),
            "-R",
            repositories.as_str(),
            "-o",
            "IGNORE_OSVERSION=yes",
            "-o",
            "ASSUME_ALWAYS_YES=yes",
        ];
        let pkg = |what, args: &[&str]| {
            let all: Vec<&str> = base.iter().copied().chain(args.iter().copied()).collect();
            self.runner
                .inherit(PKG, &all)
                .map_err(|source| InstallError::Step { what, source })
        };
        pkg(
            "cannot read the package repositories",
            &["update", "-q", "-f"],
        )?;
        let desktop = self.desktop();
        let mut install = vec!["install", "-q", "-y"];
        install.extend(PACKAGES);
        if let Some(desktop) = &desktop {
            install.extend(desktop.packages.iter().map(String::as_str));
            install.extend(["dbus", "meta-services", "meta-update-manager"]);
        }
        pkg("cannot install packages", &install)?;
        pkg(
            "cannot clean the package cache",
            &["clean", "-q", "-a", "-y"],
        )
    }

    /// Runs what pkg defers when it installs into another root: font, icon and
    /// schema caches, without which a desktop does not start.
    fn run_triggers(&mut self) -> Result<(), InstallError> {
        let root = self.root();
        let dev = self.path("dev").display().to_string();
        self.step("cannot mount devfs", MOUNT, &["-t", "devfs", "devfs", &dev])?;
        self.mounted.push(dev);
        self.step(
            "cannot build the library cache",
            CHROOT,
            &[&root, "/usr/sbin/service", "ldconfig", "onestart"],
        )?;
        self.step(
            "cannot run package triggers",
            CHROOT,
            &[&root, "/usr/local/sbin/pkg", "triggers", "-q"],
        )?;
        Ok(())
    }

    fn install_loader(&mut self, partition: &str) -> Result<(), InstallError> {
        let device = format!("/dev/{partition}");
        self.step(
            "cannot format the EFI system partition",
            "/sbin/newfs_msdos",
            &["-F", "32", "-c", "1", "-L", "EFISYS", &device],
        )?;
        let mountpoint = self.path("boot/efi");
        let io = |source| InstallError::Io {
            path: "boot/efi".into(),
            source,
        };
        fs::create_dir_all(&mountpoint).map_err(io)?;
        let mountpoint = mountpoint.display().to_string();
        self.step(
            "cannot mount the EFI system partition",
            MOUNT,
            &["-t", "msdosfs", &device, &mountpoint],
        )?;
        self.mounted.push(mountpoint);
        let loader = self.path("boot/loader.efi");
        // The first is the removable-media path every firmware falls back to.
        for name in ["EFI/BOOT/BOOTX64.EFI", "EFI/freebsd/loader.efi"] {
            let target = self.path("boot/efi").join(name);
            let io = |source| InstallError::Io {
                path: target.display().to_string(),
                source,
            };
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io)?;
            }
            let io = |source| InstallError::Io {
                path: target.display().to_string(),
                source,
            };
            fs::copy(&loader, &target).map_err(io)?;
        }
        Ok(())
    }

    fn configure(&self) -> Result<(), InstallError> {
        let label = &self.options.label;
        let mut fstab = format!("/dev/gpt/meta-efi-{label}\t/boot/efi\tmsdosfs\trw,failok\t0\t0\n");
        if self.config.swap_gib > 0 {
            fstab.push_str(&format!(
                "/dev/gpt/meta-swap-{label}\tnone\tswap\tsw\t0\t0\n"
            ));
        }
        self.write("etc/fstab", &fstab)?;
        let mut loader = String::from(
            "zfs_load=\"YES\"\nkern.geom.label.disk_ident.enable=\"0\"\n\
             kern.geom.label.gptid.enable=\"0\"\n",
        );
        if self.options.serial_console {
            loader.push_str("console=\"comconsole,efi,vidconsole\"\nboot_multicons=\"YES\"\nboot_serial=\"YES\"\n");
        }
        self.append("boot/loader.conf", &loader)?;

        let keymap = format!("usr/share/vt/keymaps/{}.kbd", self.config.keymap);
        if !self.path(&keymap).is_file() {
            return Err(InstallError::MissingChoice(keymap));
        }
        let zone = self.path("usr/share/zoneinfo").join(&self.config.timezone);
        if !zone.is_file() {
            return Err(InstallError::MissingChoice(format!(
                "timezone {}",
                self.config.timezone
            )));
        }
        let localtime = self.path("etc/localtime").display().to_string();
        let zone = zone.display().to_string();
        self.step("cannot set the timezone", "/bin/cp", &[&zone, &localtime])?;
        self.write("var/db/zoneinfo", &format!("{}\n", self.config.timezone))?;

        let mut settings = vec![
            ("hostname", self.config.hostname.clone()),
            ("keymap", format!("{}.kbd", self.config.keymap)),
            ("zfs_enable", "YES".to_owned()),
            ("dumpdev", "AUTO".to_owned()),
        ];
        if self.config.network == NetworkMode::Dhcp {
            settings.push(("ifconfig_DEFAULT", "DHCP inet6 accept_rtadv".to_owned()));
        }
        if self.config.linux_compat {
            settings.push(("linux_enable", "YES".to_owned()));
        }
        if let Some(desktop) = self.desktop() {
            for service in ["dbus_enable", "metaupdd_enable", "meta_netd_enable"] {
                settings.push((service, "YES".to_owned()));
            }
            for package in &desktop.packages {
                match package.as_str() {
                    "lightdm" => settings.push(("lightdm_enable", "YES".to_owned())),
                    "gdm" => settings.push(("gdm_enable", "YES".to_owned())),
                    "sddm" => settings.push(("sddm_enable", "YES".to_owned())),
                    "seatd" => settings.push(("seatd_enable", "YES".to_owned())),
                    _ => {}
                }
            }
        }
        if let Some(desktop) = self.desktop()
            && desktop.display.contains("x11")
        {
            let method = meta_common::platform::sysctl(self.runner, "machdep.bootmethod");
            let driver = if method.as_deref() == Some("UEFI") {
                "scfb"
            } else {
                "vesa"
            };
            self.write(
                "usr/local/etc/X11/xorg.conf.d/10-meta-framebuffer.conf",
                &framebuffer(driver),
            )?;
        }
        let root = self.root();
        let lines: String = settings
            .iter()
            .map(|(key, value)| format!("{key}=\"{value}\"\n"))
            .collect();
        self.append("etc/rc.conf", &lines)?;

        self.append(
            "etc/login.conf",
            &format!(
                "\n{LOGIN_CLASS}|META users:\\\n\t:charset=UTF-8:\\\n\t:lang={}:\\\n\t:tc=default:\n",
                self.config.language
            ),
        )?;
        self.step(
            "cannot rebuild login.conf",
            CHROOT,
            &[&root, "cap_mkdb", "/etc/login.conf"],
        )?;
        self.write(
            "usr/local/etc/meta/eds.toml",
            &format!("enabled = {}\n", self.config.eds_enabled()),
        )?;
        // Single-user mode asks for authentication.
        let ttys =
            fs::read_to_string(self.path("etc/ttys")).map_err(|source| InstallError::Io {
                path: "etc/ttys".into(),
                source,
            })?;
        self.write("etc/ttys", &insecure_console(&ttys))
    }

    fn create_user(&self, password: &Password) -> Result<(), InstallError> {
        let root = self.root();
        let user = &self.config.user;
        let mut groups = user.groups.clone();
        if user.sudo && !groups.iter().any(|g| g == "wheel") {
            groups.push("wheel".into());
        }
        if self.config.desktop.is_some() && !groups.iter().any(|g| g == "video") {
            groups.push("video".into());
        }
        let groups = groups.join(",");
        let mut args = vec![
            "-R",
            root.as_str(),
            "useradd",
            "-n",
            &user.name,
            "-c",
            &user.full_name,
            "-m",
            "-s",
            "/bin/sh",
            "-L",
            LOGIN_CLASS,
        ];
        if !groups.is_empty() {
            args.extend(["-G", groups.as_str()]);
        }
        // The password is read from standard input, so it never shows up in the
        // process list.
        args.extend(["-h", "0"]);
        self.runner
            .feed(PW, &args, password.expose().as_bytes())
            .map_err(|source| InstallError::Step {
                what: "cannot create the user",
                source,
            })?;
        self.step(
            "cannot lock the root password",
            PW,
            &["-R", &root, "usermod", "-n", "root", "-h", "-"],
        )?;
        self.write(
            "usr/local/etc/sudoers.d/meta",
            "Defaults use_pty\nDefaults env_reset\n\
             Defaults secure_path=\"/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin\"\n\
             %wheel ALL=(ALL:ALL) ALL\n",
        )?;
        let file = self
            .path("usr/local/etc/sudoers.d/meta")
            .display()
            .to_string();
        self.step(
            "cannot protect the sudo rules",
            "/bin/chmod",
            &["0440", &file],
        )?;
        self.step(
            "the sudo rules are not valid",
            CHROOT,
            &[
                &root,
                "/usr/bin/env",
                // The library path cache is only built when the system boots.
                "LD_LIBRARY_PATH=/usr/local/lib",
                "/usr/local/sbin/visudo",
                "-c",
                "-q",
            ],
        )?;
        Ok(())
    }

    fn close(&mut self) -> Result<(), InstallError> {
        let mut result = Ok(());
        for mountpoint in self.mounted.drain(..).rev() {
            let unmounted = step(self.runner, "cannot unmount", UMOUNT, &[&mountpoint]);
            result = result.and(unmounted.map(drop));
        }
        if self.pool {
            let exported = step(
                self.runner,
                "cannot export the pool",
                ZPOOL,
                &["export", &self.options.pool],
            );
            result = result.and(exported.map(drop));
            self.pool = false;
        }
        result
    }
}

/// X server configuration for a display without a kernel graphics driver.
///
/// Without it the X server finds both framebuffer drivers and refuses to choose.
fn framebuffer(driver: &str) -> String {
    format!(
        "# Unaccelerated graphics through the firmware's framebuffer. Remove this file\n\
         # after installing a kernel graphics driver such as drm-kmod.\n\
         Section \"Device\"\n\tIdentifier \"Firmware framebuffer\"\n\tDriver \"{driver}\"\nEndSection\n"
    )
}

/// Marks the console insecure in ttys(5), leaving every other line alone.
fn insecure_console(ttys: &str) -> String {
    let mut result = String::new();
    for line in ttys.lines() {
        if line.split_whitespace().next() == Some("console") {
            result.push_str(
                &line
                    .replace("insecure", "secure")
                    .replace("secure", "insecure"),
            );
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    result
}

/// Whether the configuration can be installed by this version.
pub(crate) fn supported_filesystem(config: &InstallConfig) -> bool {
    config.filesystem == Filesystem::Zfs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::tests::valid;
    use meta_common::CommandOutput;
    use std::cell::RefCell;

    const FREE: &str =
        "Geom name: vtbd1\n1. Name: vtbd1\n   Mediasize: 42949672960 (40G)\n   Mode: r0w0e0\n";
    const BUSY: &str =
        "Geom name: vtbd1\n1. Name: vtbd1\n   Mediasize: 42949672960 (40G)\n   Mode: r1w1e1\n";

    /// Records commands; everything succeeds except what `fail` starts with.
    struct Recorder {
        geom: &'static str,
        fail: &'static str,
        commands: RefCell<Vec<String>>,
    }

    impl Recorder {
        fn new(geom: &'static str) -> Self {
            Self {
                geom,
                fail: "none",
                commands: RefCell::default(),
            }
        }
    }

    impl CommandRunner for Recorder {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
            let line = format!("{program} {}", args.join(" "));
            let failed = line.contains(self.fail);
            self.commands.borrow_mut().push(line.clone());
            let stdout = match line.as_str() {
                "/sbin/geom disk list" => self.geom.to_owned(),
                "/sbin/zpool list -H -o name" => "zroot\n".to_owned(),
                _ => String::new(),
            };
            Ok(CommandOutput {
                code: i32::from(failed),
                stdout,
                stderr: String::new(),
            })
        }

        fn feed(&self, program: &str, args: &[&str], _: &[u8]) -> Result<(), CommandError> {
            self.stdout(program, args).map(drop)
        }
    }

    fn options(target: &Path) -> Options {
        Options {
            repositories: "/repos".into(),
            pool: "meta".into(),
            target: target.to_owned(),
            confirmation: "vtbd1".into(),
            serial_console: false,
            label: "test".into(),
            trust: None,
        }
    }

    fn config() -> InstallConfig {
        InstallConfig {
            disk: Some("vtbd1".into()),
            ..valid()
        }
    }

    fn password() -> Password {
        let mut password = Password::default();
        "secret".chars().for_each(|c| password.push(c));
        password
    }

    fn check(
        change: impl FnOnce(&mut InstallConfig, &mut Options, &mut Password),
        geom: &'static str,
    ) -> Result<String, InstallError> {
        let dir = tempfile::tempdir().unwrap();
        let (mut config, mut options, mut password) = (config(), options(dir.path()), password());
        change(&mut config, &mut options, &mut password);
        let runner = Recorder::new(geom);
        let result = preflight(&config, &password, &options, &runner);
        let commands = runner.commands.borrow();
        assert!(
            commands.iter().all(|c| c.contains(" list")),
            "only listing: {commands:?}"
        );
        result
    }

    #[test]
    fn preflight_accepts_a_free_confirmed_disk() {
        assert_eq!(check(|_, _, _| {}, FREE).unwrap(), "vtbd1");
    }

    #[test]
    fn preflight_refuses_without_touching_anything() {
        use InstallError as E;
        assert!(matches!(check(|_, _, _| {}, BUSY), Err(E::InUse(_))));
        assert!(matches!(check(|_, _, _| {}, ""), Err(E::NoDisk(_))));
        for text in ["", "vtbd0", "vtbd1 ", "VTBD1", "yes"] {
            let result = check(|_, o, _| o.confirmation = text.into(), FREE);
            assert!(matches!(result, Err(E::NotConfirmed { .. })), "{text:?}");
        }
        for pool in ["", "-f", "a b", "mirror0", "log", "Zroot", "a/b"] {
            let result = check(|_, o, _| o.pool = pool.into(), FREE);
            assert!(matches!(result, Err(E::PoolName(_))), "{pool:?}");
        }
        assert!(matches!(
            check(|_, o, _| o.pool = "zroot".into(), FREE),
            Err(E::PoolExists(_))
        ));
        assert!(matches!(
            check(|_, _, p| *p = Password::default(), FREE),
            Err(E::NoPassword)
        ));
        assert!(matches!(
            check(|c, _, _| c.disk = None, FREE),
            Err(E::Plan(_))
        ));
        assert!(matches!(
            check(|c, _, _| c.encrypt = true, FREE),
            Err(E::Unsupported(_))
        ));
        let ufs = check(|c, _, _| c.filesystem = Filesystem::Ufs, FREE);
        assert!(matches!(ufs, Err(E::Unsupported(_))));
    }

    #[test]
    fn preflight_refuses_a_used_target_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x"), "").unwrap();
        let result = preflight(
            &config(),
            &password(),
            &options(dir.path()),
            &Recorder::new(FREE),
        );
        assert!(matches!(result, Err(InstallError::TargetBusy(_))));
    }

    #[test]
    fn failure_exports_the_pool_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = Recorder::new(FREE);
        runner.fail = " install -q -y";
        let error = install(
            &config(),
            &password(),
            &options(dir.path()),
            &runner,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            error,
            InstallError::Step {
                what: "cannot install packages",
                ..
            }
        ));
        let commands = runner.commands.borrow();
        assert_eq!(commands.last().unwrap(), "/sbin/zpool export meta");
        assert!(!commands.iter().any(|c| c.contains("newfs_msdos")));
    }

    #[test]
    fn only_the_confirmed_disk_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let mut runner = Recorder::new(FREE);
        runner.fail = " install -q -y";
        let _ = install(
            &config(),
            &password(),
            &options(dir.path()),
            &runner,
            &mut |_| {},
        );
        for command in runner
            .commands
            .borrow()
            .iter()
            .filter(|c| c.starts_with(GPART))
        {
            assert!(command.ends_with(" vtbd1"), "{command}");
        }
    }

    #[test]
    fn console_becomes_insecure_once() {
        let text = "console\tnone\tunknown\toff secure\nttyv0\t\"x\"\txterm\tonifexists secure\n";
        let changed = insecure_console(text);
        assert!(changed.starts_with("console\tnone\tunknown\toff insecure\n"));
        assert!(changed.contains("onifexists secure"));
        assert_eq!(insecure_console(&changed), changed);
    }
}
