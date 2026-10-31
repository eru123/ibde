# ADR-0005: ibde-shell — one process, degradable modules

- Status: Accepted
- Date: 2026-10-31

## Decision

The entire shell surface — menubar, dock, launcher/spotlight,
notifications, OSD, app-switcher, control center, lock screen — runs as
**one process** with internal modules: one Wayland connection, one render
context.

- Module API: `init/destroy`, internal event bus, config subscriptions.
- Each module owns its layer-surface(s); a failed module degrades (its
  surface disappears) without killing the process.
- Supervisor + systemd user unit (`Restart=on-failure`); state serialized
  so a restarted shell restores dock pins, notification history, and
  profile within seconds.
- D-Bus services hosted: `org.freedesktop.Notifications`,
  `com.canonical.AppMenu.Registrar` (compat — ADR-0007),
  `org.ibde.Shell1` (toggles, profiles, keymap).

## Why single process

Each extra process costs a Wayland connection, buffers, and ≥ 1–3 MB;
ten shell surfaces as separate clients would spend 10–30 MB and multiply
IPC. One process is the largest single RAM win in the architecture.
