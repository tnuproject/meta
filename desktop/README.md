# Desktops

Status: EXPERIMENTAL. [desktops.toml](desktops.toml) lists the packages the installer
installs and whether they existed in FreeBSD's repository at the last check. Only XFCE has been
installed and used, in a virtual machine without graphics acceleration.

Check availability on FreeBSD:

```sh
pkg rquery '%n %v' gnome xfce cinnamon i3 sway hyprland
```

Findings from 2026-11-25, FreeBSD 15 amd64 quarterly packages:

- KDE: `plasma6-kwin` and others existed, but `plasma6-plasma`, `plasma6-plasma-desktop` and
  `plasma6-plasma-workspace` did not.
- Wayland sessions need a DRM/KMS driver.

Update integration: [docs/updates/desktop-integration.md](../docs/updates/desktop-integration.md).
