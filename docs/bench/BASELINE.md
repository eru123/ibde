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

Two runs captured: initial (RSS-only) and v0.1 (adds **PSS** via
smaps_rollup — the honest marginal cost — and disables
unattended-upgrades noise). Numbers below are from the v0.1 run unless
noted.

| Metric | RSS | PSS (marginal) |
|---|---|---|
| waybar (GTK bar) | 46.1 MB | **36.8 MB** |
| labwc (compositor, pixman) | 26.2 MB | **18.2 MB** |
| at-spi2 pair (pulled by GTK) | 14.4 MB | 4.1 MB |
| Total system used at idle desktop | **119 MB / 464 MB usable** | — |
| Session process count | 88 | — |

## Analysis

1. **The stock minimal session already breaks ADR-0002.** DE-own
   footprint = labwc + waybar + seatd ≈ **55 MB PSS** against a 50 MB
   budget — and that buys *one bar*, no dock, no launcher, no
   notifications, no lock screen, no settings daemon.
2. **waybar costs double the compositor.** One GTK panel process is
   36.8 MB of true marginal memory — the exact ADR-0001/ADR-0004 thesis.
   Our single-process `ibde-shell` (no GTK; ibde-ui + shared pango/cairo)
   replaces waybar *and* adds dock/spotlight/notifications/lock/OSD for a
   projected ≤ 25 MB PSS.
3. **The 200 MB full-session budget is comfortable.** Idle hit 119 MB
   even with waybar's waste; our shell swap keeps ~330 MB of the 512 MB
   floor device free for the user.
4. labwc at 18.2 MB PSS is acceptable for Phase 1–2 (ADR-0003 ladder);
   compositor trimming is a Phase 3+ conversation only if the 35 MB
   stretch goal matters.

## Implications for the budget

| Component | Today (stock, PSS) | IBDE target (PSS) |
|---|---|---|
| Compositor | labwc 18.2 MB | ~18 MB (Phase 1–2), revisit at Phase 3 |
| Shell surface | waybar 36.8 MB (bar only) | **≤ 25 MB** (bar+dock+launcher+notify+lock+OSD) |
| Seat + session glue | seatd < 1 MB | ~1 MB |
| **DE-own total** | **~55 MB (partial DE)** | **≤ 50 MB (full DE)** |
| Full session idle | 119 MB | ≤ 200 MB (≈ 105 MB expected) |

Next refinements (bench v1): interactive workout sampling, JSON output,
PSS accounting (smem-style), unattended-upgrades disabled, RSS-regression
gate wired into CI.

## Update 2026-11-01: the IBDE session on the floor profile

Same QEMU floor profile (1 vCPU / 512 MB / pixman), labwc, but the
session is now IBDE itself: `ibde-shell` (menubar + dock) replaces
waybar, plus a live `ibde-terminal` (60×12) running command output.
Screenshot: `docs/bench/floor-session.png` — all three elements
pixel-verified.

| Component | RSS | PSS (marginal) |
|---|---|---|
| labwc | 28.3 MB | **20.1 MB** |
| ibde-shell (bar + dock) | 13.9 MB | **6.4 MB** |
| ibde-terminal (60×12, live) | 14.6 MB | **6.9 MB** |
| **DE-own total** | 56.8 MB | **33.4 MB** |

- **ADR-0002 verdict: PASS with 33 % headroom.** The full working
  desktop — bar, dock, interactive terminal, compositor — costs 33.4 MB
  PSS against the 50 MB budget (which was set for the DE's own
  processes; labwc is adopt-phase and trims later per ADR-0003).
- **vs the stock baseline above**: labwc+waybar was 55.1 MB PSS for a
  bar and nothing else. IBDE delivers the complete interactive desktop
  in 61 % of that. waybar alone (36.8 MB) costs 2.9× our entire shell.
- Session total: **115 MB used of 464 MB** (119 MB with waybar and no
  terminal) — 350 MB left for the user on the 512 MB floor device.
- Shell discipline held on the floor CPU: 1 redraw/second (clock tick),
  dock drew once, terminal drew only on PTY output.

Reproduce: `cp target/release/{ibde-shell,ibde-terminal} bench/vm/ &&
bench/vm/run-bench.sh` (the measure phase auto-detects the binaries;
without them it falls back to the waybar baseline above).

