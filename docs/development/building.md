# Building

## Requirements

| Task | Needs |
|---|---|
| Rust workspace | Rust 1.89 or newer |
| Checks | Python 3.11 or newer; ShellCheck for shell scripts |
| Native package | FreeBSD with pkg |
| Debian prototype | `dpkg-deb` |
| Arch prototype | `makepkg`, as an unprivileged user |
| Kernel | FreeBSD 15 with its source tree |

## Workspace

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`make` wraps these: `build`, `test`, `lint`, `docs`, `data`, `shell`, `ota`, `check`. The
Makefile works with BSD make and GNU make. Without make, run the commands above and:

```sh
python3 scripts/check-data.py
python3 scripts/check-docs.py
sh tests/ota-e2e.sh target/debug
```

On Windows use Git Bash for the last one, and `python` instead of `python3`.

## FreeBSD from another host

Type checking needs only the Rust target, not a C compiler or a sysroot:

```sh
rustup target add x86_64-unknown-freebsd
cargo clippy --workspace --all-targets --locked --target x86_64-unknown-freebsd
```

Linking needs a FreeBSD sysroot; building in a virtual machine is simpler.

## On FreeBSD

```sh
pkg install rust
cargo build --workspace --locked
```

## Packages

```sh
sh packages/native/build-packages.sh -b target/release -o dist -V 26.0-alpha1
sh packages/debian/build-deb.sh dist
sh packages/arch/build-arch.sh dist
```

## Kernel

```sh
sh scripts/fetch-kernel-source.sh /var/tmp/meta-src
sh scripts/build-kernel.sh -s /var/tmp/meta-src -o /var/tmp/meta-obj
sh packages/native/build-kernel-stage.sh -s /var/tmp/meta-src -o /var/tmp/meta-obj -d /var/tmp/stage
sh packages/native/build-packages.sh -b target/release -o dist -V 26.0-alpha1 \
    -k /var/tmp/stage meta-kernel
```

None of these touches the running system. The build took about four minutes with twelve virtual
CPUs. To try the kernel without risk, install the package into a boot environment:

```sh
bectl create meta-kernel
bectl mount meta-kernel /mnt
cp dist/meta-kernel-*.pkg /mnt/tmp/
pkg -c /mnt add /tmp/meta-kernel-26.0.a1.pkg
bectl umount meta-kernel
bectl activate -t meta-kernel
```
