# ADR-0002: Floor device and memory budget contract

- Status: Accepted (owner decision)
- Date: 2026-10-31

## Floor device

Raspberry Pi Zero 2 W class: **512 MB RAM, 1 vCPU, VideoCore GLES2 or pure
software rendering**. Old x86 (~2008, i915 or no GPU, HDD) shares the
floor. Comfort tier (Pi 4/5, 2012+ x86) enables animations and effects.
The floor is a *floor for the DE itself*: modern third-party apps are not
expected to run beside it on a 512 MB machine, and no effort is spent
optimizing them for it.

## Budgets (DE's own processes: compositor, shell, settingsd, session)

| Metric | Budget | Measured by |
|---|---|---|
| Idle RSS, all DE processes | ≤ 50 MB (stretch 35 MB) | bench floor profile, settle ≥ 10 s |
| Peak RSS during interaction | ≤ 100 MB | scripted workout (below) |
| Full session idle (OS baseline incl., no apps) | ≤ 200 MB | same boot |
| Soak | 24 h, zero crashes, RSS drift < 2 MB | nightly soak VM |

**Interactive workout** (defines "peak"): open/close spotlight, open the
global menu, fire a 5-notification burst, spawn and destroy 5 windows —
RSS sampled at 250 ms, no frame cliff on the pixman renderer.

## Enforcement

`ibde-bench` runs the floor profile (QEMU: 1 vCPU / 512 MB, both
renderers) nightly; RSS regressions > 5 % fail the gate. OS tuning for
floor devices (zram ~256 MB lz4, minimal package set, no
recommends-chains) ships in `ibde-defaults`.
