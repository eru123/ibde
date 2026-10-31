# ADR-0008: Keyboard model — Linux by default, Mac as a detected option

- Status: Accepted (owner decision, 2026-10-31)
- Owner's words: *"by default should be in-favor of linux settings, not
  Mac, we only copy the Mac feel and appearance, not the Mac itself."*

## Decision

1. **The default keyboard profile is "Linux"**: Ctrl+C/V/X/Z, Alt+Tab
   cycles **windows**, F11 fullscreen — stock conventions. Nothing
   surprises a Linux user or fights a GTK/Qt app.
2. **A "Mac" profile exists** (opt-in): Super becomes ⌘ via compositor-
   side **Super→Ctrl translation** for shortcuts the shell does not
   consume (ADR-0003 fork), app-grouped cycling, `Super+M/W/Q/H`
   minimize/close/quit/hide semantics.
3. **Auto-detection**: `ibde-settingsd` inspects input devices via
   udev/evdev at session start and on hotplug. When an Apple keyboard is
   detected (vendor/device metadata), the Welcome flow *suggests* the Mac
   profile. Suggestion only — never a silent override.
4. **Layout auto-configuration** from udev + locale; everything
   overridable in the Keyboard settings pane, which also shows detection
   status.

## Consequences

- The translation engine ships in Phase 1 but is **inert unless the Mac
  profile is active** — the engine is not the default experience.
- Spotlight keeps both `Ctrl+Space` and `Super+Space` so neither profile
  loses it.
- We copy the feel and the appearance — never the keys — unless asked.
