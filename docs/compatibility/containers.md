# Containers

Status: PLANNED.

Containers on META are FreeBSD jails. A compatibility root is a jail with its own Linux
version settings.

[jails/meta-compat.conf.in](../../jails/meta-compat.conf.in) is a template. With a Debian
12 root it started on FreeBSD 15.1-RELEASE-p7, and `uname` inside it identified itself as
Linux:

```
Linux meta-debian 5.15.0 FreeBSD 15.1-RELEASE-p7 ... x86_64 GNU/Linux
```

## Open points

| Point | Status |
|---|---|
| Mounting `/proc`, `/sys` and `/dev/shm` inside the jail | PLANNED |
| Sharing Wayland, X11, D-Bus and audio sockets | RESEARCH REQUIRED |
| GPU access through `/dev/dri` and a devfs ruleset | RESEARCH REQUIRED |
| Networking: shared stack or VNET | RESEARCH REQUIRED |
| OCI images with Podman, which FreeBSD packages | RESEARCH REQUIRED |

Linux container engines inside Linuxulator need namespaces and cgroups, which it does not
provide.

## Removing a root

**Unmount before deleting.** A root has devfs, linprocfs and possibly the user's home
directory mounted inside it. Deleting the tree while they are mounted deletes through
the mounts. This happened with devfs during testing in a disposable machine; with `/home`
it would have destroyed user data.

Any META tool that removes a root must:

1. stop the jail,
2. unmount everything below the root, deepest first,
3. confirm from the mount table that nothing is mounted there,
4. destroy the root's ZFS dataset instead of deleting files.

Giving every root its own dataset makes step 4 safe, because destroying a dataset cannot
follow mounts.
