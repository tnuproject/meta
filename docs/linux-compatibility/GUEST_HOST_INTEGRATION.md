# Guest and host

| What | How | Status |
|---|---|---|
| Commands | SSH on the loopback interface, pinned keys | IMPLEMENTED |
| Signals | A second connection, to a process recorded by the first | IMPLEMENTED |
| Files | 9p, `/var/meta-linux/share/NAME` and `/mnt/host/share` | IMPLEMENTED |
| Further directories | `--share TAG=DIR`, at `/mnt/host/TAG` | EXPERIMENTAL |
| Network out | `--network open`, user-mode NAT; see [limitations](../compatibility/KNOWN_LIMITATIONS.md#machines) | EXPERIMENTAL |
| Ports in | `--forward HOST:GUEST`, loopback only | EXPERIMENTAL |
| Identity | The owner's name and number exist in the guest | IMPLEMENTED |
| Readiness | cloud-init and systemd, over SSH | IMPLEMENTED |
| Power | ACPI power button | EXPERIMENTAL |
| Entropy | virtio-rnd | EXPERIMENTAL |
| X11 | SSH forwarding, with `--x11` | EXPERIMENTAL |
| Clipboard | What X11 forwarding carries | EXPERIMENTAL |
| Audio | bhyve's `hda`, not configured | RESEARCH REQUIRED |
| Wayland | waypipe over SSH | RESEARCH REQUIRED |
| Time | Guest's own NTP client, with an open network | EXPERIMENTAL |
| USB, GPU, other devices | Not offered | RESEARCH REQUIRED |
| Memory ballooning | bhyve has none | RESEARCH REQUIRED |

bhyve has no vsock. Its virtio-console could carry a private channel to an agent, which would
remove the loopback port; SSH was chosen because nothing has to be installed in the guest.

## Files

The guest's root may change files in the share, which then belong to the machine's owner on
the host. Set-user-ID programs do not work from the share on either side. Changes on the
host are seen by the guest; inotify does not report them.

A program is run where it lies when it is in a share, and copied to `~/.cache/meta-run`
otherwise. The working directory is translated in the same way.

## Host directories

`--share` accepts a directory only when its filesystem is mounted `nosuid` and it belongs to
the owner. On ZFS:

```sh
sudo zfs create -o setuid=off -o devices=off -o mountpoint=/srv/src zroot/src
sudo chown alice /srv/src
sudo meta-linux vm create --share src=/srv/src --yes work
```
