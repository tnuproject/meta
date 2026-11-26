# Supported hardware

META has tested **no physical hardware**. These are the only validated entries:

| Entry | Tested on |
|---|---|
| VirtIO network adapter | 15.1-RELEASE-p7, QEMU i440FX |
| VirtIO block device | 15.1-RELEASE-p7, QEMU i440FX |
| QEMU standard VGA | 15.1-RELEASE-p7, QEMU i440FX with BIOS |

Everything else in the database is documented upstream and not tested by META. A device of
yours is at best PROBABLE until someone reports it.

```sh
meta-eds list            # all entries
sudo meta-eds scan       # your hardware
sudo meta-eds report     # text for a report
```

The machine's owner tests physical hardware; a validated entry needs the release, the machine,
the date and what was done. See [the model](MDCF_COMPATIBILITY_MODEL.md).
