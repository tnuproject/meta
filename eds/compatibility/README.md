# Validating hardware

An entry becomes `validated` when its `tested_on` list says where it worked.

1. Use a supported FreeBSD release, fully updated. Note `freebsd-version -ku`.
2. Record `meta-eds scan --json`.
3. Exercise the device, not just its driver:

| Category | Minimum |
|---|---|
| GPU | Correct resolution, accelerated rendering, suspend and resume |
| Wi-Fi | Scan, connect with WPA2 or WPA3, transfer 1 GiB, reconnect after suspend |
| Ethernet | Link, DHCP, transfer 1 GiB |
| Bluetooth | Pair and use one input and one audio device |
| Camera | Picture in two programs |
| Audio | Playback, recording, jack detection |
| Fingerprint | Enroll, verify, reject another finger |
| Printer, scanner | One page each |

4. Report failures too; they become notes or `unsupported` entries.

"The driver attached" alone is not a result.
