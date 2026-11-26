# Containers

Status: EXPERIMENTAL. This document is about compatibility roots, which are FreeBSD jails. Linux
container engines run in [Linux machines](../linux-compatibility/CONTAINER_INTEGRATION.md).

A compatibility root is a ZFS dataset with a Linux distribution's userland, run as a FreeBSD
jail. `meta-compat` manages it:

```sh
meta-compat create debian
meta-compat exec debian -- /bin/bash
meta-compat update debian
meta-compat rollback debian SNAPSHOT --yes
meta-compat remove debian --yes
```

## What a root gets

| Mount | Purpose |
|---|---|
| devfs with ruleset 2600 | Basic devices and terminals only; no disks, no kernel memory |
| fdescfs, tmpfs on `/dev/shm`, linprocfs, linsysfs | What Linux programs expect |
| `/home`, `/tmp` | Shared with the host |
| `/var/run/dbus` as `/run/dbus`, read only | The system bus; services on it authorize callers |
| `/etc/resolv.conf` | Copied from the host at every start |

FreeBSD's own jail ruleset, 4, hides `/dev/shm`, so a separate ruleset is defined.

The jail shares the host's network. It has no child jails, sees only its own mounts, and
reports itself as Linux. Commands run as the user who called sudo, not as root.

A root is for organization, not a sandbox. A program in it can read whatever its user can
read in `/home`.

## Removing a root

**Unmount before deleting.** A root has devfs, linprocfs and the user's home directory
mounted inside it. Deleting the tree while they are mounted deletes through the mounts. This
happened with devfs in a disposable machine during early experiments; with `/home` it would
have destroyed user data.

`meta-compat remove` therefore:

1. finds the dataset mounted exactly at the root, and refuses if there is none,
2. stops the jail,
3. unmounts everything below the root, deepest first, and stops if anything is busy,
4. checks the mount table again,
5. destroys the dataset. It never deletes files, and destroying a dataset cannot follow mounts.

Tested with a process holding the root busy: nothing was removed, and `/home` was intact.

The kernel can keep a stopped jail's root busy for up to a minute, until its last network
connections time out. `meta-compat` waits.

## Open points

| Point | Status |
|---|---|
| Wayland, X11 and audio sockets | RESEARCH REQUIRED |
| GPU access through `/dev/dri` | RESEARCH REQUIRED |
| Networking of its own (VNET) | RESEARCH REQUIRED |
| OCI images with Podman, which FreeBSD packages | RESEARCH REQUIRED |

Linux container engines inside Linuxulator need namespaces and cgroups, which it does not
provide.
