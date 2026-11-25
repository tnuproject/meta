# metaupd

Status: EXPERIMENTAL.

metaupd is the only update engine in META. Desktop programs talk to it; none implements
its own.

## Commands

| Command | Behaviour | Status |
|---|---|---|
| `metaupd`, `metaupd status` | Shows state, trust and boot environments | EXPERIMENTAL |
| `metaupd check` | Finds the newest acceptable release | EXPERIMENTAL |
| `metaupd download` | Downloads and verifies into the staging area | EXPERIMENTAL |
| `metaupd install --dry-run` | Verifies again and shows the plan | EXPERIMENTAL |
| `metaupd install --yes` | Installs into a new boot environment for one boot | EXPERIMENTAL |
| `metaupd confirm` | After that boot, makes it permanent or records why not | EXPERIMENTAL |
| `metaupd history` | Shows the history and checks its integrity | EXPERIMENTAL |
| `metaupd rollback` | Lists or selects boot environments | EXPERIMENTAL |
| `metaupd channel` | Shows or sets stable, testing or unstable | EXPERIMENTAL |

`--source DIR` reads releases from a local directory with the same verification.

## Steps of an update

| Step | Status |
|---|---|
| 1. List releases from the GitHub REST API | EXPERIMENTAL |
| 2. Follow root metadata rotations | EXPERIMENTAL |
| 3. Verify manifest signatures against the trusted root | EXPERIMENTAL |
| 4. Reject expired, older or conflicting manifests | EXPERIMENTAL |
| 5. Check architecture, kernel, META and metaupd versions | EXPERIMENTAL |
| 6. Download what is missing, resuming partial files | EXPERIMENTAL |
| 7. Verify size and SHA-256 of every artifact | EXPERIMENTAL |
| 8. Clone and mount a boot environment | EXPERIMENTAL |
| 9. Install packages into it with `pkg` | EXPERIMENTAL |
| 10. Validate it | EXPERIMENTAL |
| 11. Update items outside the boot environment | RESEARCH REQUIRED |
| 12. Activate it for one boot | EXPERIMENTAL |
| 13. Confirm health and activate permanently | EXPERIMENTAL |

Releases with artifacts for step 11 are refused before anything happens.

## Applying updates

The running system is never modified.

1. The state directory must be outside the boot environment, or installing is refused.
2. A clone is created and mounted below the state directory, with devfs.
3. Artifacts are copied in, and **the copies** are verified against the signed manifest.
4. `pkg -c` indexes and installs them. It sees one repository, the copies, so nothing else
   can be fetched. That repository needs no signature of its own.
5. The clone must contain a kernel, loader, rc and init, report exactly the release's version,
   and pass `pkg check -s` for the installed packages.
6. It is unmounted and activated for one boot.

If any step fails, the clone is unmounted and destroyed, and nothing is activated. A clone
that cannot be unmounted is left alone and reported, because destroying it might follow a
mount.

## Confirming a boot

`metaupd_confirm` runs after login services are up. If the system runs on the new environment
and reports the new version, the environment becomes permanent. If it runs on the old one, the
failure is recorded. If the new system never gets that far, the next reset returns to the old
one without any software being involved.

Health is judged by the boot reaching that point and by the version. Richer checks are
PLANNED.

Package scripts run as root in the clone while packages are installed. They come from signed
packages; running them in `bectl jail` would limit the damage and is PLANNED.

## Evidence

`tests/vm/suite.sh m9`, across five reboots: refusal with state inside the boot environment, a
modified staged package, installation of a release with a kernel, automatic confirmation, an
update sabotaged so that the kernel panics, automatic return with the failure recorded, notice
of a withheld release, and rollback with the history intact.

## What cannot be atomic

See [boot and storage](../architecture/boot-and-storage.md#what-boot-environments-do-not-cover).
Every artifact states how it is applied. Those outside the boot environment come last in a
plan and are shown as not undone by rollback. Migrations state whether they are reversible.

## Files

| Path | Contents |
|---|---|
| `/usr/local/etc/meta/trust/root.json`, `root.sig.json` | Trust anchor, installed with the system |
| `/var/db/metaupd/state.json` | Channel, highest sequences, root version |
| `/var/db/metaupd/trust/` | Later root versions, re-verified from the anchor each time |
| `/var/db/metaupd/staging/VERSION/` | Downloads |
| `/var/db/metaupd/history.jsonl` | History |

Directories writable by others are refused. State that does not parse is an error rather
than being reset, because resetting would discard downgrade protection.

## History

Each record contains the digest of the one before it, so edits and removals in the middle
are found. The chain is not signed: root can rewrite it, and removing the newest records
leaves no trace. It is a record for administrators, not protection against them.

## Crates

| Crate | Responsibility |
|---|---|
| `metaupd-metadata` | Formats and structural validation |
| `metaupd-verify` | Signatures, hashes, downgrade and compatibility policy |
| `metaupd-github` | Release listing and transport; trusts nothing |
| `metaupd-rollback` | `bectl` |
| `metaupd` | Engine, state, history, command line |
| `metaupd-release` | Release engineering |
