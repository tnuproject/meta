# Desktop update integration

Status: EXPERIMENTAL.

All integrations use the [D-Bus API](dbus-api.md). None of them verifies, downloads or
installs anything itself.

| Desktop | Route | Status |
|---|---|---|
| XFCE | META Update Manager | EXPERIMENTAL |
| Cinnamon, i3, Sway, Hyprland | META Update Manager | PLANNED |
| KDE Plasma | A Discover backend plugin | RESEARCH REQUIRED |
| GNOME | A GNOME Software plugin | RESEARCH REQUIRED |
| Console | `metaupd` | EXPERIMENTAL |

Discover and GNOME Software do not work with metaupd by themselves. Each needs a plugin
written against an interface that changes between versions.

PackageKit is an alternative both already support. It models package updates rather than
system images with reboots and rollback, so it would not represent them properly. RESEARCH
REQUIRED.

## META Update Manager

[desktop/update-manager](../../desktop/update-manager/meta-update-manager), in Python with
GTK 3. It shows the available release, what was rejected and why, the history and the restart
prompt. It never asks for a password; polkit does. At login, `--notify` checks once and shows
a notification.

Python was chosen because a native GTK binding would need a C toolchain in every build of the
workspace.

## Evidence

`tests/vm/suite.sh m10`, in an XFCE session on a virtual display: the window found a signed
release, its Install button put it into a new boot environment, and after a restart it was
confirmed. An unprivileged caller was refused. The button was pressed by an automated key
press, with a polkit rule standing in for the administrator's password.

The notification and the polkit password dialog have not been tested.
