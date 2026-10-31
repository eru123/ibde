# ADR-0010: Target matrix

- Status: Accepted
- Date: 2026-10-31

| Tier | Devices | Base | Renderer | Notes |
|---|---|---|---|---|
| Floor | Pi Zero 2 W / Pi 3 (512 MB–1 GB), ~2008 x86 | Debian 13, RPi OS 64 | VideoCore GLES2, i915, pixman | bench-gate target; animations auto-off; zram default |
| Comfort | Pi 4/5, 2012+ x86 | Debian 13 | GLES2+ | all effects on |

- **Architectures**: amd64, arm64.
- **Bases**: Debian 13 (trixie) primary; RPi OS 64-bit (trixie-based) for
  Pi images (pi-gen, Phase 3); x86 ISO Phase 3–4 (live-build).
- **CI**: floor profile (QEMU 1 vCPU / 512 MB, both renderers) nightly;
  per-MR fast lane (build + unit tests) on amd64 + arm64 cross.
