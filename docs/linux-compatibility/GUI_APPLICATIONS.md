# Graphical Linux programs

| Way | Level | State | Status |
|---|---|---|---|
| X11 socket shared with Linuxulator | 1 | Programs connect to the host's X server directly; not tested by META | RESEARCH REQUIRED |
| X11 socket shared with a jail | 2 | `/tmp` is shared, so the socket is visible; not tested by META | RESEARCH REQUIRED |
| X11 over SSH | 3 | Works without acceleration, `--x11` | EXPERIMENTAL |
| Wayland with waypipe | 3 | Needs waypipe on both sides and a Wayland compositor on the host | RESEARCH REQUIRED |
| VNC to bhyve's framebuffer | 3 | A whole desktop in a window; bhyve offers it, META does not | RESEARCH REQUIRED |
| RDP or PipeWire remoting | 3 | Same class as waypipe, for GNOME and KDE | RESEARCH REQUIRED |
| virtio-gpu | 3 | Not in bhyve; needs upstream work | RESEARCH REQUIRED |
| GPU passthrough | 3 | Needs an IOMMU and a second GPU; weakens isolation | RESEARCH REQUIRED |

```sh
meta-linux vm exec work -- sudo apt-get install -y xauth
meta-exec run --x11 --runtime vm ./program
```

A forwarded X11 program is a full client of the display. It can read the keyboard and the
contents of other windows. SSH's untrusted mode, `-X`, restricts this through the X SECURITY
extension; what that stops depends on the server. Use a separate X server, such as Xephyr, for
programs that are not trusted.

The same file for a desktop: bhyve's `fbuf` needs the guest to have a desktop installed, and
gives a second desktop rather than windows of the first.
