# Linux containers

Status: EXPERIMENTAL. Written and unit-tested; not yet run in a guest, because installing an
engine needs the open network, which failed in the tests.

Linux container engines need namespaces, cgroups, overlayfs and netfilter, which FreeBSD does
not have. They run inside a machine, on its kernel. They are not FreeBSD jails; `meta-compat`'s
roots are.

```sh
sudo meta-linux vm set --network open work     # stopped machine; images are downloaded
meta-linux container install --engine podman
meta-linux container run docker.io/library/alpine:3.22 cat /etc/os-release
```

| Engine | Package in Debian, Arch | Runs as, in the guest | Image format |
|---|---|---|---|
| Podman | `podman` | The owner, rootless | OCI, Docker |
| Docker | `docker.io`, `docker` | root | OCI, Docker |
| containerd | `containerd` | root, through `ctr` | OCI, Docker |

Every container is removed when it ends and gets a memory limit (1024 MiB) and, except with
containerd, a process limit (512). Other options of an engine are available with
`meta-linux vm exec`.

The exit code is the container's. Ports: forward one to the machine, and publish it there.

| Open point | Status |
|---|---|
| Building images, volumes from shares | RESEARCH REQUIRED |
| Compose files | PLANNED |
| Podman on the host, with a machine as remote | RESEARCH REQUIRED |
| Kubernetes | RESEARCH REQUIRED |
