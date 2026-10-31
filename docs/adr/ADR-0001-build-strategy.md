# ADR-0001: Build strategy — from-scratch desktop environment on the wlroots ecosystem

- Status: Accepted (owner decision)
- Date: 2026-10-31

## Context

IBDE targets a floor of 512 MB RAM / 1 vCPU (Raspberry Pi Zero 2 W class)
with a hard budget for the DE's own footprint (≤ 50 MB idle — ADR-0002).
Every GTK3 process costs 30–60 MB RSS; a GTK-based shell cannot meet the
budget under any amount of tuning. Existing lightweight DEs idle at
330–450 MB precisely because their shells sit on heavyweight toolkits.

## Decision

Build IBDE as its own codebase on the Wayland / wlroots foundation:

- **Own from day one**: `libibde-ui` (toolkit), `ibde-shell`,
  `ibde-settingsd`, `ibde-session`, `ibde-terminal`, theming.
- **Compositor ladder** (ADR-0003): adopt labwc → surgical fork → own.
- **Borrow apps only** (file manager, GTK settings app) — apps sit outside
  the idle budget.

We do **not** hand-write DRM/KMS, input, or Wayland protocol plumbing;
wlroots (MIT) provides that layer, as it does for sway, labwc, Hyprland.

## Alternatives rejected

1. **XFCE chassis + custom shell** — lead agent's recommendation; owner
   rejected it. Identity + the memory floor require owning the shell stack
   outright; GTK in the session-critical path breaks ADR-0002.
2. **Composition-only (themed parts)** — skin-deep; no real global menu,
   spotlight, or product identity.
3. **Zero-reuse own display stack** — DRM/input drivers and protocol
   corner cases would consume the first year with no user-visible payoff
   (CutefishOS is the cautionary tale).

## Consequences

- The custom toolkit is the project's biggest bet; it is proven first
  (Phase 0 spike) before any shell work depends on it.
- We control every byte of the session-critical path — the only way the
  50 MB budget is achievable.
