# Security policy

## Supported versions

META has no releases, so no version receives security updates. Do not use this
software to protect anything yet. Vulnerabilities in the code are still wanted.

FreeBSD vulnerabilities should go to the
[FreeBSD Security Team](https://www.freebsd.org/security/).

## Reporting

Report privately through
[GitHub security advisories](https://github.com/tnuproject/meta/security/advisories/new).
Do not open a public issue. Include what an attacker needs, what they gain, and
steps to reproduce.

You should get an acknowledgement within seven days. Please allow 90 days for a fix
before publishing; earlier publication can be agreed when a fix is ready.

## Of particular interest

- Accepting an update that is not signed, is older, or is for another system.
- Writing outside the staging directory, or using files another user can modify.
- Escaping a compatibility root, or one package ecosystem writing another's files.
- Leaking installer passwords into plans, logs or process arguments.
- Crashes or memory growth from malformed ELF files or metadata.

## Design documents

- [Threat model](docs/security/threat-model.md)
- [Release signing](docs/security/release-signing.md)
- [Privileges](docs/security/privileges.md)

## Keys

No META signing keys exist yet. Anything that claims to be signed by META today is
not.
