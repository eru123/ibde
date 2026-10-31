# ADR-0007: Global menu via D-Bus appmenu, canonical-compatible registrar

- Status: Accepted
- Date: 2026-10-31

## Decision

The menubar is a layer-shell client (ADR-0005). Focused-app tracking
comes from `wlr-foreign-toplevel-management` (app-id). Menus arrive over
D-Bus: our shell hosts a registrar implementing the
**`com.canonical.AppMenu.Registrar`** interface, so the existing
`appmenu-gtk-module` works unpatched for GTK3 apps.

## Coverage expectations (honest)

- **GTK3 apps**: real global menus, zero patches.
- **Qt apps**: same D-Bus menu protocol; stretch goal.
- **GTK4/CSD, Electron**: degrade to app name + Quit. We never
  synthesize fake menus.

## Consequences

Zero patches to third-party components; degradation is graceful and rare
in practice on the lightweight app set a floor device runs.
