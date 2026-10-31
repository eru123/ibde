# ADR-0004: libibde-ui — our retained-mode C toolkit

- Status: Accepted
- Date: 2026-10-31

## Decision

`libibde-ui` is a retained-mode widget toolkit in C (Apache-2.0):

- **Dual backend, both first-class**: EGL/GLES2 where a GPU exists;
  wl_shm + pixman/cairo for software rendering. Chosen at runtime; floor
  devices are never an afterthought.
- **Text**: pango/cairo behind an `ibde_text` abstraction — correct i18n,
  RTL, CJK from day 1. If the bench shows pango is a material share of
  the 50 MB budget, swap the backend to harfbuzz + friBidi behind the
  same API. Measure first, optimize second.
- **Damage tracking + partial redraw**; an idle shell repaints zero
  pixels and wakes zero timers.
- **Animation**: timeline/tween engine with spring curves; animations
  auto-disable below a measured FPS threshold (comfort features, never
  load-bearing).
- **Text-grid fast path**: terminal cell renderer (glyph atlas, O(1)
  scrolling, damage-tracked) designed in from the start —
  `ibde-terminal` (ADR-0011) is a first-class consumer.
- **Styling**: declarative skin files (CSS-lite), hot-reloadable; full
  accent palette day 1.
- **Tests**: offscreen render → golden hash per widget; layout unit
  tests; fuzzed skin-file parser.

## Rejected

- **GTK3/4** — memory floor (30–60 MB per process) breaks ADR-0002.
- **Qt** — runtime RSS and startup cost.
- **Immediate-mode only** — wastes CPU on floor devices.
