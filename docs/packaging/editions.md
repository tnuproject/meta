# Packaging and editions

Status: EXPERIMENTAL.

## Three ecosystems

| Ecosystem | Manager | Owns | Status |
|---|---|---|---|
| Native | pkg | The host | EXPERIMENTAL |
| Debian | APT, dpkg | `/compat/meta/debian` | EXPERIMENTAL |
| Arch | Pacman | `/compat/meta/arch` | EXPERIMENTAL |

Both editions are the same META with a different compatibility environment. The kernel,
base system and META tools are identical.

## Isolation

- Every path has one owner. Anything outside the edition roots belongs to pkg.
- APT and Pacman run only inside their root, in a jail. They cannot reach the host's
  files, so they cannot overwrite them.
- pkg never writes into an edition root.
- Roots do not overlap each other or system directories.
- Only `/home` and `/tmp` are shared writable.
- There is no shared dependency resolution. A Linux package can never satisfy a native
  dependency, or the reverse.

[packages/policy](../../packages/policy/src/lib.rs) encodes these rules, and tests check
the [edition definitions](../../packages/editions) against them.

FreeBSD's ports contain `dpkg` and `pacman`. They are not used: run on the host, they
would track files next to pkg.

## Trust

A root is only ever created from authenticated material, and everything is verified before a
dataset exists.

| Edition | Source | Pinned |
|---|---|---|
| Debian 12 | debootstrap, `--force-check-gpg` | SHA-256 of both archive keys |
| Arch | Monthly bootstrap archive | Fingerprint of the release signer |

Debian's fingerprints were compared with the keyring package in Ubuntu's archive, which
Ubuntu signs. Arch's was compared between its download page and its web key directory.

FreeBSD packages `debian-keyring`, which holds developer keys but not the archive keys, so
META fetches and pins those itself.

Keys expire and are replaced. Updating a pin is a reviewed change to the edition definition.

## Behaviour worth knowing

Found while testing, and handled by `meta-compat`:

- FreeBSD's debootstrap wants a program named `gpgv2`; its gnupg installs `gpgv`. Without
  it, signatures cannot be checked.
- APT reports "gpgv ... required for verification, but neither seems installed" when
  `/dev/null` is missing, although gpgv is present.
- Arch's archive has Linux file capabilities, which FreeBSD cannot store.
- Pacman's download sandbox needs Linux features Linuxulator lacks, so it is turned off.
- The Linux modules must be loaded before a root is created.

## Kernels and boot loaders

Inside a root they have no function. In Debian, an APT pin leaves them without a candidate;
in Arch, a Pacman hook stops the transaction. Both were tested.

## Update model

metaupd updates the host. Roots are updated by their own manager through
`meta-compat update`, which takes a snapshot first. Rolling back the host leaves roots as
they are.

## Evidence

`tests/vm/suite.sh m7` for Debian 12 and Arch: creation, isolation, running as a user,
installing a package with verified signatures, refusing a kernel, snapshot, rollback, update,
refusal to remove a busy root, removal with `/home` intact.

Not tested: graphical programs, large package sets, months of rolling Arch updates.

## Prototype packages

| Package | Status |
|---|---|
| `meta-compat-environment` (deb) | EXPERIMENTAL |
| `meta-compat-environment` (Arch) | EXPERIMENTAL |

They carry the same protections for roots not made by `meta-compat`. The Arch package has
only been built by CI definitions that have not run.
