# Branding

## Names

- The project is **META**.
- The first planned release is **META 26.0**, codename **uglydemon**, lowercase.
- Tagline: "FreeBSD at its core. Linux compatibility by design."

Programs, the installer and documentation take these from
[common/src/version.rs](../../common/src/version.rs).

## Fastfetch

Fastfetch shows its built-in FreeBSD logo on META. Fastfetch chooses a logo from
`os-release`, and META's own identifier has none, so
[fastfetch/config.jsonc](fastfetch/config.jsonc) selects it by name. META ships no
logo of its own for Fastfetch and must not add one.

Verified with fastfetch 2.69.0 on FreeBSD 15.1-RELEASE-p7, 2026-11-25.

## FreeBSD's marks

Two different things are often mixed up:

- The **FreeBSD logo**, the horned sphere, is a trademark of the FreeBSD Foundation.
  The ASCII drawing in Fastfetch is based on it. Use follows the Foundation's
  [trademark guidelines](https://freebsdfoundation.org/legal/trademark-usage-terms-and-conditions/).
- The **BSD Daemon**, "Beastie", is copyright Marshall Kirk McKusick. Use needs his
  permission; see the [FreeBSD daemon page](https://www.freebsd.org/copyright/daemon/).

Rules for META:

- Do not add either image to this repository.
- Say "based on FreeBSD". Never suggest that META is FreeBSD, or is endorsed by the
  FreeBSD Project or Foundation.
- Keep FreeBSD's names and copyright notices in FreeBSD software.
- Installing the Fastfetch configuration in a release needs a check of the trademark
  guidelines first.
