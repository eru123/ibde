# IBDE Roadmap

Updated 2026-10-31 after owner decisions (ADRs 0001–0011). Full-time
execution with parallel agent workstreams; critical path: bench →
`libibde-ui` → shell modules / terminal.

## Phases

| Phase | Time | Delivers | Exit gate |
|---|---|---|---|
| **0 Foundations** | 2 wks | repo, CI, contracts, `ibde-bench` floor profile, labwc baseline numbers, `libibde-ui` spike | nightly bench publishing; go/no-go on toolkit bet |
| **1 Shell skeleton + flagship terminal** | 6 wks | `libibde-ui` core, shell v0 (bar, dock, launcher), keymap engine (inert — Linux default), **`ibde-terminal` v1** | v0.1 internal alpha; floor bench ≤ 60 MB with written path to 50 |
| **2 macOS-spec shell** | 6–8 wks | global menu, notifications, OSD, lock screen, control center, settingsd + Settings app, theme/icon/wallpaper v1, Welcome, first `.deb`s + apt repo | v0.5 public alpha (Debian 13 amd64/arm64) |
| **3 Own compositor + images** | 6–8 wks | `ibde-comp` fork (animations, overview, menubar auto-reveal, screenshots), `ibde-greeter`, Pi image (pi-gen), x86 ISO | v0.9 beta images booting on floor hardware |
| **4 Hardening → 1.0** | 4–6 wks | watchdog/soak hardening, full keyboard nav, i18n, docs site, a11y pass | 1.0 |

## Phase 0 — week by week

- **W1**: repo + layout + ADRs 0001–0011 (done at bootstrap); bench v0
  (QEMU 1 vCPU / 512 MB, NoCloud-net seed, serial capture); baseline:
  stock labwc + waybar RSS; **order Pi Zero 2 W + PSU + SD card** (human).
- **W2**: bench v1 interactive workout + JSON output; `libibde-ui` spike:
  one layer-shell window, animated rounded rect + pango text, RSS + FPS on
  GLES2 and pixman; keymap-engine feasibility patch vs labwc; go/no-go.

## Phase 1 — week by week

- **W3–4**: `libibde-ui` core (widget tree, styling, text, animation,
  input, **text-grid fast path**) + render-hash tests; shell skeleton +
  bar module (IBDE menu, clock, status via UPower/NetworkManager).
- **W5**: dock v1 (pin/launch/indicators/minimize-to-dock) via
  foreign-toplevel; launcher v1 (`.desktop` parse + fuzzy); **terminal
  grid rendering + vtm core loop**.
- **W6**: keymap engine (labwc patch, inert by default — ADR-0008);
  switcher overlay (Linux default: window cycling); OSD; notifyd v1;
  **ibde-terminal v1 feature-complete for daily use**.
- **W7**: session (systemd user units, xdg-autostart, env), greetd +
  interim greeter, settingsd v1 + CLI, `.deb`s building in CI.
- **W8**: soak + polish, freeze contracts v0, tag **v0.1**.

## Workstreams (agent organization)

| Stream | Owns |
|---|---|
| W1 compositor | `comp/` — labwc wrap, patch series, fork |
| W2 toolkit | `ui/` — `libibde-ui`, render-hash tests |
| W3 shell | `shell/` — bar, dock, launcher, notifyd, OSD, switcher, lock |
| W4 system | `settings/`, `session/`, greeter, keymap engine |
| W5 terminal | `terminal/` — flagship terminal |
| W6 design | `theme/` — light theme, accents, icons, wallpapers, motion |
| W7 release | `bench/`, CI, packaging, apt repo, images |

Rules: cross-stream surface = frozen contract under `docs/contracts/`;
changes need an ADR; nightly bench gate on main.
