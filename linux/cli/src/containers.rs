//! Container engines inside machines.
//!
//! These are Linux containers on the guest's kernel. They are not FreeBSD jails, and
//! the engine is as privileged in the guest as it would be on any Linux system; what
//! protects the host is the machine around it.

use std::io::IsTerminal;

use clap::{Args, Subcommand, ValueEnum};
use meta_linux_vm::guest::Options;
use meta_linux_vm::{Manager, Network, VmConfig, forward};

use crate::user_machine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Engine {
    Docker,
    Podman,
    Containerd,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Engine::Docker => "docker",
            Engine::Podman => "podman",
            Engine::Containerd => "containerd",
        }
    }

    fn package(self, debian: bool) -> &'static str {
        match (self, debian) {
            (Engine::Docker, true) => "docker.io",
            _ => self.name(),
        }
    }
}

#[derive(Debug, Args)]
pub struct Target {
    /// Machine; may be left out when you have only one
    #[arg(long, value_name = "NAME")]
    pub vm: Option<String>,
    #[arg(long, value_enum, default_value = "podman")]
    pub engine: Engine,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Install an engine from the guest's distribution
    Install {
        #[command(flatten)]
        target: Target,
    },
    /// Run a container and pass its exit code on
    Run {
        #[command(flatten)]
        target: Target,
        /// Memory limit in MiB
        #[arg(long, default_value_t = 1024)]
        memory: u32,
        /// Highest number of processes
        #[arg(long, default_value_t = 512)]
        processes: u32,
        /// OCI image, such as docker.io/library/alpine:3.22
        image: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// List running containers
    List {
        #[command(flatten)]
        target: Target,
    },
    /// Stop a container
    Stop {
        #[command(flatten)]
        target: Target,
        container: String,
    },
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// Images and container names must not be mistaken for options.
fn valid_reference(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphanumeric())
        && text.len() <= 255
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"./:@_-".contains(&b))
}

fn installation(engine: Engine, image: &str) -> Option<Vec<String>> {
    let script = match image {
        "debian-12" => {
            "export DEBIAN_FRONTEND=noninteractive; apt-get update -q && \
             exec apt-get install -q -y --no-install-recommends \"$@\""
        }
        // Installing without upgrading is not supported on Arch.
        "arch" => "exec pacman -Syu --noconfirm --needed \"$@\"",
        _ => return None,
    };
    let mut command = strings(&["sudo", "sh", "-c", script, "sh"]);
    command.push(engine.package(image == "debian-12").to_owned());
    if engine == Engine::Podman {
        // Rootless containers: id maps, network and storage in user namespaces.
        let uidmap = if image == "debian-12" {
            "uidmap"
        } else {
            "shadow"
        };
        command.extend(strings(&[uidmap, "slirp4netns", "fuse-overlayfs"]));
    }
    if engine == Engine::Docker && image == "debian-12" {
        command.push("iptables".into());
    }
    Some(command)
}

/// The commands of a run, in order; the last one's exit code counts.
fn running(
    engine: Engine,
    image: &str,
    memory: u32,
    processes: u32,
    command: &[String],
) -> Vec<Vec<String>> {
    let limits = [
        format!("--memory={memory}m"),
        format!("--pids-limit={processes}"),
    ];
    let mut all = Vec::new();
    let mut run = match engine {
        // Containers of the ordinary user, without privileges in the guest.
        Engine::Podman => strings(&["podman", "run", "--rm", "--interactive"]),
        Engine::Docker => strings(&["sudo", "docker", "run", "--rm", "--interactive"]),
        Engine::Containerd => {
            all.push(strings(&["sudo", "ctr", "images", "pull", image]));
            strings(&["sudo", "ctr", "run", "--rm"])
        }
    };
    if engine == Engine::Containerd {
        run.push(format!("--memory-limit={}", u64::from(memory) << 20));
        run.push(image.to_owned());
        run.push(format!("meta-{}", std::process::id()));
        if command.is_empty() {
            // ctr uses the image's own command then.
        }
    } else {
        run.extend(limits);
        run.push(image.to_owned());
    }
    run.extend(command.iter().cloned());
    all.push(run);
    all
}

fn execute(manager: &Manager<'_>, config: &VmConfig, command: &[String]) -> Result<i32, String> {
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let options = Options {
        terminal,
        ..Options::default()
    };
    forward::run(&manager.connection(config), options, None, &[], command)
        .map_err(|e| e.to_string())
}

pub fn run(manager: &Manager<'_>, command: &Command) -> Result<i32, String> {
    let target = match command {
        Command::Install { target }
        | Command::Run { target, .. }
        | Command::List { target }
        | Command::Stop { target, .. } => target,
    };
    let config = user_machine(manager, target.vm.as_deref())?;
    let offline = config.network == Network::Isolated;
    match command {
        Command::Install { target } => {
            if offline {
                return Err(format!(
                    "{} has no network; see: meta-linux vm set --network open {}",
                    config.name, config.name
                ));
            }
            let install = installation(target.engine, &config.image)
                .ok_or_else(|| format!("not known how to install it on {}", config.image))?;
            let code = execute(manager, &config, &install)?;
            if code != 0 || target.engine == Engine::Podman {
                return Ok(code);
            }
            let service = target.engine.name();
            execute(
                manager,
                &config,
                &strings(&["sudo", "systemctl", "enable", "--now", service]),
            )
        }
        Command::Run {
            target,
            memory,
            processes,
            image,
            command,
        } => {
            if !valid_reference(image) {
                return Err(format!("invalid image {image:?}"));
            }
            let mut code = 0;
            for step in running(target.engine, image, *memory, *processes, command) {
                code = execute(manager, &config, &step)?;
                if code != 0 {
                    break;
                }
            }
            Ok(code)
        }
        Command::List { target } => {
            let list = match target.engine {
                Engine::Podman => strings(&["podman", "ps"]),
                Engine::Docker => strings(&["sudo", "docker", "ps"]),
                Engine::Containerd => strings(&["sudo", "ctr", "tasks", "list"]),
            };
            execute(manager, &config, &list)
        }
        Command::Stop { target, container } => {
            if !valid_reference(container) {
                return Err(format!("invalid container {container:?}"));
            }
            let mut stop = match target.engine {
                Engine::Podman => strings(&["podman", "stop"]),
                Engine::Docker => strings(&["sudo", "docker", "stop"]),
                Engine::Containerd => strings(&["sudo", "ctr", "tasks", "kill"]),
            };
            stop.push(container.clone());
            execute(manager, &config, &stop)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_not_options() {
        for good in ["alpine", "docker.io/library/alpine:3.22", "a@sha256:00"] {
            assert!(valid_reference(good), "{good}");
        }
        for bad in ["", "--privileged", "a b", "a;b", "$(x)"] {
            assert!(!valid_reference(bad), "{bad}");
        }
    }

    #[test]
    fn containers_are_limited_and_not_privileged() {
        for engine in [Engine::Docker, Engine::Podman, Engine::Containerd] {
            let steps = running(engine, "alpine", 256, 32, &strings(&["true"]));
            let text = steps.last().unwrap().join(" ");
            assert!(text.contains("--rm") && text.ends_with("true"), "{text}");
            assert!(
                text.contains("memory") && !text.contains("privileged"),
                "{text}"
            );
        }
        assert!(!running(Engine::Podman, "x", 1, 1, &[])[0].contains(&"sudo".to_owned()));
    }

    #[test]
    fn engines_come_from_the_distribution() {
        let debian = installation(Engine::Docker, "debian-12").unwrap();
        assert!(debian.contains(&"docker.io".to_owned()));
        let arch = installation(Engine::Docker, "arch").unwrap();
        assert!(arch.contains(&"docker".to_owned()));
        assert!(installation(Engine::Docker, "other").is_none());
    }
}
