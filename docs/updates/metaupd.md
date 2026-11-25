# metaupd

Status: EXPERIMENTAL. metaupd checks, verifies, downloads, plans and rolls back. It does
not apply updates.

metaupd is the only update engine in META. Desktop programs talk to it; none implements
its own.

## Commands

| Command | Behaviour | Status |
|---|---|---|
| `metaupd`, `metaupd status` | Shows state, trust and boot environments | EXPERIMENTAL |
| `metaupd check` | Finds the newest acceptable release | EXPERIMENTAL |
| `metaupd download` | Downloads and verifies into the staging area | EXPERIMENTAL |
| `metaupd install --dry-run` | Verifies again and shows the plan | EXPERIMENTAL |
| `metaupd install` | Exits with 69 after the plan | PLANNED |
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
| 8. Clone and mount a boot environment | PLANNED |
| 9. Install packages into it with `pkg` | PLANNED |
| 10. Validate it | PLANNED |
| 11. Update items outside the boot environment | RESEARCH REQUIRED |
| 12. Activate it for one boot | EXPERIMENTAL |
| 13. Confirm health and activate permanently | PLANNED |

Step 12 exists as `metaupd rollback NAME --once`, which is the same operation. It was
tested across reboots.

## Applying updates

The design for steps 8 to 13, using what FreeBSD 15 documents for pkgbase:

```sh
bectl create meta-26.0.1-20260601120000
bectl mount meta-26.0.1-20260601120000 /mnt
pkg -c /mnt upgrade -r META-staged      # repository backed by the staging directory
bectl umount meta-26.0.1-20260601120000
bectl activate -t meta-26.0.1-20260601120000
```

Open points:

- Package scripts run during installation. They come from signed packages but run as root
  in the new environment; running them in `bectl jail` limits the damage.
- `pkg -c` chroots into the environment, so its own `pkg` and libraries are used.
- Only META's staged repository may be enabled, so nothing else is fetched.

### Validation before activation

`pkg check -s`, presence of the kernel and modules it needs, `loader.conf` and `fstab`
syntax, and that `rc` configuration still parses.

### Confirming a boot

A service marks the boot healthy once networking and login services are up. Without it the
next reboot returns to the old environment.

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
