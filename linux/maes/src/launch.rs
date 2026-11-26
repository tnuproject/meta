//! Starting a program where it was decided to run.
//!
//! The decision is never revised by running the program somewhere else after it has
//! failed: it may already have changed something.

use std::path::{Path, PathBuf};

use meta_common::CommandRunner;
use meta_compat::Root;
use meta_linux_vm::guest::Options;
use meta_linux_vm::{Manager, VmConfig, forward};
use meta_model::RuntimeKind;

use crate::decide::Assessed;

/// The launcher itself failed.
pub const EXIT_LAUNCHER: u8 = 125;
/// The program exists, but nothing here can run it.
pub const EXIT_NOT_RUNNABLE: u8 = 126;
pub const EXIT_NOT_FOUND: u8 = 127;

/// Variables that describe the terminal and language rather than the host. Others
/// may hold secrets or host paths and are passed only on request.
const PASSED: &[&str] = &["TERM", "COLORTERM", "LANG", "TZ"];

#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub x11: bool,
    pub terminal: bool,
    /// Further variables for a guest.
    pub environment: Vec<(String, String)>,
}

/// What will be done, ready to be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Replace this process.
    Execute {
        program: PathBuf,
        arguments: Vec<String>,
    },
    /// Run in a guest, after copying the program there if necessary.
    Guest {
        config: Box<VmConfig>,
        copy: Option<(PathBuf, String)>,
        directory: Option<String>,
        command: Vec<String>,
    },
}

impl Action {
    pub fn describe(&self) -> Vec<String> {
        match self {
            Action::Execute { program, arguments } => {
                vec![format!("run {} {}", program.display(), arguments.join(" "))]
            }
            Action::Guest {
                config,
                copy,
                directory,
                command,
            } => {
                let mut lines = Vec::new();
                if let Some((from, to)) = copy {
                    lines.push(format!(
                        "copy {} to {to} in {}",
                        from.display(),
                        config.name
                    ));
                }
                let place = directory.as_deref().unwrap_or("the home directory");
                lines.push(format!(
                    "run {} in {}, in {place}",
                    command.join(" "),
                    config.name
                ));
                lines
            }
        }
    }
}

pub fn plan(
    choice: &Assessed,
    program: &Path,
    arguments: &[String],
    working_directory: &Path,
    manager: &Manager<'_>,
    runner: &dyn CommandRunner,
) -> Result<Action, String> {
    let program = std::path::absolute(program).map_err(|e| e.to_string())?;
    let candidate = &choice.candidate;
    if let Some(preparation) = &candidate.preparation {
        return Err(format!(
            "{} is not running; start it with: {preparation}",
            candidate.label()
        ));
    }
    let instance = candidate.instance.as_deref().unwrap_or_default();
    match candidate.kind {
        // The kernel recognises Linux programs by itself.
        RuntimeKind::Native | RuntimeKind::Linuxulator => Ok(Action::Execute {
            program,
            arguments: arguments.to_vec(),
        }),
        RuntimeKind::Userspace => {
            let root = Root::open(instance, runner).map_err(|e| e.to_string())?;
            let inside = root
                .inside(&program.to_string_lossy())
                .ok_or_else(|| format!("{} is not visible inside {instance}", program.display()))?;
            let tool = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .with_file_name("meta-compat");
            let mut all = vec!["exec".to_owned(), instance.to_owned(), "--".into(), inside];
            all.extend(arguments.iter().cloned());
            Ok(Action::Execute {
                program: tool,
                arguments: all,
            })
        }
        RuntimeKind::VirtualMachine => {
            let config = manager.load(instance).map_err(|e| e.to_string())?;
            Ok(in_guest(config, &program, arguments, working_directory))
        }
    }
}

fn in_guest(config: VmConfig, program: &Path, arguments: &[String], directory: &Path) -> Action {
    let visible = config.guest_path(&program.to_string_lossy());
    let copy = match visible {
        Some(_) => None,
        None => {
            let name = program
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            // The name is kept, because programs look at it.
            let name = name.unwrap_or_else(|| "program".into());
            let target = format!("/home/{}/.cache/meta-run/{name}", config.owner);
            Some((program.to_owned(), target))
        }
    };
    let place = visible
        .or_else(|| copy.as_ref().map(|(_, target)| target.clone()))
        .unwrap_or_default();
    let mut command = vec![place];
    command.extend(arguments.iter().cloned());
    Action::Guest {
        directory: config.guest_path(&directory.to_string_lossy()),
        config: Box::new(config),
        copy,
        command,
    }
}

/// Environment for a guest.
pub fn environment(settings: &Settings) -> Vec<(String, String)> {
    let mut environment: Vec<(String, String)> = std::env::vars()
        .filter(|(name, _)| PASSED.contains(&name.as_str()) || name.starts_with("LC_"))
        .collect();
    for (name, value) in &settings.environment {
        environment.retain(|(known, _)| known != name);
        environment.push((name.clone(), value.clone()));
    }
    environment
}

/// Carries an action out and returns the program's exit code.
pub fn run(action: &Action, settings: &Settings, manager: &Manager<'_>) -> Result<i32, String> {
    match action {
        Action::Execute { program, arguments } => execute(program, arguments),
        Action::Guest {
            config,
            copy,
            directory,
            command,
        } => {
            let connection = manager.connection(config);
            if let Some((from, to)) = copy {
                connection.upload(from, to).map_err(|e| e.to_string())?;
            }
            let options = Options {
                terminal: settings.terminal,
                x11: settings.x11,
                connect_timeout: 10,
            };
            forward::run(
                &connection,
                options,
                directory.as_deref(),
                &environment(settings),
                command,
            )
            .map_err(|e| e.to_string())
        }
    }
}

/// Replaces this process, so that streams, signals and the exit code are the
/// program's own.
#[cfg(unix)]
fn execute(program: &Path, arguments: &[String]) -> Result<i32, String> {
    use std::os::unix::process::CommandExt;
    let error = std::process::Command::new(program).args(arguments).exec();
    Err(format!("{}: {error}", program.display()))
}

#[cfg(not(unix))]
fn execute(_program: &Path, _arguments: &[String]) -> Result<i32, String> {
    Err("not supported on this operating system".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meta_linux_vm::{Cgroups, Network, Share};

    fn config() -> VmConfig {
        VmConfig {
            name: "g".into(),
            image: "debian-12".into(),
            cpus: 1,
            memory_mib: 1024,
            disk_gib: 10,
            network: Network::Isolated,
            cgroups: Cgroups::V2,
            owner: "alice".into(),
            owner_uid: 1001,
            uuid: "00000000-1111-2222-3333-444444444444".into(),
            ssh_port: 22010,
            forwards: Vec::new(),
            shares: vec![Share {
                tag: "share".into(),
                host: "/var/meta-linux/share/g".into(),
                read_only: false,
            }],
        }
    }

    #[test]
    fn shared_programs_run_in_place() {
        let shared = Path::new("/var/meta-linux/share/g/app");
        let directory = Path::new("/var/meta-linux/share/g/work");
        let action = in_guest(config(), shared, &["a b".into()], directory);
        let Action::Guest {
            copy,
            directory,
            command,
            ..
        } = action
        else {
            panic!()
        };
        assert_eq!(copy, None);
        assert_eq!(directory.as_deref(), Some("/mnt/host/share/work"));
        assert_eq!(command, ["/mnt/host/share/app", "a b"]);
    }

    #[test]
    fn other_programs_are_copied() {
        let action = in_guest(
            config(),
            Path::new("/home/x/app"),
            &[],
            Path::new("/home/x"),
        );
        assert!(action.describe()[0].starts_with("copy"));
        let Action::Guest {
            copy,
            directory,
            command,
            ..
        } = action
        else {
            panic!()
        };
        assert_eq!(copy.unwrap().1, "/home/alice/.cache/meta-run/app");
        assert_eq!(directory, None);
        assert_eq!(command, ["/home/alice/.cache/meta-run/app"]);
    }

    #[test]
    fn only_harmless_variables_are_passed() {
        let settings = Settings {
            environment: vec![("SECRET".into(), "asked for".into())],
            ..Settings::default()
        };
        let environment = environment(&settings);
        assert!(environment.iter().all(|(name, _)| {
            PASSED.contains(&name.as_str()) || name.starts_with("LC_") || name == "SECRET"
        }));
        assert!(environment.iter().any(|(name, _)| name == "SECRET"));
    }
}
