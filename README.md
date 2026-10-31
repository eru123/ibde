# IBDE — The familiar feel, brought into Linux.

IBDE is a from-scratch desktop environment for Debian-based systems and the
Raspberry Pi, built for machines the modern desktop world forgot.

## Principles

- **Familiar by design** — the polish and clarity people love about macOS,
  with **Linux keyboard conventions by default**. We copy the feel and the
  appearance, not the keys (ADR-0008).
- **Runs at the floor** — a fully interactive desktop on a 512 MB / 1-vCPU
  Raspberry Pi-class machine; the DE itself idles under 50 MB (ADR-0002).
- **Zero heavyweight toolkits in the session-critical path** — the shell is
  our own C toolkit (`libibde-ui`) over Wayland/wlroots. No GTK in the
  shell, settings daemon, or greeter.
- **Beginner-first, power-user-complete** — everything discoverable,
  everything keyboard-drivable.
- **The terminal is a flagship** — `ibde-terminal` is a first-party core
  component, not an afterthought (ADR-0011).

## Status

Phase 0 (foundations) started 2026-10-31. All owner decisions are ratified
in `docs/adr/`. See `docs/ROADMAP.md` for the plan and
`docs/bench/BASELINE.md` for measurements.

## Repository layout

| Path | Contents |
|---|---|
| `docs/` | ADRs, roadmap, UX spec, terminal spec, contracts, bench reports |
| `ui/` | `ibde-ui` — retained-mode Rust toolkit crate (GLES2 + pixman backends); `spike/` (C) and `spike-rs/` are the Phase 0 comparison artifacts |
| `shell/` | `ibde-shell` — menubar, dock, launcher, notifications, lock, OSD |
| `terminal/` | `ibde-terminal` — flagship terminal (`vte` crate + ibde-ui) |
| `comp/` | compositor integration (labwc adopt → smithay/labwc fork → own) |
| `settings/` | `ibde-settingsd` + settings schema |
| `session/` | session bootstrap, systemd user units, greeter |
| `theme/` | GTK app theme, icon theme, wallpapers, accents |
| `bench/` | `ibde-bench` — floor-profile performance harness + test images |
| `packaging/` | Debian packaging |

## License

Apache-2.0 (ADR-0009). See [LICENSE](LICENSE).
