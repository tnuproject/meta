# Release metadata format

Status: EXPERIMENTAL. Version 1 may change until the first release.

All documents are JSON, at most 4 MiB, with unknown fields rejected. Digests and keys are
lowercase hexadecimal. Times are RFC 3339 in UTC, ending in `Z`.

## Assets of a release

| Asset | Contents |
|---|---|
| `meta-manifest.json` | Release manifest |
| `meta-manifest.sig.json` | Its signatures |
| `meta-root-N.json`, `meta-root-N.sig.json` | Root metadata version N, when rotating |
| `SHA256SUMS` | For people; metaupd does not use it |
| everything else | Artifacts named in the manifest |

## Root metadata

Defines which keys may sign what.

```json
{
  "schema": 1,
  "kind": "meta-root",
  "project": "META",
  "version": 1,
  "created": "2026-01-01T00:00:00Z",
  "expires": "2027-01-01T00:00:00Z",
  "keys": { "<key id>": "<Ed25519 public key>" },
  "roles": {
    "root": { "threshold": 2, "keyids": ["<key id>"] },
    "release": { "threshold": 1, "keyids": ["<key id>"] }
  }
}
```

A key id is the SHA-256 digest of the 32-byte public key. Version N+1 needs a threshold of
signatures from the root keys of version N and from its own. Versions cannot be skipped.
Intermediate versions may have expired; the last one must not have.

Removing a key from the next version revokes it. There is no separate revocation list.

## Signatures

```json
{
  "format": "meta-signature-v1",
  "role": "release",
  "signatures": [ { "keyid": "<key id>", "sig": "<base64 Ed25519 signature>" } ]
}
```

The signed message is:

```
"META-SIGNATURE-V1" NUL role NUL bytes-of-the-document
```

The role in the message means a signature for a release cannot be used for root metadata
even if one key has both roles. Verification is strict: weak keys and non-canonical
signatures are rejected. One key counts once.

## Manifest

| Field | Meaning |
|---|---|
| `version`, `codename`, `channel` | Pre-releases cannot be on `stable` |
| `sequence` | Strictly increasing per channel |
| `created`, `expires` | Expired manifests are rejected |
| `compatibility` | Architectures, `osreldate` range, minimum META and metaupd versions |
| `artifacts` | See below |
| `changelog` | Name of an artifact |
| `migrations` | Id, summary, document, whether reversible |

An artifact has `name`, `category`, `size`, `sha256`, `apply`, `requires_reboot` and
optionally `editions`.

| Category | Contents |
|---|---|
| `kernel` | Kernel; must require a reboot |
| `meta-system` | META system components |
| `native-packages` | pkg packages and repository metadata |
| `linux2bsd`, `linux-compat` | Linux compatibility |
| `linuxkpi`, `eds`, `firmware` | Drivers and firmware |
| `core-utilities`, `desktop`, `security`, `edition` | As named |
| `documentation` | Changelog, migration instructions |

| `apply` | Covered by boot environment |
|---|---|
| `pkg-in-boot-environment` | yes |
| `efi-system-partition` | no |
| `device-firmware` | no |
| `compatibility-root` | no |
| `none` | nothing to undo |

There is no method that runs a script, and none can be added without a schema change that
older clients reject.

Names are 1 to 128 letters, digits, `.`, `_`, `+` or `-`, and cannot start with `.` or `-`.
Manifests never contain URLs or paths.

## Channels

`testing` also receives `stable` releases; `unstable` receives all. Sequences are tracked
for each publishing channel separately.

## Tools

`metaupd-release` generates every document; see
[release process](release-process.md). An example description is in
[release.toml.example](../../metaupd/release/release.toml.example).
