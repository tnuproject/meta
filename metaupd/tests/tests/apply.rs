#![allow(clippy::unwrap_used)]

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

use common::{World, key};
use meta_common::{CommandError, CommandOutput, CommandRunner};
use metaupd::apply::{self, ApplyError, Confirmation};
use metaupd::state::State;
use metaupd::{Engine, EngineError, history};
use metaupd_github::DirectorySource;

/// Imitates bectl, pkg and mount well enough to check ordering and cleanup.
/// Real behaviour is tested by tests/vm/guest/m9-update.sh.
struct System {
    mnt: PathBuf,
    active: RefCell<String>,
    commands: RefCell<Vec<String>>,
    /// Commands starting with this fail.
    fail: Option<&'static str>,
    /// Version the "installed" packages report.
    version: &'static str,
}

impl System {
    fn new(world: &World) -> Self {
        Self {
            mnt: world.paths().state_dir.join("mnt"),
            active: RefCell::new("default".into()),
            commands: RefCell::default(),
            fail: None,
            version: "26.0.1",
        }
    }

    fn ran(&self, prefix: &str) -> bool {
        self.commands.borrow().iter().any(|c| c.starts_with(prefix))
    }
}

impl CommandRunner for System {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        let name = program.rsplit('/').next().unwrap();
        let line = format!("{name} {}", args.join(" "));
        self.commands.borrow_mut().push(line.clone());
        let mut stdout = String::new();
        let mut code = 0;
        if self.fail.is_some_and(|prefix| line.starts_with(prefix)) {
            code = 1;
        } else if line.starts_with("bectl list") {
            stdout = format!("{}\tNR\t/\t1G\t2026-01-01 00:00\n", self.active.borrow());
        } else if line.starts_with("bectl mount") {
            for file in [
                "boot/kernel/kernel",
                "boot/loader.efi",
                "boot/lua/loader.lua",
                "etc/rc",
                "sbin/init",
            ] {
                let path = self.mnt.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "x").unwrap();
            }
            fs::create_dir_all(self.mnt.join("dev")).unwrap();
        } else if line.starts_with("pkg query -F") {
            stdout = "meta-release\n".into();
        } else if line.starts_with("pkg -c") && line.contains(" install ") {
            fs::write(
                self.mnt.join("etc/meta-release"),
                format!("VERSION={}\n", self.version),
            )
            .unwrap();
        }
        Ok(CommandOutput {
            code,
            stdout,
            stderr: String::new(),
        })
    }
}

/// Runs `test` with a downloaded release and the given system.
fn with_engine(
    prepare: impl FnOnce(&mut System),
    change: impl FnOnce(&mut serde_json::Value),
    test: impl FnOnce(&mut Engine<'_>, &System),
) {
    let world = World::new();
    world.publish("26.0.1", 2, &[&key(2)], change);
    let mut system = System::new(&world);
    prepare(&mut system);
    let source = DirectorySource::new(world.releases());
    let mut engine = world.engine(&source);
    engine.runner = &system;
    engine.download().unwrap();
    test(&mut engine, &system);
}

fn install(engine: &Engine<'_>) -> Result<apply::Pending, EngineError> {
    apply::apply(engine, &engine.staged("26.0.1").unwrap())
}

#[test]
fn installs_into_a_clone_and_activates_it_once() {
    with_engine(
        |_| {},
        |_| {},
        |engine, system| {
            let pending = install(engine).unwrap();
            assert_eq!(pending.previous, "default");
            assert!(system.ran(&format!("bectl activate -t {}", pending.boot_environment)));
            assert!(!system.ran("bectl destroy"));
            let commands = system.commands.borrow();
            let position =
                |prefix: &str| commands.iter().position(|c| c.starts_with(prefix)).unwrap();
            assert!(position("umount") < position("bectl umount"));
            assert!(position("bectl umount") < position("bectl activate"));
            // pkg saw only the staged repository, inside the clone.
            for command in commands
                .iter()
                .filter(|c| c.contains(" install ") || c.contains(" update "))
            {
                assert!(
                    command.contains("REPOS_DIR=/var/cache/metaupd/repos"),
                    "{command}"
                );
            }
            assert_eq!(State::load(&engine.paths).unwrap().pending, Some(pending));
        },
    );
}

#[test]
fn every_failure_destroys_the_clone_and_activates_nothing() {
    for step in [
        "bectl mount",
        "mount",
        "pkg query",
        "pkg repo",
        "pkg -c",
        "bectl activate",
    ] {
        with_engine(
            |s| s.fail = Some(step),
            |_| {},
            |engine, system| {
                assert!(install(engine).is_err(), "{step}");
                assert!(system.ran("bectl destroy -o meta-26.0.1-"), "{step}");
                assert!(State::load(&engine.paths).unwrap().pending.is_none());
                let records = history::load(&engine.paths).unwrap().records;
                assert!(!records.last().unwrap().success);
            },
        );
    }
}

#[test]
fn a_clone_that_cannot_be_unmounted_is_not_destroyed() {
    with_engine(
        |s| s.fail = Some("umount"),
        |_| {},
        |engine, system| {
            assert!(install(engine).is_err());
            assert!(!system.ran("bectl destroy"));
            assert!(!system.ran("bectl activate"));
        },
    );
}

#[test]
fn a_clone_reporting_another_version_is_rejected() {
    with_engine(
        |s| s.version = "26.0",
        |_| {},
        |engine, system| {
            let error = install(engine).unwrap_err();
            assert!(
                matches!(error, EngineError::Apply(ApplyError::Invalid(_))),
                "{error}"
            );
            assert!(system.ran("bectl destroy"));
        },
    );
}

#[test]
fn unsupported_methods_are_refused_before_anything_happens() {
    let change = |m: &mut serde_json::Value| m["artifacts"][0]["apply"] = "device-firmware".into();
    with_engine(
        |_| {},
        change,
        |engine, system| {
            let error = install(engine).unwrap_err();
            assert!(matches!(
                error,
                EngineError::Apply(ApplyError::UnsupportedMethod { .. })
            ));
            assert!(!system.ran("bectl create"));
        },
    );
}

#[test]
fn state_inside_the_boot_environment_is_refused() {
    with_engine(
        |_| {},
        |_| {},
        |engine, system| {
            engine.require_separate_state = true;
            // The temporary directory may be on its own filesystem; only then is
            // installing acceptable.
            if let Err(error) = install(engine) {
                assert!(
                    matches!(
                        error,
                        EngineError::Apply(ApplyError::StateInsideBootEnvironment(_))
                    ),
                    "{error}"
                );
                assert!(!system.ran("bectl create"));
            }
        },
    );
}

#[test]
fn a_second_install_waits_for_the_first() {
    with_engine(
        |_| {},
        |_| {},
        |engine, _| {
            install(engine).unwrap();
            let error = install(engine).unwrap_err();
            assert!(
                matches!(error, EngineError::Apply(ApplyError::AlreadyPending { .. })),
                "{error}"
            );
        },
    );
}

#[test]
fn healthy_boot_is_confirmed() {
    with_engine(
        |_| {},
        |_| {},
        |engine, system| {
            assert_eq!(apply::confirm(engine).unwrap(), Confirmation::Nothing);
            let pending = install(engine).unwrap();
            system.active.replace(pending.boot_environment.clone());
            engine.host.meta_version = Some("26.0.1".parse().unwrap());
            assert_eq!(
                apply::confirm(engine).unwrap(),
                Confirmation::Confirmed(pending.clone())
            );
            assert!(system.ran(&format!("bectl activate {}", pending.boot_environment)));
            assert_eq!(apply::confirm(engine).unwrap(), Confirmation::Nothing);
        },
    );
}

#[test]
fn unhealthy_boot_is_not_made_permanent() {
    with_engine(
        |_| {},
        |_| {},
        |engine, system| {
            let pending = install(engine).unwrap();
            system.active.replace(pending.boot_environment.clone());
            assert!(matches!(
                apply::confirm(engine).unwrap(),
                Confirmation::Unhealthy(..)
            ));
            assert!(!system.ran(&format!("bectl activate {}", pending.boot_environment)));
        },
    );
}

#[test]
fn returning_to_the_old_system_is_recorded() {
    with_engine(
        |_| {},
        |_| {},
        |engine, _| {
            let pending = install(engine).unwrap();
            assert_eq!(
                apply::confirm(engine).unwrap(),
                Confirmation::Reverted(pending)
            );
            assert!(State::load(&engine.paths).unwrap().pending.is_none());
            assert!(
                !history::load(&engine.paths)
                    .unwrap()
                    .records
                    .last()
                    .unwrap()
                    .success
            );
        },
    );
}
