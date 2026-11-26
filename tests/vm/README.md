# Virtual machine tests

These run whole milestones on a real FreeBSD kernel. They need a Linux host with QEMU, KVM,
`genisoimage` and OVMF.

```sh
bash tests/vm/vm.sh start          # downloads and verifies FreeBSD's image the first time
bash tests/vm/suite.sh build
bash tests/vm/suite.sh m6 m9 m5 m11
bash tests/vm/vm.sh stop           # deletes the machine
```

| File | Purpose |
|---|---|
| `vm.sh` | Disposable machine on an overlay; reachable only from localhost |
| `console.sh`, `serial.py` | Second machine, driven through its serial console |
| `suite.sh` | Runs milestones |
| `lib.sh` | Helpers for guest scripts |
| `guest/mlce-*.sh`, `guest/mdcf.sh` | Linux machines under bhyve, and drivers; `suite.sh mlce mdcf` |
| `guest/*.sh` | Run inside the machine as root; they change it freely |
| `guest/*.serial` | Console scripts |

Some milestones build on others: m11 uses the repositories of m5, m10 the keys of m9, and
m12 the kernel of m1.

In console scripts, `expect` also sees the echo of what was typed. A command that prints a
marker must not contain it: write `echo DONE''-OK` and expect `DONE-OK`.

`mlce` starts guests inside the guest. It needs nested virtualization, takes hours where that is
slow, and downloads about 1 GiB of images, which are verified.

Keys are generated inside the machine and disappear with it.
