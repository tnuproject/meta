//! Answers NetworkManager's D-Bus interface on top of FreeBSD's network tools.
//!
//! A prototype: it reports interfaces, addresses, routes and name servers, and
//! connects or disconnects Ethernet interfaces. It is not NetworkManager and
//! contains none of its code.

mod model;
mod service;

use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use meta_common::{Platform, SystemRunner};
use zbus::Connection;
use zbus::fdo::RequestNameFlags;

use crate::model::{DEVICE_ETHERNET, DEVICE_LOOPBACK, Device, Model};
use crate::service::{
    Active, DeviceObject, Generic, Ip4, Ip6, Loopback, MANAGER_PATH, Manager, Profile,
    SETTINGS_PATH, Settings, Shared, Wired, lock,
};

const NAME: &str = "org.freedesktop.NetworkManager";
/// libnm reads every object through an object manager at this path.
const ROOT_PATH: &str = "/org/freedesktop";

#[derive(Parser)]
#[command(
    name = "meta-netd",
    version,
    about = "NetworkManager-compatible service (prototype)"
)]
struct Cli {
    /// Seconds between looking for changes
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=60))]
    interval: u64,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("meta-netd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    Platform::detect(&SystemRunner).require_freebsd("meta-netd")?;
    let shared = Arc::new(Shared {
        runner: Box::new(SystemRunner),
        model: Mutex::new(Model::default()),
        devices: Mutex::new(Vec::new()),
    });
    let connection = zbus::block_on(start(&shared)).map_err(|e| match e {
        zbus::Error::NameTaken => format!("{NAME} is already running"),
        error => error.to_string(),
    })?;
    loop {
        zbus::block_on(refresh(&connection, &shared));
        std::thread::sleep(Duration::from_secs(cli.interval));
    }
}

async fn start(shared: &Arc<Shared>) -> zbus::Result<Connection> {
    let connection = Connection::system().await?;
    let server = connection.object_server();
    server.at(ROOT_PATH, zbus::fdo::ObjectManager).await?;
    server.at(MANAGER_PATH, Manager(Arc::clone(shared))).await?;
    server
        .at(SETTINGS_PATH, Settings(Arc::clone(shared)))
        .await?;
    // Clients see complete objects from the first moment the name exists.
    refresh(&connection, shared).await;
    // Waiting in the queue behind another instance would look like a hang.
    connection
        .request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into())
        .await?;
    Ok(connection)
}

/// Reads FreeBSD's state and publishes what changed.
pub async fn refresh(connection: &Connection, shared: &Arc<Shared>) {
    let status = match meta_net::status(shared.runner.as_ref()) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("meta-netd: {error}");
            return;
        }
    };
    let new = lock(&shared.model).devices(&status);
    let old = std::mem::replace(&mut *lock(&shared.devices), new.clone());
    if old == new {
        return;
    }
    if let Err(error) = publish(connection, shared, &old, &new).await {
        eprintln!("meta-netd: {error}");
    }
}

async fn publish(
    connection: &Connection,
    shared: &Arc<Shared>,
    old: &[Device],
    new: &[Device],
) -> zbus::Result<()> {
    let find = |list: &[Device], index| list.iter().find(|d| d.index == index).cloned();
    for device in new {
        let before = find(old, device.index);
        if before.is_none() {
            add(connection, shared, device).await?;
        }
        let was_active = before.as_ref().is_some_and(Device::active);
        if device.active() && !was_active {
            let active = Active {
                shared: Arc::clone(shared),
                index: device.index,
            };
            connection
                .object_server()
                .at(device.active_path(), active)
                .await?;
        }
        if let Some(before) = before.filter(|before| before != device) {
            service::announce_device(connection, &before, device).await?;
        }
        if !device.active() && was_active {
            connection
                .object_server()
                .remove::<Active, _>(device.active_path())
                .await?;
        }
    }
    for device in old.iter().filter(|d| find(new, d.index).is_none()) {
        remove(connection, device).await?;
    }
    // Before the name is owned nobody listens, and the objects exist already.
    service::announce_manager(connection).await
}

async fn add(connection: &Connection, shared: &Arc<Shared>, device: &Device) -> zbus::Result<()> {
    let server = connection.object_server();
    let index = device.index;
    let shared = || Arc::clone(shared);
    server
        .at(
            device.path(),
            DeviceObject {
                shared: shared(),
                index,
            },
        )
        .await?;
    match device.kind {
        DEVICE_ETHERNET => {
            server
                .at(
                    device.path(),
                    Wired {
                        shared: shared(),
                        index,
                    },
                )
                .await?;
            server
                .at(
                    device.profile_path(),
                    Profile {
                        shared: shared(),
                        index,
                    },
                )
                .await?;
        }
        DEVICE_LOOPBACK => {
            server
                .at(
                    device.path(),
                    Loopback {
                        shared: shared(),
                        index,
                    },
                )
                .await?;
        }
        _ => {
            server
                .at(
                    device.path(),
                    Generic {
                        shared: shared(),
                        index,
                    },
                )
                .await?;
        }
    }
    server
        .at(
            device.ip4_path(),
            Ip4 {
                shared: shared(),
                index,
            },
        )
        .await?;
    server
        .at(
            device.ip6_path(),
            Ip6 {
                shared: shared(),
                index,
            },
        )
        .await?;
    Ok(())
}

async fn remove(connection: &Connection, device: &Device) -> zbus::Result<()> {
    let server = connection.object_server();
    server.remove::<Active, _>(device.active_path()).await?;
    server.remove::<Profile, _>(device.profile_path()).await?;
    server.remove::<Wired, _>(device.path()).await?;
    server.remove::<Loopback, _>(device.path()).await?;
    server.remove::<Generic, _>(device.path()).await?;
    server.remove::<Ip4, _>(device.ip4_path()).await?;
    server.remove::<Ip6, _>(device.ip6_path()).await?;
    server.remove::<DeviceObject, _>(device.path()).await?;
    Ok(())
}
