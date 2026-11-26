//! `meta-linux vm`.

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};
use meta_linux::{is_root, print_json, user};
use meta_linux_vm::guest::Options;
use meta_linux_vm::manager::{Change, Finding, Request};
use meta_linux_vm::{Cgroups, Forward, Manager, Network, Share, VmConfig, forward};

use crate::{Failure, exit_code, require_root, user_machine};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum NetworkArgument {
    /// No connections out
    Isolated,
    /// Connections out, including to services on the host's loopback interface
    Open,
}

impl From<NetworkArgument> for Network {
    fn from(argument: NetworkArgument) -> Self {
        match argument {
            NetworkArgument::Isolated => Network::Isolated,
            NetworkArgument::Open => Network::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CgroupArgument {
    V1,
    V2,
}

#[derive(Debug, Args)]
pub struct Create {
    name: String,
    /// Image; see `meta-linux image list`
    #[arg(long, default_value = "debian-12")]
    image: String,
    #[arg(long, default_value_t = 2)]
    cpus: u32,
    /// MiB
    #[arg(long, default_value_t = 2048)]
    memory: u32,
    /// GiB
    #[arg(long, default_value_t = 20)]
    disk: u32,
    #[arg(long, value_enum, default_value = "isolated")]
    network: NetworkArgument,
    /// Control group version the guest starts with
    #[arg(long, value_enum, default_value = "v2")]
    cgroups: CgroupArgument,
    /// User the machine belongs to; defaults to the one who called sudo
    #[arg(long)]
    owner: Option<String>,
    /// Make a guest port reachable on the host's loopback interface
    #[arg(long, value_name = "HOST:GUEST", value_parser = parse_forward)]
    forward: Vec<Forward>,
    /// Share a further directory, at /mnt/host/TAG in the guest
    #[arg(long, value_name = "TAG=DIRECTORY[,ro]", value_parser = parse_share)]
    share: Vec<Share>,
    /// Create it; otherwise only show what would be done
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List machines
    List,
    /// Create a machine from a verified image
    Create(Create),
    /// Start a machine
    Start {
        name: String,
        /// Wait until it has started, for at most this many seconds
        #[arg(long, value_name = "SECONDS", num_args = 0..=1, default_missing_value = "300")]
        wait: Option<u64>,
    },
    /// Ask a machine to power off
    Stop {
        name: String,
        /// Take the power away at once
        #[arg(long)]
        force: bool,
        /// Seconds the guest has to power off before the power is taken away
        #[arg(long, default_value_t = 180)]
        timeout: u64,
    },
    /// Stop and start a machine
    Restart { name: String },
    /// Show a machine, or all of them
    Status { name: Option<String> },
    /// Check a machine and what it depends on
    Diagnose { name: String },
    /// Show the end of a machine's console output
    Console { name: String },
    /// Run a command in a machine; its exit code is passed on
    Exec {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        command: Vec<String>,
    },
    /// Change a stopped machine
    Set {
        name: String,
        #[arg(long)]
        cpus: Option<u32>,
        /// MiB
        #[arg(long)]
        memory: Option<u32>,
        /// GiB; a disk can only grow
        #[arg(long)]
        disk: Option<u32>,
        #[arg(long, value_enum)]
        network: Option<NetworkArgument>,
        /// Replaces all forwarded ports
        #[arg(long, value_name = "HOST:GUEST", value_parser = parse_forward)]
        forward: Option<Vec<Forward>>,
    },
    /// Remove a stopped machine and its disk
    Remove {
        name: String,
        /// Also remove the directory shared with it
        #[arg(long)]
        share: bool,
        /// Confirm; the disk cannot be recovered
        #[arg(long)]
        yes: bool,
    },
    #[command(hide = true)]
    Supervise { name: String },
}

fn parse_forward(text: &str) -> Result<Forward, String> {
    let (host, guest) = text.split_once(':').ok_or("expected HOST:GUEST")?;
    let port = |text: &str| text.parse::<u16>().map_err(|e| e.to_string());
    Ok(Forward {
        host_port: port(host)?,
        guest_port: port(guest)?,
    })
}

fn parse_share(text: &str) -> Result<Share, String> {
    let (tag, rest) = text.split_once('=').ok_or("expected TAG=DIRECTORY")?;
    let (host, read_only) = match rest.strip_suffix(",ro") {
        Some(host) => (host, true),
        None => (rest, false),
    };
    Ok(Share {
        tag: tag.to_owned(),
        host: host.to_owned(),
        read_only,
    })
}

pub fn run(manager: &Manager<'_>, command: Command, json: bool) -> Result<ExitCode, Failure> {
    let caller = user();
    match command {
        Command::List => list(manager, json)?,
        Command::Create(arguments) => return create(manager, arguments, json),
        Command::Start { name, wait } => {
            require_root()?;
            let config = manager.load(&name)?;
            manager.start(&config, &caller)?;
            if let Some(seconds) = wait {
                manager.wait_ready(&config, Duration::from_secs(seconds))?;
            }
            println!(
                "{name} {}",
                if wait.is_some() {
                    "is ready"
                } else {
                    "is starting"
                }
            );
        }
        Command::Stop {
            name,
            force,
            timeout,
        } => {
            require_root()?;
            let grace = (!force).then(|| Duration::from_secs(timeout));
            manager.stop(&manager.load(&name)?, grace, &caller)?;
            println!("{name} is stopped");
        }
        Command::Restart { name } => {
            require_root()?;
            let config = manager.load(&name)?;
            manager.stop(&config, Some(Duration::from_secs(180)), &caller)?;
            manager.start(&config, &caller)?;
            println!("{name} is starting");
        }
        Command::Status { name: None } => list(manager, json)?,
        Command::Status { name: Some(name) } => {
            let status = manager.status(&manager.load(&name)?);
            if json {
                print_json(&status)?;
            } else {
                println!("name      {}", status.name);
                println!("state     {:?}", status.state);
                println!("image     {}", status.image);
                println!("owner     {}", status.owner);
                println!("cpus      {}", status.cpus);
                println!("memory    {} MiB", status.memory_mib);
                println!("disk      {} GiB", status.disk_gib);
                println!("network   {:?}", status.network);
                println!("cgroups   {:?}", status.cgroups);
                println!("ssh       127.0.0.1:{}", status.ssh_port);
                if let Some(last) = status.last_exit {
                    println!("last exit {last}");
                }
            }
        }
        Command::Diagnose { name } => {
            let findings = manager.diagnose(&manager.load(&name)?);
            return show(&findings, json);
        }
        Command::Console { name } => {
            let config = user_machine(manager, Some(&name))?;
            print!("{}", manager.console(&config, 16 << 10)?);
        }
        Command::Exec { name, command } => {
            let config = user_machine(manager, Some(&name))?;
            let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let options = Options {
                terminal,
                ..Options::default()
            };
            let connection = manager.connection(&config);
            return Ok(exit_code(forward::run(
                &connection,
                options,
                None,
                &[],
                &command,
            )?));
        }
        Command::Set {
            name,
            cpus,
            memory,
            disk,
            network,
            forward,
        } => {
            require_root()?;
            let change = Change {
                cpus,
                memory_mib: memory,
                disk_gib: disk,
                network: network.map(Into::into),
                forwards: forward,
            };
            manager.change(&manager.load(&name)?, change, &caller)?;
            println!("{name} is changed");
        }
        Command::Remove { name, share, yes } => {
            require_root()?;
            let config = manager.load(&name)?;
            if !yes {
                println!(
                    "Would destroy {} and its disk.",
                    manager.directory(&name).display()
                );
                let fate = if share { "destroyed too" } else { "kept" };
                println!("The directory shared with it is {fate}.");
                println!("Nothing was changed. Run again with --yes to proceed.");
                return Ok(ExitCode::SUCCESS);
            }
            manager.remove(&config, share, &caller)?;
            println!("{name} is removed");
        }
        Command::Supervise { name } => {
            require_root()?;
            manager.supervise(&manager.load(&name)?)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn list(manager: &Manager<'_>, json: bool) -> Result<(), Failure> {
    let all: Vec<_> = manager
        .list()
        .iter()
        .map(|config| manager.status(config))
        .collect();
    if json {
        print_json(&all)?;
    } else {
        for vm in all {
            let state = format!("{:?}", vm.state).to_lowercase();
            println!("{:24} {state:8} {:12} {}", vm.name, vm.image, vm.owner);
        }
    }
    Ok(())
}

fn create(manager: &Manager<'_>, arguments: Create, json: bool) -> Result<ExitCode, Failure> {
    let owner = match arguments.owner {
        Some(owner) if is_root() => owner,
        _ => user(),
    };
    let request = Request {
        name: arguments.name,
        image: arguments.image,
        cpus: arguments.cpus,
        memory_mib: arguments.memory,
        disk_gib: arguments.disk,
        network: arguments.network.into(),
        cgroups: match arguments.cgroups {
            CgroupArgument::V1 => Cgroups::V1,
            CgroupArgument::V2 => Cgroups::V2,
        },
        owner,
        forwards: arguments.forward,
        shares: arguments.share,
    };
    if !arguments.yes {
        for line in manager.describe(&request)? {
            println!("{line}");
        }
        println!("Nothing was changed. Run again with --yes to proceed.");
        return Ok(ExitCode::SUCCESS);
    }
    require_root()?;
    let config: VmConfig = manager.create(request, &user())?;
    if json {
        print_json(&manager.status(&config))?;
    } else {
        println!(
            "{} is created; start it with: meta-linux vm start {}",
            config.name, config.name
        );
        println!("shared directory: {}", config.shares[0].host);
    }
    Ok(ExitCode::SUCCESS)
}

fn show(findings: &[Finding], json: bool) -> Result<ExitCode, Failure> {
    if json {
        print_json(&findings)?;
    } else {
        for finding in findings {
            let mark = if finding.ok { "ok" } else { "problem" };
            println!("{mark:8} {:18} {}", finding.check, finding.detail);
        }
    }
    let healthy = findings.iter().all(|finding| finding.ok);
    Ok(if healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_parsed() {
        let forward = parse_forward("8080:80").unwrap();
        assert_eq!((forward.host_port, forward.guest_port), (8080, 80));
        assert!(parse_forward("80").is_err() && parse_forward("a:b").is_err());
        let share = parse_share("src=/srv/src,ro").unwrap();
        assert!(share.read_only && share.host == "/srv/src" && share.tag == "src");
        assert!(parse_share("x").is_err());
    }
}
