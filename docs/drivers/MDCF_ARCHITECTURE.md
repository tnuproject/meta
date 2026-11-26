# Hardware driver compatibility framework

Status: EXPERIMENTAL.

MDCF finds hardware, says which drivers could serve it and how certain that is, and
carries out changes after showing them. It is part of EDS.

| Step | Code | Changes the system |
|---|---|---|
| Discovery | `eds/hardware-detection` | No |
| Database | `eds/driver-database` | No |
| Host facts | `eds/engine/src/host.rs` | No |
| Evaluation | `evaluate.rs` | No |
| Firmware | `firmware.rs` | No |
| LinuxKPI | `linuxkpi.rs` | No |
| Capabilities | `capability.rs` | No |
| Prediction | `predict.rs` | No |
| Life cycle | `lifecycle.rs` | After `--yes` |

## Commands

| Command | Shows |
|---|---|
| `meta-eds status` | Policy, database, LinuxKPI registry |
| `scan`, `hardware` | Devices, with the best verdict; with all details |
| `drivers`, `compatibility [ID]` | Candidates; every check behind a verdict |
| `firmware` | What fwget(8) says this hardware needs |
| `capabilities` | What the hardware can do, and how that was determined |
| `linuxkpi [INTERFACE]` | State of LinuxKPI |
| `predict DIR` | [Prediction](MPDC_PREDICTION_ENGINE.md) |
| `diagnose`, `verify ID`, `report` | Problems; a component's state; a text for a bug report |
| `plan OPERATION ID` | What an operation would do |
| `prepare`, `install`, `enable`, `disable`, `remove` ID | Operations |
| `enable`, `disable` | EDS's policy |
| `list`, `info ID` | The database |
| `update` | Exits with 69; drivers are updated with the system |

`--json` works everywhere. Exit codes: 0, 1 failed, 2 usage or `verify` found a problem, 69.

## Life cycle

| Operation | Steps |
|---|---|
| `prepare` | Download packages |
| `install` | Install packages |
| `enable` | Load modules, and load them at boot |
| `disable` | Stop loading at boot. Loaded modules stay until the next restart |
| `remove` | Remove packages, unless other packages need them |

A plan lists every command and the one that undoes it. Carrying it out stops at the first
failure; what was done before is not undone by itself. Settings are changed with sysrc(8)
only, in `kld_list`. No module is ever unloaded, because unloading a driver in use can take
the machine down. Untested components need `--accept-untested`; INCOMPATIBLE ones are
refused even so.

## What it does not do

- Build drivers, patch them, or load modules it was not told to.
- Install anything on its own, at boot or otherwise.
- Update drivers; packages are updated with the system.
