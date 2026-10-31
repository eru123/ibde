# libibde-ui Phase 0 spike report

ADR-0004 go/no-go evidence. Updated as results land.

## Environment

- Host: Debian 13 (trixie), gcc, wayland 1.23, cairo 1.18, pango 1.56
- Spike: one wlr-layer-shell top bar (1280×32), wl_shm ARGB8888 buffer,
  cairo + pango, animated rounded pill + text, frame-callback driven
  (single-buffered; production shell will be double-buffered)

## Offscreen render cost (no compositor, pure render path)

`spike --offscreen 600` (host, amd64):

```
OFFSCREEN frames=600 total_ms=72 avg_ms=0.120 rss_kb=18700
```

- **0.12 ms/frame** for the bar at 1280×32 — the software path costs
  ~0.7 % of a 60 fps frame budget. Render cost is a non-issue at this
  surface size; the frame-callback round-trip will dominate real FPS.
- RSS 18,700 KB is whole-process RSS **including shared library pages**
  (pango/fontconfig/glib/cairo). PSS (marginal cost) is lower and
  amortizes across shell + terminal once both run. Tracked in bench v1
  with smem-style PSS accounting; ADR-0004's harfbuzz escape hatch fires
  only if the 50 MB budget is threatened after real measurements.

## Runtime (headless sway, pixman renderer, ephemeral container)

Pending — results appended when the container run completes.

## Runtime (floor VM: labwc, pixman, 1 vCPU / 512 MB)

Pending — results appended from `docs/bench/BASELINE.md` run.
