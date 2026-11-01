//! ptr-inject — drive the pointer via zwlr_virtual_pointer_manager_v1.
//! Companion to kb-inject for the interactive bench workout: hovers and
//! clicks the dock.
//!
//! Usage: ptr-inject --x 640 --y 770 [--click] [--extents 1280x800] [--after-ms 400]

use wayland_client::protocol::wl_pointer::ButtonState as ZwlrVirtualPointerButtonState;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct App {
    manager: Option<ZwlrVirtualPointerManagerV1>,
    seat: Option<wl_seat::WlSeat>,
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
                "zwlr_virtual_pointer_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "wl_seat" => state.seat = Some(registry.bind(name, version.min(4), qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerManagerV1,
        _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrVirtualPointerV1, ()> for App {
    fn event(
        _: &mut Self,
        _: &ZwlrVirtualPointerV1,
        _: wayland_protocols_wlr::virtual_pointer::v1::client::zwlr_virtual_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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

fn main() {
    let mut x = 0u32;
    let mut y = 0u32;
    let mut click = false;
    let mut ext_w = 1280u32;
    let mut ext_h = 800u32;
    let mut after_ms = 400u64;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("missing flag value");
        match a.as_str() {
            "--x" => x = val().parse().expect("bad x"),
            "--y" => y = val().parse().expect("bad y"),
            "--click" => click = true,
            "--extents" => {
                let v = val();
                let (w, h) = v.split_once('x').expect("extents WxH");
                ext_w = w.parse().expect("bad w");
                ext_h = h.parse().expect("bad h");
            }
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
    };
    let _reg = conn.display().get_registry(&qh, ());
    queue.roundtrip(&mut app).expect("roundtrip");
    let manager = app
        .manager
        .take()
        .expect("no zwlr_virtual_pointer_manager_v1");
    let seat = app.seat.take().expect("no wl_seat");

    let vp = manager.create_virtual_pointer(Some(&seat), &qh, ());
    let _ = conn.flush();
    std::thread::sleep(std::time::Duration::from_millis(after_ms));

    let t = 0u32;
    vp.motion_absolute(t, x, y, ext_w, ext_h);
    vp.frame();
    let _ = conn.flush();
    std::thread::sleep(std::time::Duration::from_millis(250));

    if click {
        const BTN_LEFT: u32 = 0x110;
        vp.button(t, BTN_LEFT, ZwlrVirtualPointerButtonState::Pressed);
        vp.frame();
        let _ = conn.flush();
        std::thread::sleep(std::time::Duration::from_millis(120));
        vp.button(t, BTN_LEFT, ZwlrVirtualPointerButtonState::Released);
        vp.frame();
        let _ = conn.flush();
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    eprintln!("[ptr-inject] moved to {x},{y} click={click}");
}
