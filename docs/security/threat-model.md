# Threat model

## Assets

The integrity of installed software, the ability to keep updating, user data, and the
confidentiality of credentials.

## Update attackers

| Attacker | Can | Cannot |
|---|---|---|
| Network | Block, delay | Change anything unnoticed |
| GitHub, or whoever controls the repository | Withhold, reorder, replay old releases | Forge a release, downgrade a client |
| Holder of a release key, below threshold | Nothing | Sign a release |
| Holder of a threshold of release keys | Sign malicious releases | Change which keys are trusted |
| Holder of a threshold of root keys | Everything | |
| Local unprivileged user | Read state | Change state, staging or trust |

## Defences

| Attack | Defence | Tested |
|---|---|---|
| Forged or edited manifest | Threshold signatures over exact bytes | yes |
| Signature reused for another purpose | Role in the signed message | yes |
| One key counted twice | Distinct key ids | yes |
| Downgrade to an older release | Sequence per channel, remembered on disk | yes |
| Two releases with one sequence | Digest remembered; both rejected | yes |
| Installing an older version | Version must be newer than installed | yes |
| Replay after expiry | `expires` in manifest and root | yes |
| Release for another system | Compatibility checks | yes |
| Changed artifact | Size and SHA-256 | yes |
| Change after download | Verified again when planning | yes |
| Interrupted download | Partial file, verified before use, dropped if wrong | yes |
| Endless download | Size limit while downloading | partly: local source only |
| Path traversal | Restricted names; versions parsed before use as paths | yes |
| Files from another host | URLs never come from metadata; prefix checked | yes |
| Compromised key | Rotation signed by old and new root keys | yes |
| Rolling trust back | Root version remembered; chain must reach it | yes |
| Modified trust anchor | Must carry its own valid signatures | yes |
| State writable by others | Ownership and mode checked | yes |
| Two updaters at once | Lock file | yes |
| Unsigned scripts | Format has no script method | yes |
| Staged file changed before installation | The copy pkg reads is verified | yes |
| pkg fetching something else during an update | Only the verified copies are visible to it | yes |
| Update that does not boot | One-time activation | yes |
| Unprivileged D-Bus caller | polkit, with the caller identified by the bus | yes |
| Package repository signed by another key | EdDSA signature | yes |
| Forged distribution archive | Pinned keys, checked before a root exists | partly: not with a forged archive |

## Accepted limits

- **Freeze.** Whoever controls distribution can hide new releases until the last seen
  manifest expires. Clients then report expiry. A short validity limits this; a separate
  timestamp role would limit it further.
- **First installation.** The anchor comes with the installation medium. Verifying the
  medium is the user's responsibility.
- **Clock.** A wrong clock can make valid metadata look expired, or expired metadata valid.
- **Rollback.** An administrator may return to an older boot environment with known
  vulnerabilities. That is deliberate, and recorded.
- **State inside the boot environment.** Without a separate dataset, rollback also reverts
  downgrade protection. metaupd refuses to install then. See
  [boot and storage](../architecture/boot-and-storage.md#update-state-must-live-outside-the-boot-environment).
- **Package scripts.** Signed packages can run code as root while being installed.
- **fetch(1)** follows redirects without insisting on HTTPS. Only confidentiality of the
  request is affected.

## Linux compatibility

Linux programs run with the user's full privileges. A compatibility root is for
organization, not a sandbox; with a shared `/home`, programs in it can read the user's
files. Linuxulator enlarges the kernel's attack surface and is optional.

## Installer

Text input reaches privileged tools, so it is validated on entry and again before use. Plans
contain no secrets. The wrong disk is the main danger: a disk in use is refused, and its name
must be repeated.
