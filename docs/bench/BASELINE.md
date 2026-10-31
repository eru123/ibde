# ibde-bench — floor-profile performance harness

Definitions and budgets: ADR-0002.

## v0 — baseline (stock labwc + waybar on Debian 13)

Boots a QEMU VM at the floor profile (**1 vCPU, 512 MB, software
rendering via `WLR_RENDERER=pixman`**), auto-starts stock `labwc` +
`waybar` on the serial console, settles, dumps measurements to the serial
log, powers off.

No ISO tooling required: cloud-init seed is delivered via the **NoCloud
net datasource** — the host serves `user-data`/`meta-data` over QEMU
user-network (`http://10.0.2.2:8123/`).

## Usage

```bash
bench/vm/fetch-image.sh   # download Debian 13 generic qcow2 (once, ~1 GB)
bench/vm/run-bench.sh     # boot, wait, print results
```

SSH debug access: `ssh -p 2222 debian@127.0.0.1`, password `ibdebench`
(VM-local only, user-mode networking).

## Notes

- KVM is used when available (host has it); falls back to TCG — a TCG
  first boot (cloud-init + apt install) takes 10–20 min; subsequent boots
  skip cloud-init growth.
- user-data runs once per instance-id; change `meta-data`'s instance-id
  to force a re-provision.
