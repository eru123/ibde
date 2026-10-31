# ADR-0006: Settings — schema-validated keyfiles, not gsettings

- Status: Accepted
- Date: 2026-10-31

## Decision

- **Storage**: INI keyfiles. System defaults `/etc/ibde/`, user overrides
  `~/.config/ibde/`. Human-readable, greppable, rsync-friendly.
- **`ibde-settingsd`** owns schemas (JSON Schema per key: type, range,
  enum — see `docs/contracts/settings-schema.draft.json`), validates
  writes, runs versioned migrations, and emits change notifications on
  D-Bus (`org.ibde.Settings1`); shell modules subscribe instead of
  polling.
- **CLI**: `ibde-settings get/set/list/monitor` — the power user's
  escape hatch.
- No gsettings/dconf dependency anywhere in the DE.

## Consequences

Config is trivially scriptable and snapshot-able; the cost is owning a
small, well-bounded schema/migration layer.
