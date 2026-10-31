# IBDE Contracts

Frozen boundaries between workstreams. A change to anything in this
directory requires an ADR and coordinator sign-off; releases freeze a
contract version.

| Contract | Status | Consumers |
|---|---|---|
| `settings-schema.draft.json` | draft | settingsd, shell, terminal, settings app, bench |
| `libibde-ui` public headers | draft (Phase 1 W3) | shell, terminal, greeter |
| D-Bus interfaces (`org.ibde.Settings1`, `org.ibde.Shell1`, `org.freedesktop.Notifications`, `com.canonical.AppMenu.Registrar`) | draft | shell, settingsd, apps, toolkit tests |
| Skin/format files (`.ibdeskin`) | draft (Phase 1 W3) | theme, ui |
| Keymap table (profiles, detection hooks) | draft | comp, settingsd, settings app |
| Bench output JSON schema | v0 | bench, CI gates |

## Rules

1. Cross-stream code touches **only** these surfaces — never internals.
2. Contracts are versioned (`v0`, `v1`, …); deprecations announced one
   release ahead.
3. Every contract ships with a conformance test next to its definition.
