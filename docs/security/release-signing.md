# Release signing

Status: PLANNED. No keys exist.

## Keys

| Role | Where | Threshold | Validity |
|---|---|---|---|
| root | Offline, several people | 2 of 3 or more | One year per root version |
| release | One offline, one in GitHub's protected environment | 2 of 2 for stable | Until rotated |

`metaupd-release keygen` writes unencrypted keys and refuses to write inside a Git
repository. Keep root keys on encrypted removable media or, preferably, hardware tokens.
Hardware token support is RESEARCH REQUIRED; until it exists, use a machine that is never
connected to a network.

## First ceremony

1. Each root holder generates a key on an offline machine and reads out the public key and
   key id in person or over a verified channel.
2. One holder writes `root.toml`:

   ```toml
   version = 1
   expires = "2028-01-01T00:00:00Z"

   [roles.root]
   threshold = 2
   keys = ["<public key>", "<public key>", "<public key>"]

   [roles.release]
   threshold = 2
   keys = ["<public key>", "<public key>"]
   ```

3. `metaupd-release root --config root.toml --out root.json`
4. Every holder checks the file and signs:
   `metaupd-release sign --role root --file root.json --signatures root.sig.json --key-file KEY`
5. Publish both files in the repository and announce the key ids independently of GitHub.

## Rotation

1. Write `root.toml` with the next version number.
2. Sign with a threshold of the old root keys **and** of the new ones.
3. Attach as `meta-root-N.json` and `meta-root-N.sig.json` to every release from then on.
   Clients may skip releases but not root versions, so all versions must stay available.

Rotate root metadata before it expires. If it expires first, clients reject every release
until they can walk to a newer version; that still works, because expiry is checked only on
the last version.

## Revocation

| Compromised | Action |
|---|---|
| Release key | New root version without it |
| Root key, below threshold | New root version without it |
| Threshold of root keys | Trust cannot be repaired online. Publish new installation media and a new anchor, and tell users out of band |

## Never

- Put a secret key in the repository, an issue, a log, or workflow source.
- Give a workflow triggered by pull requests access to signing environments.
- Sign a document you have not read.

`.gitignore` excludes common key names and CI searches for key material. Both are a last
line, not a method.
