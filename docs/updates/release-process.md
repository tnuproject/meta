# Release process

Status: EXPERIMENTAL. The tools are tested; no release has been made, and the workflow has
not run on GitHub.

Releases are prepared by automation and published by people.

## Prerequisites

1. Root metadata signed by the root key holders, as described in
   [release signing](../security/release-signing.md). Committed as
   `metaupd/release/root.json` and `root.sig.json`.
2. GitHub environments `release-signing` and `release-draft`, each with required reviewers
   who are not the person preparing the release, restricted to `main`.
3. `META_RELEASE_KEY` stored as a secret of `release-signing` only.
4. Branch protection on `main` with required review.
5. At least two maintainers.

None of these exist yet.

## Steps

1. In a pull request, add `metaupd/release/release.toml` from the
   [example](../../metaupd/release/release.toml.example) and update `CHANGELOG.md`. Review
   checks version, channel, sequence, compatibility and artifacts.
2. Run the *Release preparation* workflow from `main`.
3. The workflow tests and builds on FreeBSD, generates the manifest, and stops for approval
   before signing.
4. A reviewer inspects the unsigned release and approves.
5. The workflow signs, verifies the result as a client would, and writes checksums.
6. With *create_draft*, after a second approval, it creates a draft release.
7. A maintainer downloads the draft, verifies it independently with
   `metaupd-release verify`, and publishes it by hand.
8. Remove `release.toml` again, so the workflow cannot run by accident.

## By hand

The same without GitHub Actions, for offline signing:

```sh
metaupd-release manifest --config release.toml --artifacts out/
metaupd-release sign --role release --file out/meta-manifest.json \
    --signatures out/meta-manifest.sig.json --key-file /media/usb/release.secret
metaupd-release verify --root root.json --root-signatures root.sig.json --dir out/
metaupd-release checksums --dir out/
```

`sign` adds to an existing signature file, so several holders can sign in turn. It refuses
documents a client would reject.

## Safeguards in the workflow

| Safeguard | Purpose |
|---|---|
| Runs only from `main` and only by hand | No release from a branch or pull request |
| Fails without reviewed release inputs | No accidental release |
| `contents: read` except in the draft job | A compromised build step cannot publish |
| Tool built before the key is available | Build scripts never see the key |
| Actions pinned by commit | A moved tag cannot change the workflow |
| Draft only | Publishing stays a human decision |

## Remaining risks

- A GitHub Actions secret can be read by whoever controls the signing job. Keys in GitHub
  should have the lowest authority: a release threshold of two with one offline key makes
  the GitHub key insufficient on its own. This is recommended before the first stable
  release.
- Builds are not reproducible yet, so reviewers cannot compare binaries independently.

## After a bad release

Publish a newer release with a higher sequence. Deleting a release from GitHub does not
make clients forget it; they will warn that it is being withheld.
