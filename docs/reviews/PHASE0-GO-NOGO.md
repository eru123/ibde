# Phase 0 go/no-go review

Date: 2026-10-31 · Decider: project owner (ratified lazily — this review
is agent-authored from measured evidence; owner override welcome)

## Decision: **GO** — proceed to Phase 1 (shell skeleton + flagship terminal)

## Evidence

| Question | Result | Source |
|---|---|---|
| Can a from-scratch, no-GTK shell render at interactive rates on pure software rendering? | **Yes** — 63 fps @ 1280×32 under sway/pixman, both C and Rust | `docs/bench/SPIKE-REPORT.md` |
| Is per-frame render cost viable? | **Yes** — 0.12–0.22 ms/frame offscreen (~1 % of a 60 fps budget) | SPIKE-REPORT |
| Does Rust cost us the memory budget? | **No** — 12.7 MB vs 12.2 MB process RSS, shared libs dominate either way | SPIKE-REPORT (ADR-0012) |
| Does the ecosystem work (layer-shell, cairo/pango bindings, container test rig)? | **Yes** — `ibde-ui` crate v0.1 in-repo, dogfooded by the spike, fmt/clippy/test clean | `crates/ibde-ui` |
| Was the stock-composition alternative really disqualified? | **Yes, measured** — waybar alone 45.8 MB (one bar!), DE-own ≈ 74 MB partial vs 50 MB full-DE budget | `docs/bench/BASELINE.md` |
| Can we measure the floor profile repeatably? | **Yes** — two-phase QEMU bench, reproducible with one script | `bench/vm/` |

## What is deliberately NOT yet proven (tracked, not blocking)

1. **Keymap engine patch vs labwc** (W2 spike) — Super→Ctrl translation in
   labwc's keyboard path. Fallback exists: ship without Mac-profile
   translation until `ibde-comp` (ADR-0003/0008).
2. **Interactive workout gate** (bench v1) — peak-RSS sampling during
   scripted interaction; harness exists, workout lands with the first
   shell modules (Phase 1 W3–4).
3. **Real floor hardware** — Pi Zero 2 W on order (owner action). QEMU
   1 vCPU/512 MB is the proxy until it arrives.
4. **GLES2 backend** — pixman path proven; EGL path is Phase 1 W3 (the
   toolkit's dual-backend seam is already in the Canvas contract).

## Phase 1 entry state

- Workspace: `crates/ibde-ui` (shm/canvas/stats + tests), spike binaries,
  CI (fmt/clippy/test/build), bench harness with baseline + reports.
- 8 commits, Apache-2.0 headers policy, ADRs 0001–0012 ratified.
- Phase 1 critical path per ROADMAP W3–W8: widget tree + text-grid fast
  path → shell bar/dock/launcher → terminal v1 → v0.1 internal alpha.
