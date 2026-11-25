# Contributing

## Ground rules

- **Say what is true.** Documentation, `status.toml` and program output describe
  what works now. Use the five status labels and nothing else.
- **Bring evidence.** Claims about hardware or applications need an upstream
  reference or a recorded test. "It should work" is not evidence.
- **Reuse upstream.** Check FreeBSD, its ports and existing projects first. Fixes to
  FreeBSD, drm-kmod or Linuxulator belong upstream.
- **Keep the host safe.** Nothing writes to disks, partitions or firmware without an
  explicit confirmation in the same invocation.
- The project is called META.

## Workflow

1. Open an issue before large changes.
2. Branch from `main`; keep each pull request to one subject.
3. Run `make check`. Run `make shell` too if you touched shell scripts.
4. Test FreeBSD-specific changes on FreeBSD and give the release in the pull
   request. A virtual machine is fine; see
   [testing](docs/development/testing.md).
5. Update documentation and `CHANGELOG.md` in the same pull request.

## Code

- Rust: `cargo fmt`, Clippy without warnings, no `unsafe`, no `unwrap` outside tests.
- Parsers of external input return errors and have tests with malformed input.
  Prefer captured output from a real system, stored under `fixtures/`.
- Run system tools by absolute path through `CommandRunner`.
- Shell: POSIX `sh`, checked with ShellCheck.
- New dependencies need a reason. Prefer pure Rust; FreeBSD must stay a first-class
  build host.
- Comments explain why, not what.

## Hardware reports

Use the hardware report issue form. An entry becomes `validated` only with a
`tested_on` record naming the release, machine, date and result.

## Licensing

Contributions are accepted under the BSD 2-Clause License. Do not submit code copied
from GPL sources; LinuxKPI work must respect the licences of the drivers involved.
