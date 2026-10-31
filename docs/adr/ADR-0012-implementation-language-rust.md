# ADR-0012: Implementation language — Rust for all first-party components

- Status: Accepted (owner decision, 2026-10-31: *"are we not doing this in Rust?"*)
- Supersedes: the C implementation choice embedded in ADR-0004, ADR-0005,
  ADR-0011 (their architecture stands; only the language changes).

## Decision

All first-party IBDE components are written in **Rust**:

- `ibde-ui` (toolkit crate): cairo/pango via `cairo-rs`/`pango`/`pangocairo`;
  Wayland via `wayland-client` + `wayland-protocols-wlr`; GLES2 backend via
  `glow`/`khronos-egl` when Phase 1 adds it.
- `ibde-shell`: layer-shell client(s), single process (ADR-0005 unchanged).
- `ibde-terminal`: `vte` crate (pure-Rust VT parser, alacritty-proven) —
  **replaces libvterm and its FFI** (updates ADR-0011 / TERMINAL-SPEC).
- `ibde-settingsd`: `zbus` for D-Bus, serde + JSON Schema for validation
  (ADR-0006 unchanged), INI keyfiles.
- `ibde-bench` tooling, greeter, session utilities: Rust.

## Compositor path (ADR-0003 updated in wording only)

- Phase 1–2 **adopt stock labwc (C) unmodified** — we only execute it, so
  its language is irrelevant; no FFI into it from our side (we speak
  Wayland protocols and D-Bus, both language-neutral).
- Phase 2 keymap engine: smallest viable path remains a ~100-line C patch
  against labwc; alternatively an early smithay prototype — decided by
  Phase 2 spike data.
- Phase 3 endgame preference shifts to **`ibde-comp` on smithay** (pure
  Rust compositor library, COSMIC-comp/niri-proven), with the labwc-fork
  route as fallback. This completes a 100 % first-party Rust stack.

## Rationale

1. **Memory safety under parallel agent authorship.** The project is built
   by many concurrent agent workstreams; Rust's compiler enforces the
   frozen contracts (ADR process) mechanically — borrow checking, module
   visibility, trait bounds — where C relies on review and sanitizers.
   Whole bug classes (UAF, leaks, data races) drop out.
2. **Ecosystem is ready**: `wayland-client`/`wayland-protocols-wlr`
   (layer-shell, foreign-toplevel, screencopy), smithay, `zbus`, `vte`,
   `rustybuzz` (pure-Rust ADR-0004 escape hatch), `cairo-rs`/`pango`.
3. **Budget impact ≈ 0**: no GC, no runtime; RSS is dominated by shared
   GUI libraries identical to the C stack. Verified same-day by the Rust
   spike vs the C spike (see `docs/bench/SPIKE-REPORT.md`).
4. Debian packaging via `dh-cargo` is established; cross-compiling
   (amd64/arm64) via cargo targets is simpler than autotools.

## Consequences

- Toolchain: rustc (stable) in dev/CI; MSRV recorded in the workspace
  manifest.
- `unsafe` confined to: EGL/GLES2 backend glue and any unavoidable FFI —
  each instance documented with a `// SAFETY:` comment.
- The C spike is retained under `ui/spike/` as the comparison baseline.
