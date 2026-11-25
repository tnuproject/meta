//! D-Bus objects.
//!
//! Each object looks its device up in the shared snapshot when asked, so there is
//! one source of truth. Unknown devices answer with empty values rather than
//! failing, because a client may ask in the moment a device disappears.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use meta_common::CommandRunner;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, fdo, interface};

use crate::model::{self, Device, Model, NO_OBJECT};

/// The NetworkManager API level this service follows.
pub const API_VERSION: &str = "1.42.0";
pub const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
pub const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const ACTION_CONTROL: &str = "org.freedesktop.NetworkManager.network-control";

type Dict = HashMap<String, OwnedValue>;

pub struct Shared {
    pub runner: Box<dyn CommandRunner + Send + Sync>,
    pub model: Mutex<Model>,
    pub devices: Mutex<Vec<Device>>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("devices", &self.devices)
            .finish()
    }
}

/// A poisoned lock only means another request panicked; the data is still valid.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    fn device(&self, index: u32) -> Option<Device> {
        lock(&self.devices)
            .iter()
            .find(|device| device.index == index)
            .cloned()
    }

    fn device_at(&self, path: &str) -> Option<Device> {
        lock(&self.devices)
            .iter()
            .find(|device| device.path() == path)
            .cloned()
    }

    fn all(&self) -> Vec<Device> {
        lock(&self.devices).clone()
    }

    fn primary(&self) -> Option<Device> {
        self.all()
            .into_iter()
            .find(|d| d.active() && d.gateway4.is_some())
    }
}

fn path(text: impl AsRef<str>) -> OwnedObjectPath {
    OwnedObjectPath::try_from(text.as_ref()).unwrap_or_default()
}

fn value<'a>(value: impl Into<Value<'a>>) -> OwnedValue {
    OwnedValue::try_from(value.into()).unwrap_or_else(|_| OwnedValue::from(0u32))
}

fn unknown_device() -> fdo::Error {
    fdo::Error::Failed("unknown device".into())
}

#[derive(Debug, Clone)]
pub struct Manager(pub Arc<Shared>);

impl Manager {
    async fn disconnect_device(
        &self,
        device: &Device,
        connection: &Connection,
        header: &Header<'_>,
    ) -> fdo::Result<()> {
        meta_dbus::authorize(connection, header, ACTION_CONTROL)
            .await
            .map_err(fdo::Error::AccessDenied)?;
        if !device.managed() || !model::valid_interface(&device.interface.name) {
            return Err(fdo::Error::NotSupported(
                "this device is not managed".into(),
            ));
        }
        lock(&self.0.model)
            .disconnected
            .insert(device.interface.name.clone());
        model::disconnect(self.0.runner.as_ref(), device)
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        crate::refresh(connection, &self.0).await;
        Ok(())
    }
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices()
    }

    fn get_all_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices()
    }

    fn get_device_by_ip_iface(&self, iface: &str) -> fdo::Result<OwnedObjectPath> {
        self.0
            .all()
            .into_iter()
            .find(|device| device.interface.name == iface)
            .map(|device| path(device.path()))
            .ok_or_else(unknown_device)
    }

    fn get_permissions(&self) -> HashMap<String, String> {
        // What polkit will be asked; clients use it to grey out controls.
        HashMap::from([(ACTION_CONTROL.to_owned(), "auth".to_owned())])
    }

    #[zbus(name = "state")]
    fn state_method(&self) -> u32 {
        self.state()
    }

    fn check_connectivity(&self) -> u32 {
        self.connectivity()
    }

    fn get_logging(&self) -> (String, String) {
        ("INFO".to_owned(), "CORE".to_owned())
    }

    async fn activate_connection(
        &self,
        profile: OwnedObjectPath,
        device: OwnedObjectPath,
        _specific_object: OwnedObjectPath,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        meta_dbus::authorize(connection, &header, ACTION_CONTROL)
            .await
            .map_err(fdo::Error::AccessDenied)?;
        // Either may be "/", meaning "choose".
        let device = self
            .0
            .all()
            .into_iter()
            .find(|d| d.path() == device.as_str() || d.profile_path() == profile.as_str())
            .ok_or_else(unknown_device)?;
        let name = device.interface.name.clone();
        if !device.managed() || !model::valid_interface(&name) {
            return Err(fdo::Error::NotSupported(
                "this device is not managed".into(),
            ));
        }
        {
            let mut model = lock(&self.0.model);
            model.disconnected.remove(&name);
            model.connecting.insert(name.clone());
        }
        crate::refresh(connection, &self.0).await;
        // Obtaining an address takes seconds; the caller watches the state.
        let shared = Arc::clone(&self.0);
        let connection = connection.clone();
        std::thread::spawn(move || {
            if let Err(error) = model::connect(shared.runner.as_ref(), &name) {
                eprintln!("meta-netd: {name}: {error}");
            }
            lock(&shared.model).connecting.remove(&name);
            zbus::block_on(crate::refresh(&connection, &shared));
        });
        Ok(path(device.active_path()))
    }

    async fn deactivate_connection(
        &self,
        active: OwnedObjectPath,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let device = self
            .0
            .all()
            .into_iter()
            .find(|d| d.active_path() == active.as_str())
            .ok_or_else(unknown_device)?;
        self.disconnect_device(&device, connection, &header).await
    }

    #[zbus(signal, name = "StateChanged")]
    async fn state_changed_signal(emitter: &SignalEmitter<'_>, state: u32) -> zbus::Result<()>;

    #[zbus(property)]
    pub fn devices(&self) -> Vec<OwnedObjectPath> {
        self.0.all().iter().map(|d| path(d.path())).collect()
    }

    #[zbus(property)]
    fn all_devices(&self) -> Vec<OwnedObjectPath> {
        self.devices()
    }

    #[zbus(property)]
    fn checkpoints(&self) -> Vec<OwnedObjectPath> {
        Vec::new()
    }

    #[zbus(property)]
    fn active_connections(&self) -> Vec<OwnedObjectPath> {
        self.0
            .all()
            .iter()
            .filter(|d| d.active())
            .map(|d| path(d.active_path()))
            .collect()
    }

    #[zbus(property)]
    fn primary_connection(&self) -> OwnedObjectPath {
        self.0
            .primary()
            .map_or(path(NO_OBJECT), |d| path(d.active_path()))
    }

    #[zbus(property)]
    fn primary_connection_type(&self) -> String {
        self.0
            .primary()
            .map_or_else(String::new, |_| "802-3-ethernet".to_owned())
    }

    #[zbus(property)]
    fn activating_connection(&self) -> OwnedObjectPath {
        self.0
            .all()
            .iter()
            .find(|d| d.state == model::STATE_IP_CONFIG)
            .map_or(path(NO_OBJECT), |d| path(d.active_path()))
    }

    #[zbus(property)]
    fn networking_enabled(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn wireless_enabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn wireless_hardware_enabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn wwan_enabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn wwan_hardware_enabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn radio_flags(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn metered(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn startup(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn version(&self) -> String {
        API_VERSION.to_owned()
    }

    #[zbus(property)]
    fn capabilities(&self) -> Vec<u32> {
        Vec::new()
    }

    /// 70 is connected, 20 disconnected, 40 connecting.
    #[zbus(property)]
    pub fn state(&self) -> u32 {
        let devices = self.0.all();
        if devices.iter().any(|d| d.state == model::STATE_ACTIVATED) {
            70
        } else if devices.iter().any(|d| d.state == model::STATE_IP_CONFIG) {
            40
        } else {
            20
        }
    }

    /// Nothing checks for a captive portal, so a default route counts as full.
    #[zbus(property)]
    pub fn connectivity(&self) -> u32 {
        if self.0.primary().is_some() { 4 } else { 1 }
    }

    #[zbus(property)]
    fn connectivity_check_available(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn connectivity_check_enabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn connectivity_check_uri(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn global_dns_configuration(&self) -> Dict {
        Dict::new()
    }
}

#[derive(Debug, Clone)]
pub struct Settings(pub Arc<Shared>);

#[interface(name = "org.freedesktop.NetworkManager.Settings")]
impl Settings {
    fn list_connections(&self) -> Vec<OwnedObjectPath> {
        self.connections()
    }

    fn get_connection_by_uuid(&self, uuid: &str) -> fdo::Result<OwnedObjectPath> {
        self.0
            .all()
            .iter()
            .find(|d| d.managed() && d.profile_uuid() == uuid)
            .map(|d| path(d.profile_path()))
            .ok_or_else(|| fdo::Error::Failed("unknown connection".into()))
    }

    #[zbus(property)]
    pub fn connections(&self) -> Vec<OwnedObjectPath> {
        self.0
            .all()
            .iter()
            .filter(|d| d.managed())
            .map(|d| path(d.profile_path()))
            .collect()
    }

    #[zbus(property)]
    fn hostname(&self) -> String {
        meta_common::platform::sysctl(self.0.runner.as_ref(), "kern.hostname").unwrap_or_default()
    }

    /// Profiles are derived from the interfaces and cannot be edited yet.
    #[zbus(property)]
    fn can_modify(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn version_id(&self) -> u64 {
        1
    }
}

macro_rules! device_object {
    ($name:ident) => {
        #[derive(Debug, Clone)]
        pub struct $name {
            pub shared: Arc<Shared>,
            pub index: u32,
        }

        impl $name {
            fn read<T: Default>(&self, read: impl FnOnce(&Device) -> T) -> T {
                self.shared
                    .device(self.index)
                    .map_or_else(T::default, |device| read(&device))
            }
        }
    };
}

device_object!(Profile);
device_object!(DeviceObject);
device_object!(Wired);
device_object!(Loopback);
device_object!(Generic);
device_object!(Active);
device_object!(Ip4);
device_object!(Ip6);

#[interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl Profile {
    fn get_settings(&self) -> HashMap<String, Dict> {
        self.read(|d| {
            let automatic = || Dict::from([("method".to_owned(), value("auto"))]);
            HashMap::from([
                (
                    "connection".to_owned(),
                    Dict::from([
                        ("id".to_owned(), value(d.profile_id())),
                        ("uuid".to_owned(), value(d.profile_uuid())),
                        ("type".to_owned(), value("802-3-ethernet")),
                        ("interface-name".to_owned(), value(d.interface.name.clone())),
                    ]),
                ),
                ("802-3-ethernet".to_owned(), Dict::new()),
                ("ipv4".to_owned(), automatic()),
                ("ipv6".to_owned(), automatic()),
            ])
        })
    }

    #[zbus(property)]
    fn unsaved(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn flags(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn filename(&self) -> String {
        String::new()
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl DeviceObject {
    async fn disconnect(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let device = self.shared.device(self.index).ok_or_else(unknown_device)?;
        Manager(Arc::clone(&self.shared))
            .disconnect_device(&device, connection, &header)
            .await
    }

    #[zbus(signal, name = "StateChanged")]
    async fn state_changed_signal(
        emitter: &SignalEmitter<'_>,
        new: u32,
        old: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn udi(&self) -> String {
        self.read(|d| format!("/freebsd/net/{}", d.interface.name))
    }

    #[zbus(property)]
    fn path(&self) -> String {
        self.read(|d| d.interface.name.clone())
    }

    #[zbus(property)]
    fn interface(&self) -> String {
        self.read(|d| d.interface.name.clone())
    }

    #[zbus(property)]
    fn ip_interface(&self) -> String {
        self.read(|d| {
            if d.active() {
                d.interface.name.clone()
            } else {
                String::new()
            }
        })
    }

    #[zbus(property)]
    fn driver(&self) -> String {
        self.read(|d| d.driver.clone())
    }

    #[zbus(property)]
    fn driver_version(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn firmware_version(&self) -> String {
        String::new()
    }

    /// 1: supported, 2: carrier detection.
    #[zbus(property)]
    fn capabilities(&self) -> u32 {
        self.read(|d| if d.managed() { 3 } else { 1 })
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.read(|d| d.state)
    }

    #[zbus(property)]
    fn state_reason(&self) -> (u32, u32) {
        (self.state(), 0)
    }

    #[zbus(property)]
    fn active_connection(&self) -> OwnedObjectPath {
        let active = self.read(|d| d.active().then(|| d.active_path()));
        path(active.unwrap_or_else(|| NO_OBJECT.to_owned()))
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        let found = self.read(|d| (!d.addresses(false).is_empty()).then(|| d.ip4_path()));
        path(found.unwrap_or_else(|| NO_OBJECT.to_owned()))
    }

    #[zbus(property)]
    fn dhcp4_config(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }

    #[zbus(property)]
    fn ip6_config(&self) -> OwnedObjectPath {
        let found = self.read(|d| (!d.addresses(true).is_empty()).then(|| d.ip6_path()));
        path(found.unwrap_or_else(|| NO_OBJECT.to_owned()))
    }

    #[zbus(property)]
    fn dhcp6_config(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }

    #[zbus(property)]
    fn managed(&self) -> bool {
        self.read(Device::managed)
    }

    #[zbus(property)]
    fn autoconnect(&self) -> bool {
        self.read(Device::managed)
    }

    #[zbus(property)]
    fn firmware_missing(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn nm_plugin_missing(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn device_type(&self) -> u32 {
        self.read(|d| d.kind)
    }

    #[zbus(property)]
    fn available_connections(&self) -> Vec<OwnedObjectPath> {
        self.read(|d| {
            if d.managed() {
                vec![path(d.profile_path())]
            } else {
                Vec::new()
            }
        })
    }

    #[zbus(property)]
    fn physical_port_id(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn mtu(&self) -> u32 {
        self.read(|d| d.interface.mtu.unwrap_or(0))
    }

    #[zbus(property)]
    fn metered(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn real(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn ip4_connectivity(&self) -> u32 {
        self.read(|d| if d.gateway4.is_some() { 4 } else { 1 })
    }

    #[zbus(property)]
    fn ip6_connectivity(&self) -> u32 {
        self.read(|d| if d.gateway6.is_some() { 4 } else { 1 })
    }

    /// 1: up, 2: lower up, 65536: carrier.
    #[zbus(property)]
    fn interface_flags(&self) -> u32 {
        self.read(|d| {
            let mut flags = 0;
            if d.interface.up {
                flags |= 1;
            }
            if d.carrier() {
                flags |= 2 | 65536;
            }
            flags
        })
    }

    #[zbus(property)]
    fn hw_address(&self) -> String {
        self.read(|d| d.interface.ether.clone().unwrap_or_default().to_uppercase())
    }

    #[zbus(property)]
    fn ports(&self) -> Vec<OwnedObjectPath> {
        Vec::new()
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Wired")]
impl Wired {
    #[zbus(property)]
    fn hw_address(&self) -> String {
        self.read(|d| d.interface.ether.clone().unwrap_or_default().to_uppercase())
    }

    #[zbus(property)]
    fn perm_hw_address(&self) -> String {
        self.hw_address()
    }

    /// Unknown: ifconfig(8) reports a media name, not a number.
    #[zbus(property)]
    fn speed(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn s390_subchannels(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn carrier(&self) -> bool {
        self.read(Device::carrier)
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Loopback")]
impl Loopback {
    #[zbus(property)]
    fn hw_address(&self) -> String {
        self.read(|d| d.interface.ether.clone().unwrap_or_default())
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device.Generic")]
impl Generic {
    #[zbus(property)]
    fn hw_address(&self) -> String {
        self.read(|d| d.interface.ether.clone().unwrap_or_default().to_uppercase())
    }

    #[zbus(property)]
    fn type_description(&self) -> String {
        self.read(|d| d.driver.clone())
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(signal, name = "StateChanged")]
    async fn state_changed_signal(
        emitter: &SignalEmitter<'_>,
        state: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn connection(&self) -> OwnedObjectPath {
        path(self.read(Device::profile_path))
    }

    #[zbus(property)]
    fn specific_object(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.read(Device::profile_id)
    }

    #[zbus(property)]
    fn uuid(&self) -> String {
        self.read(Device::profile_uuid)
    }

    #[zbus(property, name = "Type")]
    fn kind(&self) -> String {
        "802-3-ethernet".to_owned()
    }

    #[zbus(property)]
    fn devices(&self) -> Vec<OwnedObjectPath> {
        self.read(|d| vec![path(d.path())])
    }

    #[zbus(property)]
    fn state(&self) -> u32 {
        self.read(|d| match d.state {
            model::STATE_ACTIVATED => model::ACTIVE_ACTIVATED,
            _ => model::ACTIVE_ACTIVATING,
        })
    }

    #[zbus(property)]
    fn state_flags(&self) -> u32 {
        0
    }

    #[zbus(property, name = "Default")]
    fn is_default(&self) -> bool {
        self.read(|d| d.gateway4.is_some())
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        path(self.read(Device::ip4_path))
    }

    #[zbus(property)]
    fn dhcp4_config(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }

    #[zbus(property)]
    fn default6(&self) -> bool {
        self.read(|d| d.gateway6.is_some())
    }

    #[zbus(property)]
    fn ip6_config(&self) -> OwnedObjectPath {
        path(self.read(Device::ip6_path))
    }

    #[zbus(property)]
    fn dhcp6_config(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }

    #[zbus(property)]
    fn vpn(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn controller(&self) -> OwnedObjectPath {
        path(NO_OBJECT)
    }
}

fn address_data(addresses: Vec<(String, u32)>) -> Vec<Dict> {
    addresses
        .into_iter()
        .map(|(address, prefix)| {
            Dict::from([
                ("address".to_owned(), value(address)),
                ("prefix".to_owned(), value(prefix)),
            ])
        })
        .collect()
}

fn nameserver_data(device: &Device, v6: bool) -> Vec<Dict> {
    device
        .nameservers
        .iter()
        .filter(|server| server.contains(':') == v6)
        .map(|server| Dict::from([("address".to_owned(), value(server.clone()))]))
        .collect()
}

#[interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4 {
    #[zbus(property)]
    fn address_data(&self) -> Vec<Dict> {
        address_data(self.read(|d| d.addresses(false)))
    }

    #[zbus(property)]
    fn gateway(&self) -> String {
        self.read(|d| d.gateway4.clone().unwrap_or_default())
    }

    #[zbus(property)]
    fn route_data(&self) -> Vec<Dict> {
        Vec::new()
    }

    #[zbus(property)]
    fn nameserver_data(&self) -> Vec<Dict> {
        self.read(|d| nameserver_data(d, false))
    }

    #[zbus(property)]
    fn domains(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn searches(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn dns_priority(&self) -> i32 {
        100
    }
}

#[interface(name = "org.freedesktop.NetworkManager.IP6Config")]
impl Ip6 {
    #[zbus(property)]
    fn address_data(&self) -> Vec<Dict> {
        address_data(self.read(|d| d.addresses(true)))
    }

    #[zbus(property)]
    fn gateway(&self) -> String {
        self.read(|d| d.gateway6.clone().unwrap_or_default())
    }

    #[zbus(property)]
    fn route_data(&self) -> Vec<Dict> {
        Vec::new()
    }

    #[zbus(property)]
    fn domains(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn searches(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn dns_priority(&self) -> i32 {
        100
    }
}

/// Tells clients that a device changed state.
pub async fn announce_device(
    connection: &Connection,
    old: &Device,
    new: &Device,
) -> zbus::Result<()> {
    let device = connection
        .object_server()
        .interface::<_, DeviceObject>(new.path())
        .await?;
    let emitter = device.signal_emitter();
    let object = device.get().await;
    object.state_changed(emitter).await?;
    object.state_reason_changed(emitter).await?;
    object.active_connection_changed(emitter).await?;
    object.ip_interface_changed(emitter).await?;
    object.ip4_config_changed(emitter).await?;
    object.ip6_config_changed(emitter).await?;
    object.interface_flags_changed(emitter).await?;
    if old.state != new.state {
        DeviceObject::state_changed_signal(emitter, new.state, old.state, 0).await?;
    }
    // nmcli waits on the connection, not on the device.
    if new.active() && old.active() && old.state != new.state {
        let active = connection
            .object_server()
            .interface::<_, Active>(new.active_path())
            .await?;
        let emitter = active.signal_emitter();
        let object = active.get().await;
        object.state_changed(emitter).await?;
        object.ip4_config_changed(emitter).await?;
        object.default_changed(emitter).await?;
        Active::state_changed_signal(emitter, object.state(), 0).await?;
    }
    Ok(())
}

/// Tells clients that the set of devices or connections changed.
pub async fn announce_manager(connection: &Connection) -> zbus::Result<()> {
    let manager = connection
        .object_server()
        .interface::<_, Manager>(MANAGER_PATH)
        .await?;
    let emitter = manager.signal_emitter();
    let object = manager.get().await;
    object.devices_changed(emitter).await?;
    object.all_devices_changed(emitter).await?;
    object.active_connections_changed(emitter).await?;
    object.primary_connection_changed(emitter).await?;
    object.primary_connection_type_changed(emitter).await?;
    object.activating_connection_changed(emitter).await?;
    object.connectivity_changed(emitter).await?;
    object.state_changed(emitter).await?;
    Manager::state_changed_signal(emitter, object.state()).await?;
    let settings = connection
        .object_server()
        .interface::<_, Settings>(SETTINGS_PATH)
        .await?;
    settings
        .get()
        .await
        .connections_changed(settings.signal_emitter())
        .await
}

impl Shared {
    /// Used by tests and by callers that only have a path.
    pub fn knows(&self, path: &str) -> bool {
        self.device_at(path).is_some()
    }
}
