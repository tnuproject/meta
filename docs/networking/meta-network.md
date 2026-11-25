# META Network

Status: RESEARCH REQUIRED. `meta network status` is the only working part; it reads
FreeBSD's tools and changes nothing.

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

The compatible daemon is preferred. Until it exists, installations should use DHCP from
the base system, and desktops can add networkmgr.

### Subset for a first prototype

1. Manager: `GetDevices`, `State`, `ActiveConnections`, `Version`
2. Device and Device.Wired properties
3. IP4Config and IP6Config, read only
4. Device.Wireless: `GetAllAccessPoints`, `RequestScan`
5. Settings: list, add, delete, for wired and WPA-PSK
6. `ActivateConnection`, `DeactivateConnection`

`nmcli device status` exercises steps 1 to 3 and is M4's exit criterion.

### Feature feasibility

| Feature | FreeBSD basis | Status |
|---|---|---|
| Ethernet, DHCP | ifconfig, dhclient | PLANNED |
| Wi-Fi client | net80211, wpa_supplicant | RESEARCH REQUIRED |
| DNS | resolvconf, local_unbound | PLANNED |
| WireGuard | if_wg | PLANNED |
| OpenVPN, IPsec | if_ovpn, strongSwan | RESEARCH REQUIRED |
| Hotspot | hostapd, pf; depends on the driver | RESEARCH REQUIRED |
| Bluetooth networking | Netgraph; tethering unclear | RESEARCH REQUIRED |
| Connectivity checks | fetch | PLANNED |

### Authorization

Changes go through polkit with NetworkManager's action names, so existing desktop policy
applies. Wi-Fi secrets are readable by root only.
