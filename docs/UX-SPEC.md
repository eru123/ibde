# IBDE UX Specification v0.1

Owner-ratified 2026-10-31. Governs all shell work. Changes via ADR.

## 0. Identity

**"The familiar feel, brought into Linux."** The polish, clarity and calm
of macOS — with Linux conventions underneath. **We copy the feel and the
appearance, never the keys, unless the user asks** (ADR-0008).

## 1. Appearance

- **Default: Light.** Dark is one toggle away; "auto at sunset" later.
- **Full accent palette from day 1**: graphite (default) + red, orange,
  yellow, green, teal, blue, purple, pink. Accents flow to selection,
  controls, dock indicators, terminal cursor, spotlight highlight.
- Typography: system UI face (bundled, OFL); text-grid face JetBrains Mono
  (OFL). Text scaling in Phase 3.
- Corners: 12 px window radius, soft shadows (comfort tier), none at floor.

## 2. Menubar (top)

| Element | Behavior | Phase |
|---|---|---|
| IBDE menu (logo, leftmost) | About This Machine · System Settings… · Recent Items (stretch) · Sleep · Restart… · Shut Down… · Lock `Ctrl+Alt+l` · Log Out | 2 |
| App menus (global menu) | Real menus for GTK3 apps via D-Bus appmenu (ADR-0007); apps without exported menus show name + Quit only | 2 |
| Status area (right) | network · bluetooth · volume · battery · control center · clock (far right) | 2 |
| Clock click | Notification Center + DND toggle | 2 |
| Autohide | Optional (power profile, small screens) | 2 |

## 3. Dock (bottom, centered, translucent)

- Pin/unpin, drag-reorder, launch bounce, running-app indicator dots,
  right-click menus, minimize-to-dock (no taskbar anywhere), auto-hide
  option, Trash at right end (stretch).
- **Magnification**: comfort tier only; off at floor; off by default on
  screens < 1024 px wide.
- Default pins: Files, Terminal, Settings (+ Browser on x86 images).

## 4. Spotlight (`Ctrl+Space`, `Super+Space` alias)

Centered panel, live results: apps (fuzzy) → files (`plocate`) →
calculator (own expression parser) → web-search fallback (opens browser;
comfort tier). Keyboard-first: arrows, `1–9` quick-pick, Esc.
Clipboard history (power profile, stretch).

## 5. Window management

| Behavior | Mapping | Phase |
|---|---|---|
| Traffic lights, top-left | red close · yellow minimize-to-dock · green maximize (fill workspace); `Alt+green` fullscreen; double-click titlebar maximize | 1–2 |
| Window cycling | **Alt+Tab = windows (Linux default)**; Mac profile switches to app-grouped cycling with `` Alt+` `` within-app | 2 |
| Hide app | Mac profile only (`Super+H` hides all windows of app) | 2 |
| Minimize / close / quit | Linux defaults (`Alt+F4` close, etc.); Mac profile adds `Super+M/W/Q` via translation | 2 |
| Workspaces | horizontal strip, `Ctrl+Alt+←/→`; Mission Control overview `F3` | 3 |
| Fullscreen | menubar hidden; re-reveals on hover at top edge | 3 |
| Snap | `Super+arrows` halves/quarters — power profile default, optional otherwise | 2 |

## 6. Keyboard model (ADR-0008)

1. **Default profile "Linux"**: Ctrl+C/V/X/Z, Alt+Tab windows — stock
   conventions, zero surprises for GTK/Qt apps and existing Linux users.
2. **"Mac" profile (opt-in)**: Super becomes ⌘ — compositor-side
   Super→Ctrl translation for shortcuts our shell doesn't consume;
   app-cycling switcher; `Super+M/W/Q/H` semantics.
3. **Auto-detection**: udev/evdev inspection at session start and on
   hotplug. Apple keyboards → Welcome flow *suggests* the Mac profile.
   Suggestion only, never a silent override.
4. Layout auto-configured from udev + locale; everything overridable;
   keyboard settings pane exposes profiles + detection status.

## 7. Control Center (menubar popover) — Phase 2

Wi-Fi list, Bluetooth, DND, dark-mode toggle, volume slider, brightness,
lock, logout, power.

## 8. Notifications — Phase 2

Top-right banners under the menubar, 5 s auto-dismiss, stack, click-through
to app, action buttons (later), collected in Notification Center (clock
click), DND with allowlist (later).

## 9. Lock screen — Phase 2

Blurred wallpaper, large clock, avatar, password (ext-session-lock-v1).
Greeter (Phase 3) shares the same aesthetic.

## 10. Screenshots — Phase 3

`Ctrl+Print` full screen, `Shift+Ctrl+Print` region (crosshair; window
pick with Space), panel with options later. Thumbnail toast →
`~/Pictures/Screenshots`.

## 11. First-run Welcome — Phase 2

Language → appearance (light/dark + accent) → dock preferences →
**keyboard profile** (Linux default preselected; Mac suggested if an Apple
keyboard is detected) → profile choice (**Simple / Power**) → tour.

## 12. Profiles

- **Simple** (default): discoverable, calm, no power affordances.
- **Power**: keyboard-first (spotlight + command palette in terminal),
  snap keys, autohide options, full shortcut editor, text configs exposed
  in Welcome, clipboard history.

## 13. Settings app (GTK3, "System Settings")

Phase 2: Appearance, Desktop & Dock, Wallpaper, Displays, Sound, Network,
Keyboard (visual shortcut editor + profiles), About.
Phase 3–4: Bluetooth, Battery, Users, Printers, Date & Time, Mouse,
Language & Region, Privacy.

## 14. Desktop

Wallpaper + right-click menu at v1 (New Folder, Change Wallpaper, Display
Settings); own desktop-icons renderer in Phase 3.

## 15. Accessibility & i18n

Full keyboard navigation everywhere from day 1; gettext from day 1; RTL
mirroring in `libibde-ui` layout; text scaling Phase 3; on-screen keyboard
and a11y bus after 1.0 (honest gap vs GNOME — roadmapped, not hidden).

## 16. Tier auto-tuning

At session start: FPS probe for 500 ms → animations/magnification off
below threshold; < 2 GB RAM → zram recommended via `ibde-defaults`;
compositor effects disabled at floor.
