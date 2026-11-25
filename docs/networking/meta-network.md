# META Network

Status: EXPERIMENTAL for wired networks; RESEARCH REQUIRED for the rest.

`meta-netd` answers a subset of NetworkManager's D-Bus interface with FreeBSD's state.

## Goal

A desktop user should find Wi-Fi, wired, VPN and hotspot management that works like
NetworkManager, with the settings panels desktops already have.

## Two separate things

NetworkManager is userspace. It configures the Linux kernel through netlink, udev,
wpa_supplicant and others. The Linux kernel's network stack is not part of it, and META
keeps FreeBSD's stack. The question is who speaks NetworkManager's D-Bus interface on top
of FreeBSD.

## State of upstream, checked 2026-11-25

- There is no working NetworkManager port for FreeBSD. `net-mgmt/networkmanager-shim` only
  satisfies build dependencies.
- A daemon that exports NetworkManager's D-Bus interface on FreeBSD was
  [proposed](https://lists.freebsd.org/archives/freebsd-desktop/2025-September/006829.html)
  on the freebsd-desktop list in September 2025. META should join that discussion rather
  than start a competing one.
- `networkmgr` 6.9 and `wifimgr` 1.26 are packaged and work without D-Bus.

## Design

```
 Plasma, GNOME, nmcli, nmtui
            |  org.freedesktop.NetworkManager on D-Bus
       meta-netd                     one process, privileged
            |
   .--------+---------+-----------+-------------.
 ifconfig  route   dhclient   wpa_supplicant   resolvconf
 ioctl     socket             control socket   local_unbound
```

`meta-netd` would implement a subset of NetworkManager's interface over FreeBSD facilities.
Clients such as `nmcli` and `nmtui` come from NetworkManager itself and would have to be
built without its daemon.

### Options

| Option | For | Against |
|---|---|---|
| Port NetworkManager | Complete behaviour | Deep Linux dependencies; upstream declined FreeBSD support before |
| Compatible daemon, `meta-netd` | Native backend; desktops keep working | Large interface; behaviour must match closely |
| Use networkmgr or wifimgr | Exists today | No desktop integration |

The compatible daemon was chosen. It contains no NetworkManager code.

### Subset

Steps 1 to 3 and 6 are implemented for Ethernet; 4 and 5 are PLANNED.

1. Manager: `GetDevices`, `State`, `ActiveConnections`, `Version`
2. Device and Device.Wired properties
3. IP4Config and IP6Config, read only
4. Device.Wireless: `GetAllAccessPoints`, `RequestScan`
5. Settings: list, add, delete, for wired and WPA-PSK
6. `ActivateConnection`, `DeactivateConnection`

`nmcli device status` exercises steps 1 to 3 and is M4's exit criterion.

### Behaviour

- Every interface is a device. Ethernet devices have one automatic profile derived from the
  interface; loopback and everything else is shown as unmanaged.
- State is read every three seconds, so changes made with `ifconfig` appear too.
- Connecting brings the interface up and runs dhclient; disconnecting releases the address.
  Neither survives a reboot; `rc.conf` is not changed.
- Wireless interfaces are shown as generic devices until there is hardware to test with.
- Link speed is reported as unknown, and connectivity as full when there is a default route.
  Nothing checks for captive portals.

### Evidence

`tests/vm/suite.sh m4` runs nmcli 1.42.4 from a Debian root against it: status, addresses,
gateway and name server compared with FreeBSD's, profiles, interfaces appearing and
disappearing, disconnecting, connecting with DHCP, and refusal of an unprivileged user.
Desktop network panels have not been tested.

### Feature feasibility

| Feature | FreeBSD basis | Status |
|---|---|---|
| Ethernet, DHCP | ifconfig, dhclient | EXPERIMENTAL |
| Wi-Fi client | net80211, wpa_supplicant | RESEARCH REQUIRED |
| DNS | resolvconf, local_unbound | PLANNED |
| WireGuard | if_wg | PLANNED |
| OpenVPN, IPsec | if_ovpn, strongSwan | RESEARCH REQUIRED |
| Hotspot | hostapd, pf; depends on the driver | RESEARCH REQUIRED |
| Bluetooth networking | Netgraph; tethering unclear | RESEARCH REQUIRED |
| Connectivity checks | fetch | PLANNED |

### Authorization

Changes go through polkit with NetworkManager's action names, so existing administrator rules
apply. Reading is open to everyone, as with NetworkManager.
