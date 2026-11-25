# ath11k gap analysis

Status: EXPERIMENTAL. Made with [gap.py](gap.py) on 2026-11-25 from FreeBSD
15.1-RELEASE-p7. Nothing here has been tried on hardware.

Qualcomm's Wi-Fi 6 family (QCA6390, WCN6855 and others) is in many recent laptops.
FreeBSD's tree has the driver in `sys/contrib/dev/athk/ath11k` with a module Makefile
that builds only its PCI part. Upstream lists it as still to do.

```sh
python3 linuxkpi/gap.py /usr/src sys/contrib/dev/athk/ath11k \
    --baseline sys/contrib/dev/iwlwifi \
    --baseline sys/contrib/dev/rtw88 --baseline sys/contrib/dev/rtw89
```

The baselines are drivers that work on FreeBSD. What they lack as well is conditional
code rather than a gap.

| Driver | Kernel interfaces used | Missing | Marked unfinished |
|---|---|---|---|
| ath10k | 561 | 105 | 22 |
| ath11k | 514 | 86 | 43 |
| ath12k | 541 | 81 | 41 |
| mt76 | 715 | 122 | 45 |

## What the missing 86 are

| Group | Examples | Needed for PCI |
|---|---|---|
| Device tree, remoteproc, shared memory | `of_*`, `rproc_*`, `qcom_smem_*` | No, AHB only |
| Platform devices | `platform_get_resource` | No, AHB only |
| IOMMU | `iommu_map` | No, AHB only |
| Tracing | `trace_ath11k_*`, `__field` | No, disabled by the Makefile |
| Relay files, debugfs | `relay_*`, `simple_write_to_buffer` | No, disabled |
| Test mode, netlink attributes | `cfg80211_testmode_*`, `nla_*` | No |
| Wake on wireless | `enable_irq_wake` | No, power management is disabled |
| Interrupt affinity | `irq_set_affinity_and_hint` | Optional |
| DMA barriers, noncoherent memory | `dma_rmb`, `dma_alloc_noncoherent` | Check |

So almost nothing missing blocks the PCI driver from compiling, which agrees with the
Makefile's choices.

## What blocks it

The interfaces that exist but are marked unfinished are the actual work:

| Area | Interfaces | Why it matters |
|---|---|---|
| MHI bus | `mhi_prepare_for_power_up`, `mhi_sync_power_up`, `mhi_power_down`, `mhi_pm_suspend` and others | Loads firmware and carries every control message. Without it the device never starts |
| QMI over QRTR | `qmi_handle_init`, `qmi_add_lookup`, `qmi_send_request`, `qmi_txn_*`, `kernel_connect` | The firmware handshake: capabilities, memory, board data |
| Resizable hash table | `rhashtable_*` | Peer lookup in the data path |
| Keyed hash | `crypto_*_shash` | Michael MIC for TKIP |
| Newer 802.11 features | `ieee80211_*ema*`, `*fils*`, `*color*`, radar | Access point mode and Wi-Fi 6 extras; not needed for a first station connection |

## Order of work

1. MHI: power up, firmware download (BHI/BHIE), event and transfer rings. LinuxKPI
   declares it; the functions need bodies. Useful for ath12k and WWAN modems too.
2. QRTR and QMI on top of it, enough for the firmware handshake. Also shared with ath12k.
3. `rhashtable`. FreeBSD's `hash(9)` can back it.
4. `crypto_shash` for the one algorithm used.
5. Then a device test: attach, scan, associate with WPA2 as a station.

Steps 1 and 2 are most of the effort, and belong upstream. Ask on freebsd-wireless before
starting: ath10k, ath11k and ath12k are on the maintainers' list.

## Limits of the method

The comparison is textual. It does not know which files are compiled, cannot see behaviour,
and counts names rather than effort. A driver with no gaps can still not work.
