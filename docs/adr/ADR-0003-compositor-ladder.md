# ADR-0003: Compositor — the adopt → fork → own ladder

- Status: Accepted
- Date: 2026-10-31

## Decision

- **Phase 1 — adopt**: stock `labwc` (wlroots; Openbox-compatible config)
  drives the session. Our shell is a set of layer-shell clients; nothing
  requires a fork yet.
- **Phase 2 — surgical fork** (`ibde-comp`): only patches layer-shell
  clients cannot implement alone: keymap translation engine (ADR-0008),
  app-hide for the Mac profile, app-grouped cycling, switcher overlay
  hooks. Kept as a small documented patch series, rebased while upstream
  labwc moves.
- **Phase 3 — own**: window animations, Mission-Control-style overview,
  menubar auto-reveal in fullscreen, screenshot engine. At that point the
  fork is a product, not a patch set.

## Rationale

wlroots solves DRM/KMS, input, and the protocol surface (layer-shell,
foreign-toplevel, session-lock, screencopy, output-management, gamma).
Writing that layer buys no identity and carries enormous risk.

## Consequences

Phase 1 shell progress is decoupled from compositor work — parallel
workstreams from week 3. Every fork patch must map to a feature clients
cannot implement alone; no drive-by divergences.
