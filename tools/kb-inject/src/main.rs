//! kb-inject — type text into a Wayland compositor via
//! zwp_virtual_keyboard_manager_v1. Drives the interactive part of the
//! ibde-bench floor workout (ADR-0002): injected keys → compositor →
//! focused client's wl_keyboard → (terminal) PTY → echo → grid.
//!
//! Usage: kb-inject --text "hello\r" [--delay-ms 60] [--after-ms 500]

use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};

use vkp::{ZwpVirtualKeyboardManagerV1, ZwpVirtualKeyboardV1};

mod vkp {
    #![allow(non_camel_case_types, non_upper_case_globals, dead_code, clippy::all)]
    use wayland_client;
    use wayland_client::protocol::wl_seat;

    pub mod __interfaces {
        use wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocol/virtual-keyboard-unstable-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocol/virtual-keyboard-unstable-v1.xml");
    pub use zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1;
    pub use zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1;
}

struct App {
    manager: Option<ZwpVirtualKeyboardManagerV1>,
    seat: Option<wl_seat::WlSeat>,
    #[allow(dead_code)] // proxy kept alive in main, not via this slot
    vk: Option<ZwpVirtualKeyboardV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwp_virtual_keyboard_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "wl_seat" => state.seat = Some(registry.bind(name, version.min(4), qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _: vkp::zwp_virtual_keyboard_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _: vkp::zwp_virtual_keyboard_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// (keycode, needs_shift) producing `ch` under a "us" keymap.
fn find_key(ch: char) -> Option<(u32, bool)> {
    use xkbcommon::xkb;
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let keymap =
        xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)?;
    let mut state = xkb::State::new(&keymap);
    let shift = 1u32 << keymap.mod_get_index("Shift");
    let target = ch.to_string();
    for (mask, needs_shift) in [(0u32, false), (shift, true)] {
        state.update_mask(mask, 0, 0, 0, 0, 0);
        for kc in 9u32..256 {
            let k = xkb::Keycode::from(kc);
            if state.key_get_utf8(k) == target {
                return Some((kc, needs_shift));
            }
        }
    }
    None
}

fn main() {
    #[derive(Clone)]
    enum Action {
        Text(String),
        Special(String),
    }
    let mut actions: Vec<Action> = Vec::new();
    let mut delay_ms = 60u64;
    let mut after_ms = 500u64;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("missing flag value");
        match a.as_str() {
            "--text" => actions.push(Action::Text(val())),
            "--special" => actions.push(Action::Special(val())),
            "--delay-ms" => delay_ms = val().parse().expect("bad delay"),
            "--after-ms" => after_ms = val().parse().expect("bad after"),
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(1);
            }
        }
    }

    let conn = Connection::connect_to_env().expect("no compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App {
        manager: None,
        seat: None,
        vk: None,
    };
    let _reg = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut app).expect("roundtrip");
    let manager = app.manager.take().expect("no virtual keyboard manager");
    let seat = app.seat.take().expect("no wl_seat");

    // virtual keyboard + a real "us" keymap (compositor-side translation)
    let vk = manager.create_virtual_keyboard(&seat, &qh, ());
    {
        use xkbcommon::xkb;
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap =
            xkb::Keymap::new_from_names(&ctx, "", "", "us", "", None, xkb::KEYMAP_COMPILE_NO_FLAGS)
                .expect("keymap");
        let kmstr = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);
        let name = std::ffi::CString::new("kb-inject").unwrap();
        // SAFETY: fresh memfd; we own it until handed to the protocol
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0, "memfd");
        let mut bytes = kmstr.into_bytes();
        bytes.push(0); // keymap must be NUL-terminated
        let mut off = 0usize;
        while off < bytes.len() {
            // SAFETY: writing our own buffer to a fresh memfd
            let n = unsafe { libc::write(fd, bytes[off..].as_ptr().cast(), bytes.len() - off) };
            assert!(n > 0, "memfd write");
            off += n as usize;
        }
        use std::os::fd::BorrowedFd;
        // SAFETY: fd is valid and outlives this call
        let bfd = unsafe { BorrowedFd::borrow_raw(fd) };
        vk.keymap(1, bfd, bytes.len() as u32);
        // SAFETY: compositor dups the fd; we close ours
        unsafe { libc::close(fd) };
    }
    let _ = conn.flush();
    std::thread::sleep(std::time::Duration::from_millis(after_ms));

    let t0 = std::time::Instant::now();
    let t = || t0.elapsed().as_millis() as u32;
    for action in &actions {
        match action {
            Action::Text(text) => {
                for ch in text.chars() {
                    let Some((kc, shift)) = find_key(ch) else {
                        eprintln!("no key for {ch:?}, skipping");
                        continue;
                    };
                    if shift {
                        vk.modifiers(1, 0, 0, 0);
                        let _ = conn.flush();
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    // find_key returns xkb keycodes (evdev + 8); the
                    // protocol wants evdev codes
                    let evdev = kc - 8;
                    vk.key(t(), evdev, 1);
                    let _ = conn.flush();
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    vk.key(t(), evdev, 0);
                    let _ = conn.flush();
                    if shift {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        vk.modifiers(0, 0, 0, 0);
                        let _ = conn.flush();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
            Action::Special(combo) => {
                use xkbcommon::xkb;
                let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
                let keymap = xkb::Keymap::new_from_names(
                    &ctx,
                    "",
                    "",
                    "us",
                    "",
                    None,
                    xkb::KEYMAP_COMPILE_NO_FLAGS,
                )
                .expect("keymap");
                let ctrl = 1u32 << keymap.mod_get_index("Control");
                let shift = 1u32 << keymap.mod_get_index("Shift");
                let mut mods = 0u32;
                let key: u32 = match combo.as_str() {
                    "ctrl+space" => {
                        mods = ctrl;
                        57
                    }
                    "shift+pgup" => {
                        mods = shift;
                        104
                    }
                    "shift+pgdn" => {
                        mods = shift;
                        109
                    }
                    "ctrl+shift+f" => {
                        mods = ctrl | shift;
                        33
                    }
                    "esc" => 1,
                    "enter" => 28,
                    "up" => 103,
                    "down" => 108,
                    "vol-up" => 115,
                    "vol-down" => 114,
                    "mute" => 113,
                    "bright-up" => 233,
                    "bright-down" => 232,
                    other => {
                        eprintln!("unknown special {other}");
                        std::process::exit(1);
                    }
                };
                if mods != 0 {
                    vk.modifiers(mods, 0, 0, 0);
                    let _ = conn.flush();
                    std::thread::sleep(std::time::Duration::from_millis(15));
                }
                vk.key(t(), key, 1);
                let _ = conn.flush();
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                vk.key(t(), key, 0);
                let _ = conn.flush();
                if mods != 0 {
                    std::thread::sleep(std::time::Duration::from_millis(15));
                    vk.modifiers(0, 0, 0, 0);
                    let _ = conn.flush();
                }
                eprintln!("[kb-inject] special {combo}");
            }
        }
    }
    eprintln!("[kb-inject] {} actions done", actions.len());
}
