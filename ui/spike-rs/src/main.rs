//! IBDE toolkit spike — Rust edition (ADR-0012). Mirrors `ui/spike/spike.c`:
//! one wlr-layer-shell top bar, wl_shm ARGB buffer, cairo + pango, animated
//! rounded pill + text, frame-callback driven. Reports FPS + RSS.
//!
//! Modes:
//!   ibde-spike                 — layer-shell bar, 10 s
//!   ibde-spike --seconds N
//!   ibde-spike --offscreen N   — render N frames, no compositor

use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::unix::io::{AsFd, AsRawFd, FromRawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use memmap2::MmapMut;
use pango::FontDescription;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_shm,
    wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use cairo::{Context as Cr, Format, ImageSurface};

const BAR_H: i32 = 32;

static RUNNING: AtomicBool = AtomicBool::new(true);

fn rss_kb() -> i64 {
    let mut s = String::new();
    if File::open("/proc/self/statm")
        .and_then(|mut f| f.read_to_string(&mut s))
        .is_err()
    {
        return -1;
    }
    s.split_whitespace()
        .nth(1)
        .and_then(|r| r.parse::<i64>().ok())
        .map(|r| r * 4)
        .unwrap_or(-1)
}

/// One wl_shm buffer + our mapping of it.
struct ShmBuf {
    _fd: File,
    mmap: MmapMut,
    pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    stride: i32,
}

impl ShmBuf {
    fn new(
        shm: &wl_shm::WlShm,
        w: i32,
        h: i32,
        qh: &QueueHandle<App>,
    ) -> Self {
        let stride = w * 4;
        let size = (stride * h) as usize;
        let name = CString::new("ibde-spike").unwrap();
        // SAFETY: plain libc memfd_create, checked immediately.
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0, "memfd_create failed");
        // SAFETY: fd is owned from here on; File closes it on drop.
        let file = unsafe { File::from_raw_fd(fd) };
        let rc = unsafe { libc::ftruncate(fd, size as libc::off_t) };
        assert!(rc == 0, "ftruncate failed");
        // SAFETY: fd is a valid memfd of exactly `size` bytes, freshly
        // created and not shared with any other mapper.
        let mmap = unsafe { MmapMut::map_mut(file.as_raw_fd()) }.expect("mmap failed");
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            w,
            h,
            stride,
            wl_shm::Format::Argb8888,
            qh,
            (),
        );
        Self { _fd: file, mmap, pool, buffer, width: w, height: h, stride }
    }
}

struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<ZwlrLayerShellV1>,
    surface: Option<wl_surface::WlSurface>,
    buf: Option<ShmBuf>,
    cairo: Option<ImageSurface>, // persistent scratch surface (own data)
    font: FontDescription,
    cfg_w: i32,
    cfg_h: i32,
    configured: bool,
    frames: u32,
    t0: Option<Instant>,
    last_report: Instant,
    seconds: f64,
}

impl App {
    fn new(seconds: f64) -> Self {
        Self {
            compositor: None,
            shm: None,
            shell: None,
            surface: None,
            buf: None,
            cairo: None,
            font: FontDescription::from_string("Sans 12"),
            cfg_w: 0,
            cfg_h: 0,
            configured: false,
            frames: 0,
            t0: None,
            last_report: Instant::now(),
            seconds,
        }
    }
}

/// Draw one frame onto the scratch surface.
fn draw(cs: &ImageSurface, w: i32, h: i32, t: f64, frames: u32, font: &FontDescription) {
    let cr = Cr::new(cs).unwrap();
    cr.set_operator(cairo::Operator::Clear);
    cr.paint().unwrap();
    cr.set_operator(cairo::Operator::Over);

    // bar background
    cr.set_source_rgba(0.92, 0.92, 0.94, 0.88);
    cr.rectangle(0.0, 0.0, w as f64, h as f64);
    cr.fill().unwrap();

    // animated accent pill
    let pulse = 0.80 + 0.20 * (t * 2.0).sin();
    let pw = 180.0 + 40.0 * (t * 1.3).sin();
    let r = h as f64 / 2.0;
    cr.set_source_rgba(0.35, 0.38, 0.45, pulse);
    cr.arc(16.0 + r, r, r, std::f64::consts::FRAC_PI_2, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.arc(16.0 + pw - r, r, r, -std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
    cr.close_path();
    cr.fill().unwrap();

    // text
    let layout = pangocairo::functions::create_layout(&cr);
    layout.set_font_description(Some(font));
    layout.set_text(&format!("IBDE spike (rust) {w}x{h} frame {frames}"));
    let (_tw, th) = layout.pixel_size();
    cr.set_source_rgba(0.1, 0.1, 0.12, 1.0);
    cr.move_to(220.0, (h as f64 - th as f64) / 2.0);
    pangocairo::functions::show_layout(&cr, &layout);
}

/// Render + copy scratch surface into the shm buffer.
fn render_and_copy(app: &mut App, t: f64) {
    let font = app.font.clone();
    if let (Some(buf), Some(cs)) = (&mut app.buf, &mut app.cairo) {
        draw(cs, buf.width, buf.height, t, app.frames, &font);
        let data = cs.data().unwrap();
        buf.mmap[..data.len()].copy_from_slice(&data);
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for App {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, version } = event {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "zwlr_layer_shell_v1" => {
                    state.shell = Some(registry.bind(name, version.min(4), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, ()> for App {
    fn event(
        state: &mut Self,
        ls: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                ls.ack_configure(serial);
                state.cfg_w = width as i32;
                state.cfg_h = height as i32;
                state.configured = true;
            }
            zwlr_layer_surface_v1::Event::Closed => {
                RUNNING.store(false, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &wl_callback::WlCallback,
        _event: wl_callback::Event,
        _data: &(),
        conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let t0 = match state.t0 {
            Some(t) => t,
            None => return,
        };
        let t = t0.elapsed().as_secs_f64();
        if t >= state.seconds || !RUNNING.load(Ordering::Relaxed) {
            return;
        }
        state.frames += 1;

        let surface = state.surface.clone().unwrap();
        render_and_copy(state, t);

        if let Some(buf) = &state.buf {
            surface.attach(Some(&buf.buffer), 0, 0);
            surface.damage_buffer(0, 0, buf.width, buf.height);
        }
        surface.commit();

        if state.last_report.elapsed().as_millis() >= 1000 {
            println!("STATS fps={} rss_kb={} t={:.1}", state.frames, rss_kb(), t);
            state.frames = 0;
            state.last_report = Instant::now();
        }

        let _cb = surface.frame(qh, ());
        surface.commit();
        let _ = conn.flush();
    }
}

wayland_client::delegate_noop!(App: wl_compositor::WlCompositor);
wayland_client::delegate_noop!(App: ignore wl_shm::WlShm); // emits format events
wayland_client::delegate_noop!(App: wl_shm_pool::WlShmPool);
wayland_client::delegate_noop!(App: ignore wl_buffer::WlBuffer); // release
wayland_client::delegate_noop!(App: ignore wl_surface::WlSurface); // enter/leave
wayland_client::delegate_noop!(App: ignore wl_output::WlOutput);
wayland_client::delegate_noop!(App: ZwlrLayerShellV1);

fn run_offscreen(frames: u32) {
    let (w, h) = (1280, BAR_H);
    let cs = ImageSurface::create(Format::ARgb32, w, h).unwrap();
    let font = FontDescription::from_string("Sans 12");
    let t0 = Instant::now();
    for i in 0..frames {
        draw(&cs, w, h, i as f64 / 60.0, i, &font);
    }
    let dt = t0.elapsed().as_millis();
    println!(
        "OFFSCREEN frames={} total_ms={} avg_ms={:.3} rss_kb={}",
        frames,
        dt,
        dt as f64 / frames as f64,
        rss_kb()
    );
}

fn main() {
    let mut seconds = 10.0f64;
    let mut offscreen = 0u32;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seconds" => {
                if let Some(v) = args.next() {
                    seconds = v.parse().unwrap_or(10.0);
                } else {
                    eprintln!("--seconds needs a value");
                    std::process::exit(1);
                }
            }
            "--offscreen" => {
                if let Some(v) = args.next() {
                    offscreen = v.parse().unwrap_or(0);
                } else {
                    eprintln!("--offscreen needs a value");
                    std::process::exit(1);
                }
            }
            other => {
                eprintln!("unknown arg {other}: usage: ibde-spike [--seconds N] | [--offscreen N]");
                std::process::exit(1);
            }
        }
    }

    if offscreen > 0 {
        run_offscreen(offscreen);
        return;
    }

    let conn = Connection::connect_to_env()
        .expect("cannot connect to a Wayland compositor (WAYLAND_DISPLAY set?)");
    eprintln!("[spike] connected");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let display = conn.display();
    let _registry = display.get_registry(&qh, ());

    let mut app = App::new(seconds);
    queue.roundtrip(&mut app).expect("roundtrip failed");
    eprintln!("[spike] globals: compositor={} shm={} shell={}",
        app.compositor.is_some(), app.shm.is_some(), app.shell.is_some());

    let compositor = app.compositor.clone().expect("no wl_compositor");
    let shm = app.shm.clone().expect("no wl_shm");
    let shell = app
        .shell
        .clone()
        .expect("compositor lacks wlr-layer-shell (need wlroots-based: labwc/sway)");

    let surface = compositor.create_surface(&qh, ());
    let ls = shell.get_layer_surface(
        &surface,
        None::<&wl_output::WlOutput>,
        Layer::Top,
        "ibde-spike".to_string(),
        &qh,
        (),
    );
    ls.set_anchor(Anchor::Top | Anchor::Left | Anchor::Right);
    ls.set_size(0, BAR_H as u32);
    ls.set_exclusive_zone(BAR_H);
    surface.commit();
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no configure arrived");

    assert!(app.configured, "layer surface never configured");
    eprintln!("[spike] configured {}x{}", app.cfg_w, app.cfg_h);
    let w = if app.cfg_w > 0 { app.cfg_w } else { 1280 };
    let h = if app.cfg_h > 0 { app.cfg_h } else { BAR_H };
    app.buf = Some(ShmBuf::new(&shm, w, h, &qh));
    app.cairo = Some(ImageSurface::create(Format::ARgb32, w, h).unwrap());

    // initial frame: commit with damage so headless compositors start
    // delivering frame callbacks
    app.t0 = Some(Instant::now());
    app.last_report = Instant::now();
    render_and_copy(&mut app, 0.0);
    if let Some(buf) = &app.buf {
        surface.attach(Some(&buf.buffer), 0, 0);
        surface.damage_buffer(0, 0, buf.width, buf.height);
    }
    surface.commit();
    let _ = conn.flush();
    let _cb = surface.frame(&qh, ());
    surface.commit();
    let _ = conn.flush();

    eprintln!("[spike] initial frame committed, entering dispatch");
    app.surface = Some(surface);
    app.shell = Some(shell);
    let _ = ls;

    let secs = seconds;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs));
        RUNNING.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
        println!("SUMMARY avg_over={:.1}s rss_kb={}", secs, rss_kb());
        std::process::exit(0);
    });

    // blocking_dispatch is single-shot (pending → one read → return);
    // keep the loop alive until the timer thread prints SUMMARY and exits.
    loop {
        if let Err(e) = queue.blocking_dispatch(&mut app) {
            eprintln!("dispatch error: {e}");
            break;
        }
    }
}
