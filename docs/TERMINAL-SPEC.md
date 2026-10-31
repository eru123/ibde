# ibde-terminal — Specification v0.1

Owner decision (2026-10-31): *"the terminal is the most important feature
of this"* — it must work flawlessly, offer advanced features for power
users, and stay beginner friendly. First-party component from Phase 1
(ADR-0011).

## Core decisions

- **`vte` crate** (pure-Rust VT parser, alacritty-proven) for emulation —
  no libvterm FFI, no VTE/GTK (ADR-0012). Rendering via `ibde-ui`'s
  **text-grid fast path** (ADR-0004): glyph atlas, damage-tracked partial
  redraw, O(1) scrolling.
- Budget: **≤ 15 MB RSS typical**, ≤ 25 MB with 10k scrollback. (VTE+GTK3
  costs 40–60 MB — disqualifying at the floor.)

## Beginner defaults (works on day one)

- Bundled JetBrains Mono (OFL-1.1) with correct CJK/emoji fallback chain;
  sane size on small screens (auto from DPI).
- Working copy/paste (`Ctrl+Shift+C/V` + right-click), scrollback, URLs
  detected + open (OSC 8 hyperlinks too).
- Tabs: `Ctrl+Shift+T/W`, reorderable, close confirmation when processes
  are running ("never lose work by accident").
- Right-click menu in plain language; Settings pane (font, palette,
  keys) writing ADR-0006 keyfiles; themes follow system light/dark +
  accent automatically.

## Emulation baseline

24-bit truecolor; full DEC/ECMA-48 coverage libvterm provides; validated
against `tmux`, `vim`, `htop`, `less`, `ssh` to remote hosts; proper
mouse reporting; bracketed paste; focus reporting.

## Power layer

- **Splits** (`Alt+Shift+arrows`), resizable, persistent per tab.
- **vi-mode copy/search**: tmux-style navigate/select/yank without mouse.
- **Command palette** (`Ctrl+Shift+P`): fuzzy access to every action and
  setting.
- **Quake drop-down**: global hotkey terminal overlay.
- **Full key rebinding**, per-profile (Simple/Power), import/export.
- **Shell integration**: OSC 7 (cwd tracking → spotlight file hints),
  OSC 9 (desktop notifications via our notifyd), OSC 52 (clipboard).
- Unlimited scrollback (config), scrollback-to-file dump, reflow on
  resize.
- Ligatures: optional, **off at floor**.
- Sixel: stretch goal (Phase 4+).

## Performance gates (bench floor profile, CI-enforced)

| Gate | Target |
|---|---|
| RSS typical / 10k scrollback | ≤ 15 MB / ≤ 25 MB |
| `cat` of a 100 MB file | no perceptible stalls; throughput reported per run |
| Scroll smoothness @ 1 vCPU, pixman | ≥ 30 fps (60 fps comfort tier) |
| Typing echo latency | ≤ 16 ms (instrumented frame timestamps) |

These gates are part of ADR-0002's floor contract.
