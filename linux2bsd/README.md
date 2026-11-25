# linux2BSD

Status: EXPERIMENTAL. Design: [docs/compatibility/linux2bsd.md](../docs/compatibility/linux2bsd.md).

```sh
linux2bsd runtimes
linux2bsd inspect ./program
linux2bsd inspect --json --root /compat/meta/debian ./program
linux2bsd run ./program arguments
```

`inspect` works on any host. `run` needs FreeBSD, and root for programs in an edition's jail.
