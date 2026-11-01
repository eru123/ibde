//! Keyboard input (ADR-0004 backend-agnostic input path).
//!
//! [`Keyboard`] owns the xkbcommon keymap + state (fed by the session
//! glue's `wl_keyboard` events) and translates keycodes into
//! `(keysym, UTF-8)`. [`key_to_terminal_bytes`] adds the terminal's ANSI
//! encoding for special keys. Everything here is unit-testable without a
//! compositor via `Keyboard::from_names`.

use std::os::fd::OwnedFd;

use xkbcommon::xkb;
use xkbcommon::xkb::keysyms;

/// A translated key event.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub keysym: u32,
    /// UTF-8 per xkb state (modifiers already applied — Ctrl+C arrives
    /// as the 0x03 control byte)
    pub utf8: String,
}

pub struct Keyboard {
    _ctx: xkb::Context,
    keymap: xkb::Keymap,
    state: xkb::State,
    ctrl_index: xkb::ModIndex,
    shift_index: xkb::ModIndex,
    alt_index: xkb::ModIndex,
}

impl Keyboard {
    /// Build from layout names — for tests and headless use.
    pub fn from_names(layout: &str) -> Option<Self> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &ctx,
            "",
            "",
            layout,
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )?;
        let ctrl_index = keymap.mod_get_index("Control");
        let shift_index = keymap.mod_get_index("Shift");
        let alt_index = keymap.mod_get_index("Mod1");
        let state = xkb::State::new(&keymap);
        Some(Self {
            _ctx: ctx,
            keymap,
            state,
            ctrl_index,
            shift_index,
            alt_index,
        })
    }

    /// Build from a compositor keymap fd (`wl_keyboard.keymap`).
    /// Takes ownership of the fd.
    pub fn from_fd(fd: OwnedFd, size: usize) -> Option<Self> {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        // SAFETY: fd is a valid, freshly-received keymap memfd of
        // exactly `size` bytes; mapping concerns are the same as every
        // Wayland client's keymap handling.
        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &ctx,
                fd,
                size,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        }
        .ok()??;
        let ctrl_index = keymap.mod_get_index("Control");
        let shift_index = keymap.mod_get_index("Shift");
        let alt_index = keymap.mod_get_index("Mod1");
        let state = xkb::State::new(&keymap);
        Some(Self {
            _ctx: ctx,
            keymap,
            state,
            ctrl_index,
            shift_index,
            alt_index,
        })
    }

    /// wl_keyboard.modifiers payload.
    pub fn update_mask(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        let _ = self
            .state
            .update_mask(depressed, latched, locked, group, group, group);
    }

    /// Modifier mask with only Ctrl set (for tests and callers that
    /// synthesize modifiers).
    pub fn ctrl_mask(&self) -> u32 {
        1 << self.ctrl_index
    }

    pub fn ctrl_held(&self) -> bool {
        self.state
            .mod_index_is_active(self.ctrl_index, xkb::STATE_MODS_EFFECTIVE)
    }

    pub fn shift_held(&self) -> bool {
        self.state
            .mod_index_is_active(self.shift_index, xkb::STATE_MODS_EFFECTIVE)
    }

    pub fn alt_held(&self) -> bool {
        self.state
            .mod_index_is_active(self.alt_index, xkb::STATE_MODS_EFFECTIVE)
    }

    /// Translate an evdev keycode (raw wl_keyboard key + 8 handled by
    /// the caller).
    pub fn translate(&self, keycode: u32) -> KeyEvent {
        let kc = xkb::Keycode::from(keycode);
        let sym = self.state.key_get_one_sym(kc);
        let keysym: u32 = sym.into();
        let utf8 = self.state.key_get_utf8(kc);
        KeyEvent { keysym, utf8 }
    }

    pub fn keymap(&self) -> &xkb::Keymap {
        &self.keymap
    }
}

/// ANSI byte sequences for special keysyms; None for printable input.
pub fn keysym_terminal_bytes(keysym: u32) -> Option<Vec<u8>> {
    let v: &[u8] = match keysym {
        keysyms::KEY_Return | keysyms::KEY_KP_Enter => b"\r",
        keysyms::KEY_BackSpace => &[0x7f],
        keysyms::KEY_Tab => &[0x09],
        keysyms::KEY_Escape => &[0x1b],
        keysyms::KEY_Up => b"\x1b[A",
        keysyms::KEY_Down => b"\x1b[B",
        keysyms::KEY_Right => b"\x1b[C",
        keysyms::KEY_Left => b"\x1b[D",
        keysyms::KEY_Home => b"\x1b[H",
        keysyms::KEY_End => b"\x1b[F",
        keysyms::KEY_Prior => b"\x1b[5~",
        keysyms::KEY_Next => b"\x1b[6~",
        keysyms::KEY_Insert => b"\x1b[2~",
        keysyms::KEY_Delete => b"\x1b[3~",
        keysyms::KEY_F1 => b"\x1bOP",
        keysyms::KEY_F2 => b"\x1bOQ",
        keysyms::KEY_F3 => b"\x1bOR",
        keysyms::KEY_F4 => b"\x1bOS",
        _ => return None,
    };
    Some(v.to_vec())
}

/// Terminal encoding of a key event: special keys → ANSI sequences,
/// otherwise the xkb UTF-8 (control bytes included).
pub fn key_to_terminal_bytes(ev: &KeyEvent) -> Vec<u8> {
    if let Some(b) = keysym_terminal_bytes(ev.keysym) {
        return b;
    }
    ev.utf8.bytes().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVDEV_OFFSET: u32 = 8;

    #[test]
    fn translates_us_layout() {
        let k = Keyboard::from_names("us").unwrap();
        let ev = k.translate(30 + EVDEV_OFFSET); // evdev KEY_A
        assert_eq!(ev.utf8, "a");
        let ev = k.translate(36 + EVDEV_OFFSET); // evdev KEY_J
        assert_eq!(ev.utf8, "j");
    }

    #[test]
    fn ctrl_yields_control_bytes() {
        let mut k = Keyboard::from_names("us").unwrap();
        k.update_mask(k.ctrl_mask(), 0, 0, 0);
        assert!(k.ctrl_held());
        let ev = k.translate(30 + EVDEV_OFFSET); // Ctrl+A
        assert_eq!(ev.utf8.as_bytes(), &[0x01]);
    }

    #[test]
    fn special_keys_ansi() {
        let k = Keyboard::from_names("us").unwrap();
        let enter = k.translate(28 + EVDEV_OFFSET);
        assert_eq!(key_to_terminal_bytes(&enter), b"\r".to_vec());
        let up = k.translate(103 + EVDEV_OFFSET);
        assert_eq!(key_to_terminal_bytes(&up), b"\x1b[A".to_vec());
        let del = k.translate(111 + EVDEV_OFFSET);
        assert_eq!(key_to_terminal_bytes(&del), b"\x1b[3~".to_vec());
    }

    #[test]
    fn printable_passthrough() {
        let k = Keyboard::from_names("us").unwrap();
        let a = k.translate(30 + EVDEV_OFFSET);
        assert_eq!(key_to_terminal_bytes(&a), b"a".to_vec());
    }
}
