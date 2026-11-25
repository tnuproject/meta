# Governance

META is at an early stage, and its governance matches that.

## Roles

- **Contributors** open issues and pull requests.
- **Maintainers** review, merge and release. They are the members of the
  [tnuproject](https://github.com/tnuproject) organization with write access to this
  repository.
- **Key holders** hold META signing keys. They are maintainers chosen for that role.
  No keys exist yet.

## Decisions

Most decisions are made in pull requests by lazy consensus: without objections, a
maintainer merges after review.

These need a written proposal and the agreement of a majority of maintainers:

- replacing a FreeBSD subsystem,
- changing the release metadata format or trust model,
- creating, rotating or revoking signing keys,
- adding or removing an edition,
- changing this document or the licence.

Accepted proposals are recorded in
[docs/architecture/decisions.md](docs/architecture/decisions.md).

## Releases

Publishing a release takes two people: one to prepare it and a different one to
approve signing. Automation may prepare a draft but never publishes. While there are
fewer than two maintainers, no release can be published.

## Becoming a maintainer

Maintainers invite contributors with a record of careful work. Maintainers inactive
for a year move to emeritus.
