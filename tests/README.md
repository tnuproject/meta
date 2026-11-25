# Cross-component tests

`ota-e2e.sh` runs the release tool and the client against each other with throwaway keys,
then attacks the result. It needs only a POSIX shell.

```sh
cargo build --workspace --locked
sh tests/ota-e2e.sh target/debug
```

Crate-level tests live with their crates; see
[docs/development/testing.md](../docs/development/testing.md).
