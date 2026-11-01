# Phase 2 exit review — v0.2

Date: 2026-11-01 · Agent-authored from measured evidence; owner
override welcome.

## Verdict: **EXIT GRANTED** — tag `v0.2` ("a desktop you can use")

IBDE now covers the full daily-driver surface: menu, dock, launcher,
notifications, OSD, lock skeleton, installable packages, and a terminal
with the complete power-user layer — all verified end-to-end under the
headless rig, all still inside the floor memory envelope.

## Delivered in Phase 2

| Feature | Evidence | Notes |
|---|---|---|
| Notifications daemon | real `notify-send` → toast → auto-expire, pixel PASS (`docs/bench/notification.png`) | org.freedesktop.Notifications via zbus, in-shell (ADR-0005), +2.6 MB RSS |
| `.deb` packaging + `ibde-session` | fresh-container install test: binaries run from `/usr/bin`, session entry + wayland-sessions desktop (`docs/bench/deb-installed.png`) | cargo-deb; apt repo blocked only on the signing-key decision |
| OSD (volume/brightness) | media keys → pill with fill → timed fade, both PASS (`docs/bench/osd.png`) | fade machine in the shell poll loop |
| Terminal: scrollback | Shift+PgUp → history view, PASS | 10k-row cap (ADR-0011) |
| Terminal: search | Ctrl+Shift+F → query → Enter jumps to match, PASS (`docs/bench/search.png`) | history+live scan, [i/n] counter |
| Terminal: tabs | Ctrl+Shift+T/W, Ctrl+PgUp/Dn, per-tab PTY, PASS (`docs/bench/tabs.png`) | multi-PTY refactor; spawn deferred to main loop |
| Lock screen (skeleton) | state machine verified in logs; visual deferred | ext-session-lock-v1 in glue; NO AUTH in v0 — PAM is Phase 3 |

## Bugs killed this phase (lasting artifacts)

1. kb-inject actions now execute in flag order on one connection
   (order-independence + seat-capability-flap bugs)
2. Never render/spawn inside dispatch callbacks — needs_render flag
   (also the pattern for the whole shell)
3. Missing consts parse as catch-all bindings — the search
   "nothing works" bug; clippy's unreachable/snake-case warnings were
   the fingerprint
4. History rows re-pad on resize (real OOB panic from mid-test retiling)
5. ext-session-lock protocol re-read: client-owned wl_surface into
   get_lock_surface; non-nullable per-output arg

## Honest open items (Phase 3)

- PAM auth for the lock; lock visual sign-off on hardware
- Global shortcuts still bar-exclusive-keyboard (ADR-0008 endgame:
  compositor cooperation or the ibde-comp fork)
- Settings app + settingsd (D-Bus/serde planned) not started
- Theming beyond built-ins; icon theme; wallpaper picker
- Double-buffered rendering throughout
- 24h soak + memory-drift gate in CI (designed, not yet scheduled)
- apt repo (signing key — owner decision), Pi hardware run

## Memory position (unchanged envelope)

Floor-profile full session: 33.4 MB PSS (labwc 20.1 + shell 6.4 +
terminal 6.9; shell now includes zbus/notifications). The v0.1 verdict
stands with room for the Phase-3 additions.
