# metaupd D-Bus API

Status: PLANNED. The interface is
[defined](../../metaupd/dbus/io.github.tnuproject.MetaUpd1.xml); no daemon implements it.

| Item | Value |
|---|---|
| Bus | system |
| Name | `io.github.tnuproject.MetaUpd1` |
| Object | `/io/github/tnuproject/MetaUpd1` |

## Behaviour

- `Check`, `Download` and `Install` return a job object at once. The job reports progress
  and emits `Finished`.
- One job that changes state runs at a time. Others fail with `.Error.Busy`.
- The daemon uses the same engine library as the command line tool, with the same lock, so
  both can be used together.
- The daemon exits when idle and is started by D-Bus.

## Authorization

| Action | Default |
|---|---|
| `check` | any active local user |
| `download` | any active local user |
| `install` | administrator |
| `rollback` | administrator |
| `set-channel` | administrator |

Actions are polkit actions below `io.github.tnuproject.metaupd`. The daemon checks the
caller's identity through the bus and never trusts arguments for it.

Paths, URLs and the trust directory cannot be passed over D-Bus.

## Errors

| Error | Meaning |
|---|---|
| `.Error.Busy` | Another operation is running |
| `.Error.NotAuthorized` | polkit refused |
| `.Error.NoTrust` | No trusted root metadata |
| `.Error.Verification` | Signature, hash or downgrade failure |
| `.Error.Incompatible` | Not for this system |
| `.Error.Unsupported` | Not implemented |

## Versioning

Incompatible changes get a new interface name ending in a higher number.
