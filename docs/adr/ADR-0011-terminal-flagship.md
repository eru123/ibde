# ADR-0011: ibde-terminal — flagship, first-party, from Phase 1

- Status: Accepted (owner decision, 2026-10-31)
- Owner's words: *"the terminal is the most important feature of this, so
  ensure that this works properly and offers advanced features for power
  users but still beginner friendly."*

## Decision

IBDE ships **its own terminal as a core Phase 1 component**, built on
`libvterm` + `libibde-ui`'s text-grid fast path (ADR-0004). No VTE, no
GTK — the flagship must fit the floor: **≤ 15 MB RSS typical** (VTE+GTK3
costs 40–60 MB, disqualifying).

This supersedes the earlier "ship a themed xfce4-terminal" placeholder.

Full spec: `docs/TERMINAL-SPEC.md`. Summary:

- **Beginner defaults**: bundled JetBrains Mono (OFL), working
  copy/paste/scroll/URLs out of the box, plain-language right-click menu,
  settings UI pane.
- **Power layer**: tabs, splits, full key rebinding, vi-style copy mode,
  command palette, quake drop-down, OSC 7/8/9/52 shell integration.
- **Perf gates in bench** (ADR-0002): cat-throughput, scroll ≥ 30 fps on
  pixman @ 1 vCPU, typing echo ≤ 16 ms.

## Consequences

The text-grid renderer sits on the `libibde-ui` critical path from
Phase 1 — it shapes the toolkit API early, which is cheaper than
retrofitting. A dedicated workstream (W5) owns it.
