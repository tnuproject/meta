# Privileges

Status: EXPERIMENTAL. The installer sets this up.

## sudo

sudo is always installed, with:

```
Defaults use_pty
Defaults env_reset
Defaults secure_path="/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin"
%wheel ALL=(ALL:ALL) ALL
```

Passwords are required. There is no passwordless rule. The file is a drop-in below
`/usr/local/etc/sudoers.d`, checked with `visudo -c` before use. Root's password is locked,
and single-user mode asks for authentication (`console` marked insecure in `/etc/ttys`).

## META programs

| Program | Privileges |
|---|---|
| `meta`, `linux2bsd`, `meta-eds scan` | None; USB details need root |
| `meta-installer` | None; it only plans |
| `meta-install-run` | root; erases the confirmed disk |
| `meta-compat` | root; runs commands as the user who called sudo |
| `meta-netd`, `metaupdd` | root; callers are authorized by polkit |
| `metaupd check`, `download`, `install`, `channel`, `rollback` | root |
| `metaupd status`, `history` | None |
| `metaupd-release` | None; never on client systems |

No META program is set-user-ID.

System tools are run by absolute path, with fixed arguments or validated ones, and never
through a shell.
