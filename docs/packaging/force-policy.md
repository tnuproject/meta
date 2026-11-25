# Force policy

Status: EXPERIMENTAL as tested code; not connected to pkg.

Normal installation must work without any override. `--force` exists for specific, known
situations, and each must be named:

```
meta-pkg install --force=newer-os-version example
```

There is no plain `--force` and no way to override everything.

| Violation | Overridable | Reason |
|---|---|---|
| `newer-os-version` | yes | Built for a newer point release; often works |
| `compatibility-advisory` | yes | META's own advice |
| `already-installed` | yes | Reinstallation |
| `signature-invalid` | no | Integrity |
| `checksum-mismatch` | no | Integrity |
| `untrusted-repository` | no | Integrity |
| `known-vulnerability` | no | Security |
| `missing-dependency` | no | Would break the program |
| `dependency-conflict` | no | Would break another package |
| `abi-mismatch` | no | Cannot work |
| `file-conflict` | no | Would damage another package |
| `ecosystem-boundary` | no | Would break isolation |

If several violations occur, every one must be overridable and named.

`pkg install -f` is different: it reinstalls an installed package. META does not change
pkg's options. `meta-pkg` is a PLANNED front end.

Administrators who need more can use pkg directly. The policy limits what META offers to do
for them.
