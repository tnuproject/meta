//! Applying a staged release inside a cloned boot environment.
//!
//! The running system is never modified. Every step happens in the clone, which
//! is destroyed if anything fails, and the clone is first activated for a single
//! boot. [`confirm`] makes it permanent after that boot, or records that the
//! system came back on the previous environment.

use std::fs;
use std::path::{Path, PathBuf};

use meta_common::platform::read_meta_release;
use meta_common::{CommandError, CommandRunner};
use metaupd_metadata::{Apply, Artifact, Timestamp};
use metaupd_rollback::{BECTL, RollbackError};
use metaupd_verify::{HashMismatch, verify_file};
use serde::{Deserialize, Serialize};

use crate::engine::{Engine, EngineError, StagedRelease};
use crate::history::{self, Action};
use crate::state::State;
use crate::store::{Lock, StoreError, ensure_private_dir, io_at};

pub const PKG: &str = "/usr/local/sbin/pkg";
const MOUNT: &str = "/sbin/mount";
const UMOUNT: &str = "/sbin/umount";
/// Where artifacts are placed inside the clone while pkg installs them.
const INSIDE: &str = "var/cache/metaupd";
/// Files without which the clone could not boot.
const ESSENTIAL: &[&str] = &[
    "boot/kernel/kernel",
    "boot/loader.efi",
    "boot/lua/loader.lua",
    "etc/rc",
    "sbin/init",
];

/// A release that was activated for one boot and awaits confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    pub version: String,
    pub boot_environment: String,
    pub previous: String,
    pub since: Timestamp,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error(
        "{path} is inside the boot environment, so the pending update would be lost; create a \
         dataset for it, for example: zfs create -o mountpoint={path} zroot/metaupd", path = .0.display()
    )]
    StateInsideBootEnvironment(PathBuf),
    #[error("{name} uses the apply method {method:?}, which is not supported yet")]
    UnsupportedMethod { name: String, method: Apply },
    #[error("this release contains nothing to install")]
    Empty,
    #[error("META {version} in {boot_environment} is waiting for a reboot or confirmation")]
    AlreadyPending {
        version: String,
        boot_environment: String,
    },
    #[error("no boot environment is active")]
    NoActive,
    #[error("{what}: {source}")]
    Step {
        what: &'static str,
        source: CommandError,
    },
    #[error("{0} is not a FreeBSD package")]
    NotPackage(String),
    #[error("the updated environment is not valid: {0}")]
    Invalid(String),
    #[error(transparent)]
    Hash(#[from] HashMismatch),
    #[error(transparent)]
    Rollback(#[from] RollbackError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirmation {
    /// There was nothing to confirm.
    Nothing,
    /// Running on the new environment; it is now permanent.
    Confirmed(Pending),
    /// Running on the new environment, but it is unhealthy. It stays temporary.
    Unhealthy(Pending, String),
    /// The system came back on another environment.
    Reverted(Pending),
}

/// Applies `staged` and activates the result for the next boot only.
pub fn apply(engine: &Engine<'_>, staged: &StagedRelease) -> Result<Pending, EngineError> {
    let _lock = Lock::acquire(&engine.paths.state_dir)?;
    let mut state = State::load(&engine.paths)?;
    let version = staged.manifest.manifest().version.to_string();
    let result = apply_locked(engine, staged, &state);
    let detail = match &result {
        Ok(pending) => format!(
            "META {version} in {} for one boot",
            pending.boot_environment
        ),
        Err(error) => format!("META {version}: {error}"),
    };
    history::append(
        &engine.paths,
        engine.now,
        Action::Install,
        result.is_ok(),
        detail,
    )?;
    let pending = result?;
    state.pending = Some(pending.clone());
    state.save(&engine.paths)?;
    Ok(pending)
}

fn apply_locked(
    engine: &Engine<'_>,
    staged: &StagedRelease,
    state: &State,
) -> Result<Pending, ApplyError> {
    let manifest = staged.manifest.manifest();
    let artifacts: Vec<&Artifact> = manifest
        .artifacts_for(engine.edition)
        .filter(|a| a.apply != Apply::None)
        .collect();
    for artifact in &artifacts {
        if artifact.apply != Apply::PkgInBootEnvironment {
            return Err(ApplyError::UnsupportedMethod {
                name: artifact.name.to_string(),
                method: artifact.apply,
            });
        }
    }
    if artifacts.is_empty() {
        return Err(ApplyError::Empty);
    }
    if let Some(pending) = &state.pending {
        return Err(ApplyError::AlreadyPending {
            version: pending.version.clone(),
            boot_environment: pending.boot_environment.clone(),
        });
    }
    if engine.require_separate_state && shares_filesystem(&engine.paths.state_dir, Path::new("/")) {
        return Err(ApplyError::StateInsideBootEnvironment(
            engine.paths.state_dir.clone(),
        ));
    }
    let previous = metaupd_rollback::list(engine.runner)?
        .into_iter()
        .find(|be| be.active_now)
        .ok_or(ApplyError::NoActive)?
        .name;
    let name = engine.plan(staged).boot_environment;
    let mut clone = Clone::create(engine.runner, &name, &engine.paths.state_dir.join("mnt"))?;
    let packages = clone.install(staged, &artifacts)?;
    clone.validate(&packages, &manifest.version.to_string())?;
    clone.unmount()?;
    metaupd_rollback::activate(engine.runner, &name, true)?;
    clone.keep();
    Ok(Pending {
        version: manifest.version.to_string(),
        boot_environment: name,
        previous,
        since: engine.now,
    })
}

/// Called after boot: makes a healthy update permanent.
pub fn confirm(engine: &Engine<'_>) -> Result<Confirmation, EngineError> {
    let _lock = Lock::acquire(&engine.paths.state_dir)?;
    let mut state = State::load(&engine.paths)?;
    let Some(pending) = state.pending.clone() else {
        return Ok(Confirmation::Nothing);
    };
    let running = metaupd_rollback::list(engine.runner)?
        .into_iter()
        .find(|be| be.active_now);
    let record = |success, detail: String| {
        history::append(&engine.paths, engine.now, Action::Confirm, success, detail)
    };
    if running.is_none_or(|be| be.name != pending.boot_environment) {
        record(
            false,
            format!(
                "META {} did not boot; previous system kept",
                pending.version
            ),
        )?;
        state.pending = None;
        state.save(&engine.paths)?;
        return Ok(Confirmation::Reverted(pending));
    }
    let installed = engine.host.meta_version.as_ref().map(ToString::to_string);
    if installed.as_deref() != Some(pending.version.as_str()) {
        let reason = format!(
            "running system reports META {}, expected {}",
            installed.as_deref().unwrap_or("none"),
            pending.version
        );
        record(false, reason.clone())?;
        return Ok(Confirmation::Unhealthy(pending, reason));
    }
    metaupd_rollback::activate(engine.runner, &pending.boot_environment, false)?;
    record(
        true,
        format!("META {} in {}", pending.version, pending.boot_environment),
    )?;
    state.pending = None;
    state.save(&engine.paths)?;
    Ok(Confirmation::Confirmed(pending))
}

#[cfg(unix)]
fn shares_filesystem(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => true,
    }
}

#[cfg(not(unix))]
fn shares_filesystem(_a: &Path, _b: &Path) -> bool {
    true
}

/// A mounted clone of the running system, destroyed on drop unless kept.
struct Clone<'a> {
    runner: &'a dyn CommandRunner,
    name: String,
    root: PathBuf,
    devfs: bool,
    mounted: bool,
    kept: bool,
}

impl<'a> Clone<'a> {
    fn create(runner: &'a dyn CommandRunner, name: &str, root: &Path) -> Result<Self, ApplyError> {
        if !metaupd_rollback::valid_name(name) {
            return Err(RollbackError::InvalidName(name.to_owned()).into());
        }
        ensure_private_dir(root)?;
        run(
            runner,
            "cannot clone the running system",
            BECTL,
            &["create", name],
        )?;
        let mut clone = Self {
            runner,
            name: name.to_owned(),
            root: root.to_owned(),
            devfs: false,
            mounted: false,
            kept: false,
        };
        let root = clone.root_str()?;
        run(
            runner,
            "cannot mount the clone",
            BECTL,
            &["mount", name, &root],
        )?;
        clone.mounted = true;
        let dev = format!("{root}/dev");
        run(
            runner,
            "cannot mount devfs in the clone",
            MOUNT,
            &["-t", "devfs", "devfs", &dev],
        )?;
        clone.devfs = true;
        Ok(clone)
    }

    fn root_str(&self) -> Result<String, ApplyError> {
        self.root
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| ApplyError::Invalid("the mount point is not valid UTF-8".into()))
    }

    /// Installs the artifacts with pkg, which may use nothing but them.
    fn install(
        &self,
        staged: &StagedRelease,
        artifacts: &[&Artifact],
    ) -> Result<Vec<String>, ApplyError> {
        let work = self.root.join(INSIDE);
        let repository = work.join("repo");
        let configuration = work.join("repos");
        for directory in [&repository, &configuration] {
            ensure_private_dir(directory)?;
        }
        let mut packages = Vec::new();
        for artifact in artifacts {
            let target = repository.join(artifact.name.as_str());
            fs::copy(staged.directory.join(artifact.name.as_str()), &target)
                .map_err(io_at(&target))?;
            // The copy is what pkg reads, so the copy is what is verified.
            verify_file(&target, artifact)?;
            let path = target
                .to_str()
                .ok_or_else(|| ApplyError::NotPackage(artifact.name.to_string()))?;
            let name = run(
                self.runner,
                "cannot read a package",
                PKG,
                &["query", "-F", path, "%n"],
            )?;
            let name = name.trim();
            let valid = !name.is_empty()
                && !name.starts_with('-')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.+".contains(&b));
            if !valid {
                return Err(ApplyError::NotPackage(artifact.name.to_string()));
            }
            packages.push(name.to_owned());
        }
        let conf = configuration.join("META-staged.conf");
        // No repository signature is needed: every file in it was just verified
        // against the signed manifest, and no other repository is visible.
        let text = format!(
            "META-staged: {{ url: \"file:///{INSIDE}/repo\", signature_type: \"none\", \
             enabled: yes }}\n"
        );
        fs::write(&conf, text).map_err(io_at(&conf))?;
        let repo = repository
            .to_str()
            .ok_or_else(|| ApplyError::Invalid("the repository path is not valid UTF-8".into()))?;
        run(
            self.runner,
            "cannot index the packages",
            PKG,
            &["repo", "-q", repo],
        )?;
        let root = self.root_str()?;
        let repos = format!("REPOS_DIR=/{INSIDE}/repos");
        let base = ["-c", &root, "-o", &repos, "-o", "ASSUME_ALWAYS_YES=yes"];
        let pkg = |what, args: &[&str]| {
            let all: Vec<&str> = base.iter().copied().chain(args.iter().copied()).collect();
            run(self.runner, what, PKG, &all)
        };
        pkg(
            "cannot read the package index",
            &["update", "-q", "-f", "-r", "META-staged"],
        )?;
        let mut install = vec!["install", "-q", "-y", "-r", "META-staged"];
        install.extend(packages.iter().map(String::as_str));
        pkg("cannot install the packages", &install)?;
        fs::remove_dir_all(&work).map_err(io_at(&work))?;
        Ok(packages)
    }

    fn validate(&self, packages: &[String], version: &str) -> Result<(), ApplyError> {
        for file in ESSENTIAL {
            if !self.root.join(file).is_file() {
                return Err(ApplyError::Invalid(format!("/{file} is missing")));
            }
        }
        match read_meta_release(&self.root) {
            Some(release) if release.version == version => {}
            Some(release) => {
                return Err(ApplyError::Invalid(format!(
                    "it identifies itself as META {}, expected {version}",
                    release.version
                )));
            }
            None => {
                return Err(ApplyError::Invalid(
                    "it does not identify itself as META".into(),
                ));
            }
        }
        let root = self.root_str()?;
        let mut args = vec!["-c", root.as_str(), "check", "-q", "-s"];
        args.extend(packages.iter().map(String::as_str));
        run(
            self.runner,
            "installed files do not match their packages",
            PKG,
            &args,
        )?;
        Ok(())
    }

    fn unmount(&mut self) -> Result<(), ApplyError> {
        let root = self.root_str()?;
        if self.devfs {
            let dev = format!("{root}/dev");
            run(self.runner, "cannot unmount devfs", UMOUNT, &[&dev])?;
            self.devfs = false;
        }
        if self.mounted {
            run(
                self.runner,
                "cannot unmount the clone",
                BECTL,
                &["umount", &self.name],
            )?;
            self.mounted = false;
        }
        Ok(())
    }

    fn keep(&mut self) {
        self.kept = true;
    }
}

impl Drop for Clone<'_> {
    fn drop(&mut self) {
        if self.kept {
            return;
        }
        // Destroying only follows a successful unmount, so a busy mount can never
        // turn cleanup into deleting files through it.
        match self.unmount() {
            Ok(()) => {
                if let Err(error) = run(
                    self.runner,
                    "cannot destroy",
                    BECTL,
                    &["destroy", "-o", &self.name],
                ) {
                    eprintln!(
                        "metaupd: boot environment {} was left behind: {error}",
                        self.name
                    );
                }
            }
            Err(error) => {
                eprintln!(
                    "metaupd: boot environment {} was left mounted: {error}",
                    self.name
                );
            }
        }
    }
}

fn run(
    runner: &dyn CommandRunner,
    what: &'static str,
    program: &str,
    args: &[&str],
) -> Result<String, ApplyError> {
    runner
        .stdout(program, args)
        .map_err(|source| ApplyError::Step { what, source })
}
