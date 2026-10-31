# libibde-ui Phase 0 spike report

ADR-0004 go/no-go evidence; C-vs-Rust comparison backing ADR-0012.
Host: Debian 13 (trixie) amd64, gcc / rustc 1.97.1, wayland 1.23,
cairo 1.18, pango 1.56.

## The spike

One wlr-layer-shell top bar (1280×32), wl_shm ARGB8888 buffer, cairo +
pango, animated rounded pill + text, frame-callback driven,
single-buffered (production shell will be double-buffered).
Implementations: `ui/spike/` (C) and `ui/spike-rs/` (Rust) — same design,
same measurements.

## Results

| Metric | C spike | Rust spike |
|---|---|---|
| Offscreen render, avg ms/frame (600 frames) | 0.120 | 0.217 |
| Headless sway (pixman), FPS | 63 | 63 |
| Headless sway (pixman), RSS after settle | 12,212 KB | 12,784 KB |
| Offscreen RSS (whole process incl. shared libs) | 18,700 KB | 19,148 KB |

## Reading

- **Render cost is a non-issue at bar sizes** in either language
  (≤ 0.22 ms against a 16.7 ms frame budget); real FPS is bounded by the
  compositor's frame clock, which pinned both at 63.
- **RSS parity within ~0.6 MB.** RSS is dominated by the same shared
  libraries (pango/cairo/glib); Rust std adds nothing measurable.
  ADR-0004's harfbuzz/rustybuzz escape hatch stays available if the
  50 MB budget ever gets tight after PSS accounting.
- **Verdict (ADR-0012): Rust confirmed** — same performance envelope,
  memory safety under parallel agent authorship, and the ecosystem
  (`wayland-client`, `wayland-protocols-wlr`, `vte`, `zbus`, smithay
  endgame) covers everything the C path hand-rolled.

Runtime environment for the sway rows: ephemeral `debian:trixie`
container, `WLR_BACKENDS=headless WLR_RENDERER=pixman`,
`ibde-sway-test` image (bench/docker/sway-test).

## Known spike limitations (to fix in the real toolkit)

- Single buffer (no buffer-release handling) — spike only.
- Full-surface damage every frame; real toolkit does damage tracking.
- No fractional-scale / output-scale handling.
- pango layout created per frame (production caches layouts).
