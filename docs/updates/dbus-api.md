# metaupd D-Bus API

Status: EXPERIMENTAL. `metaupdd` implements the
[published interface](../../metaupd/dbus/io.github.tnuproject.MetaUpd1.xml); a test compares
the two.

| Item | Value |
|---|---|
| Bus | system |
| Name | `io.github.tnuproject.MetaUpd1` |
| Object | `/io/github/tnuproject/MetaUpd1` |
| Service | `metaupdd`, enabled with `sysrc metaupdd_enable=YES` |

## Behaviour

- `Check`, `Download` and `Install` return a job object at once. The job emits `Finished`
  and disappears. Progress is always -1; PLANNED.
- One operation that changes state runs at a time. Others fail with `.Error.Busy`.
- The service uses the same engine as the command line tool, with the same lock.
- `Check` reports warnings and rejected releases on further lines of its message, so a
  client can say that something was refused instead of "up to date".
- `Cancel` does nothing yet and is therefore not offered.

## Authorization

The caller is identified by its bus name, which it cannot choose. Root is always allowed;
everyone else is checked with polkit, and no answer means no.

| Action | Active local user | Others |
|---|---|---|
| `check`, `download` | allowed | administrator |
| `install`, `rollback`, `set-channel` | administrator | administrator |

Actions are below `io.github.tnuproject.metaupd`. Paths, URLs and the trust directory cannot
be passed over D-Bus.

## Errors

| Error | Meaning |
|---|---|
| `.Error.Busy` | Another operation is running |
| `.Error.NotAuthorized` | polkit refused |
| `.Error.NoTrust` | No trusted root metadata |
| `.Error.Verification` | Signature, hash or downgrade failure |
| `.Error.Incompatible` | Not for this system |
| `.Error.Failed` | Anything else |

## Versioning

Incompatible changes get a new interface name ending in a higher number.
