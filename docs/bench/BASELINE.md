# Floor-profile baseline — stock labwc + waybar (bench v0)

ADR-0002 "before" numbers. Captured 2026-10-31.

## Environment

- QEMU floor profile: **1 vCPU, 512 MB RAM**, virtio-vga, no GPU
  acceleration — `WLR_RENDERER=pixman` (pure software rendering)
- Debian 13 (trixie) generic cloud image, stock `labwc` + `waybar`,
  seatd seat management
- Measurement after 15 s settle post-desktop; RSS via ps (includes
  shared pages)
- Reproduce: `bench/vm/run-bench.sh` (two-phase boot: provision → measure)

## Results

| Metric | Value |
|---|---|
| Total system memory used at idle desktop | **128 MB / 464 MB usable** |
| labwc (compositor, pixman) | **26.2 MB** |
| waybar (GTK-based bar) | **45.8 MB** |
| seatd | 1.8 MB |
| at-spi2 pair (pulled in by GTK apps) | 14.5 MB |
| dbus trio + systemd suite + kernel | ~100 MB |
| Session process count | 90 |
| Noise: unattended-upgrades still finishing | 21.4 MB (to be disabled in bench images) |

## Analysis

1. **The stock minimal session already breaks ADR-0002.** DE-own
   footprint = labwc + waybar + seatd ≈ **74 MB** against a 50 MB budget —
   and that buys *one bar*, no dock, no launcher, no notifications, no
   lock screen, no settings daemon.
2. **waybar costs more than the compositor.** The GTK toolkit tax
   (~46 MB for a single panel process) is exactly the ADR-0001/ADR-0004
   thesis. Our single-process `ibde-shell` (no GTK, ibde-ui + pango/
   cairo shared libs) is projected at 15–25 MB while replacing waybar
   *and* adding dock/spotlight/notifications/lock/control-center.
3. **The 200 MB full-session budget is comfortable.** Even with waybar's
   waste, idle hit 128 MB. A 46→20 MB shell swap plus disabling
   unattended-upgrades in our images lands well under budget, leaving
   ~330 MB of the 512 MB floor device for the user.
4. labwc at 26 MB is acceptable for Phase 1–2 (ADR-0003 ladder); if the
   stretch goal (35 MB total) ever matters, compositor trimming becomes
   a Phase 3+ conversation.

## Implications for the budget

| Component | Today (stock) | IBDE target |
|---|---|---|
| Compositor | labwc 26.2 MB | 26 MB (Phase 1–2), revisit at Phase 3 |
| Shell surface | waybar 45.8 MB (bar only) | **≤ 25 MB** (bar+dock+launcher+notify+lock+OSD) |
| Seat + session glue | seatd 1.8 MB | ~2 MB |
| **DE-own total** | **~74 MB (partial DE)** | **≤ 50 MB (full DE)** |
| Full session idle | 128 MB | ≤ 200 MB (≈ 110 MB expected) |

Next refinements (bench v1): interactive workout sampling, JSON output,
PSS accounting (smem-style), unattended-upgrades disabled, RSS-regression
gate wired into CI.
