# Desktop update integration

Status: PLANNED.

All integrations use the [D-Bus API](dbus-api.md). None of them verifies, downloads or
installs anything itself.

| Desktop | Route | Status |
|---|---|---|
| KDE Plasma | A Discover backend plugin | RESEARCH REQUIRED |
| GNOME | A GNOME Software plugin | RESEARCH REQUIRED |
| XFCE, Cinnamon | META Update Manager | PLANNED |
| i3, Sway, Hyprland | META Update Manager, or a notification | PLANNED |
| Console | `metaupd` | EXPERIMENTAL |

Discover and GNOME Software do not work with metaupd by themselves. Each needs a plugin
written against an interface that changes between versions and must be rebuilt with them.

PackageKit is an alternative both already support. It models package updates rather than
system images with reboots and rollback, so it would show updates but not represent them
properly. RESEARCH REQUIRED.

## META Update Manager

A small application for desktops without a native integration. It shows the available
release and changelog, what rollback does not cover, progress, the restart prompt and boot
environments. It must work without a network connection and never ask for a password
itself; polkit does.
