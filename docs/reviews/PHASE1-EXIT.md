# Phase 1 exit review — v0.1

Date: 2026-11-01 · Agent-authored from measured evidence; owner
override welcome.

## Verdict: **EXIT GRANTED** — tag `v0.1` ("first light")

A working IBDE desktop exists: menubar, dock, IBDE menu, Spotlight,
and a fully interactive flagship terminal, composing under both sway
(headless CI) and labwc (the ADR-0003 Phase-1 target), pixel-verified
end to end and memory-verified on the floor profile.

## Planned vs delivered (ROADMAP W3–W8)

| Planned | Delivered | Note |
|---|---|---|
| libibde-ui core: widget tree, styling, animation, text-grid | **grid + glyph atlas + damage renderer + canvas + session glue + input** (23 tests) | PIVOT: shipped targeted modules over a generic widget tree — the shell needed grid/atlas/session first; tree+styling+animation move to Phase 2 where the bar's needs justify them |
| Shell v0: bar module | **bar + dock + IBDE menu popover + Spotlight + hand-drawn cursor** — one process, 6.4 MB PSS on the floor | dock/spotlight were Phase-2 scope; pulled forward because the session glue made them cheap |
| Keymap engine (labwc patch, inert by default) | **Not needed for v0.1** — ADR-0008's Linux-default keyboard shipped instead; xkb translation is client-side | Mac-profile translation patch remains queued (ADR-0003 Phase-2) |
| ibde-terminal v1 feature-complete | **PTY↔vte↔grid↔atlas↔xdg window; full input path incl. Ctrl/Shift; tabs/splits/vi-mode NOT yet** | "works properly, beginner friendly" ✓; "power layer" is Phase 2 (TERMINAL-SPEC) |
| debs building in CI | **CI: fmt/clippy/test/build + full interactive workout job (typed text + dock click + PSS gates)** | .deb packaging NOT started — first Phase-2 item with the apt repo |
| Floor bench ≤ 60 MB | **33.4 MB PSS measured on the floor profile** (labwc 20.1 + shell 6.4 + terminal 6.9) | ADR-0002 PASS with 33 % headroom; waybar baseline 55.1 MB for comparison |

## Evidence index

- `docs/bench/floor-session.png` + BASELINE.md — floor-profile session
- `docs/bench/desktop-v1.png` — composed desktop (sway)
- `docs/bench/labwc-visual.png` — labwc target-compositor proof
- `docs/bench/kb-e2e.png`, `dock-launch.png`, `ibde-menu.png`,
  `spotlight.png` — each interaction path, pixel-verified
- `bench/results/workout.json` — CI-consumable gate output
- 32 commits, 23 unit tests, clippy/fmt clean throughout

## Bugs found and killed (each with a lasting artifact)

1. wl_seat `missing_capability` kills clients that request absent
   keyboards → capability-gated binding, now documented in the glue
2. `dispatch_pending` never reads the socket → canonical poll loop
   (terminal + shell), the single most important pattern this phase
3. xkb↔evdev +8 keycode offset (bit us twice) → documented on both
   sides
4. sway-headless layer-height clamp → full-anchor + self-centered
   cards

## Known gaps entering Phase 2

- No .deb packaging / apt repo; no `ibde-session` systemd units
- Terminal: no tabs/splits/search/scrollback-UI/vi-mode; no ligatures
- No notifications daemon, OSD, lock screen, settingsd
- Bar keyboard focus is exclusive-on-bar (v0 shortcut approximation);
  global bindings need compositor cooperation (ADR-0008/0003)
- Single-buffer rendering throughout (frame-callback pacing is
  deliberate; double-buffering when release-tracking lands)
- Mac-profile keymap translation patch not yet written
- Real hardware (Pi Zero 2 W) still on order — every number here is
  QEMU-floor-profile, honestly labeled

## Phase 2 entry (per ROADMAP): macOS-spec shell completion

notifications + OSD + lock, settingsd + Settings app skeleton, theme/
icons, terminal power layer, deb packaging + apt repo, keymap patch.
