//! IBDE toolkit spike — Rust edition (ADR-0012). Mirrors `ui/spike/spike.c`
//! and dogfoods the first real `ibde-ui` crate modules (shm, canvas, stats).
//!
//! Modes:
//!   ibde-spike                 — layer-shell bar, 10 s
//!   ibde-spike --seconds N
//!   ibde-spike --offscreen N   — render N frames, no compositor

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ibde_ui::canvas::{self, Canvas};
use ibde_ui::shm::ShmBuffer;
use ibde_ui::stats::{rss_kb, FpsCounter};
use pango::FontDescription;
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_compositor, wl_output, wl_registry, wl_shm, wl_shm_pool, wl_surface,
};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

const BAR_H: i32 = 32;

static RUNNING: AtomicBool = AtomicBool::new(true);

struct App {
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    shell: Option<ZwlrLayerShellV1>,
    surface: Option<wl_surface::WlSurface>,
    buf: Option<ShmBuffer>,
    canvas: Option<Canvas>,
    font: FontDescription,
    cfg_w: i32,
    cfg_h: i32,
    configured: bool,
    frames: u32,
    fps: FpsCounter,
    t0: Option<Instant>,
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
            canvas: None,
            font: FontDescription::from_string("Sans 12"),
            cfg_w: 0,
            cfg_h: 0,
            configured: false,
            frames: 0,
            fps: FpsCounter::new(),
            t0: None,
            seconds,
        }
    }
}

/// Draw one frame onto the canvas.
fn draw(c: &Canvas, t: f64, frames: u32, font: &FontDescription) {
    let cr = c.context().unwrap();
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    // bar background
    cr.set_source_rgba(0.92, 0.92, 0.94, 0.88);
    cr.rectangle(0.0, 0.0, c.width as f64, c.height as f64);
    let _ = cr.fill();

    // animated accent pill (ibde-ui helper — dogfooding)
    let pulse = 0.80 + 0.20 * (t * 2.0).sin();
    let pw = 180.0 + 40.0 * (t * 1.3).sin();
    canvas::fill_pill(
        &cr,
        16.0,
        0.0,
        pw,
        c.height as f64,
        (0.35, 0.38, 0.45, pulse),
    );

    // text (ibde-ui helper)
    let text = format!(
        "IBDE spike (rust) {}x{} frame {}",
        c.width, c.height, frames
    );
    let (_tw, th) = canvas::label_size(&cr, &text, font);
    canvas::draw_label(
        &cr,
        &text,
        font,
        220.0,
        (c.height as f64 - th as f64) / 2.0,
        (0.1, 0.1, 0.12, 1.0),
    );
}

/// Render + copy canvas into the shm buffer.
fn render_and_copy(app: &mut App, t: f64) {
    let font = app.font.clone();
    if let (Some(buf), Some(c)) = (&mut app.buf, &mut app.canvas) {
        draw(c, t, app.frames, &font);
        c.copy_to(buf);
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
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
                }
                "wl_shm" => state.shm = Some(registry.bind(name, version.min(1), qh, ())),
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
            zwlr_layer_surface_v1::Event::Configure {
                serial,
                width,
                height,
            } => {
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
            surface.attach(Some(buf.wayland_buffer()), 0, 0);
            surface.damage_buffer(0, 0, buf.width, buf.height);
        }
        surface.commit();

        if let Some((fps, elapsed)) = state.fps.tick() {
            println!("STATS fps={fps} rss_kb={} t={elapsed:.1}", rss_kb());
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
    let c = Canvas::new(1280, BAR_H).unwrap();
    let font = FontDescription::from_string("Sans 12");
    let t0 = Instant::now();
    for i in 0..frames {
        draw(&c, i as f64 / 60.0, i, &font);
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
            "--seconds" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => seconds = v,
                None => {
                    eprintln!("--seconds needs a number");
                    std::process::exit(1);
                }
            },
            "--offscreen" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => offscreen = v,
                None => {
                    eprintln!("--offscreen needs a number");
                    std::process::exit(1);
                }
            },
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
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();
    let display = conn.display();
    let _registry = display.get_registry(&qh, ());

    let mut app = App::new(seconds);
    queue.roundtrip(&mut app).expect("roundtrip failed");
    eprintln!(
        "[spike] globals: compositor={} shm={} shell={}",
        app.compositor.is_some(),
        app.shm.is_some(),
        app.shell.is_some()
    );

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

    let w = if app.cfg_w > 0 { app.cfg_w } else { 1280 };
    let h = if app.cfg_h > 0 { app.cfg_h } else { BAR_H };
    app.buf = Some(ShmBuffer::new(&shm, w, h, &qh));
    app.canvas = Some(Canvas::new(w, h).unwrap());

    // initial frame: commit with damage so headless compositors start
    // delivering frame callbacks
    app.t0 = Some(Instant::now());
    render_and_copy(&mut app, 0.0);
    if let Some(buf) = &app.buf {
        surface.attach(Some(buf.wayland_buffer()), 0, 0);
        surface.damage_buffer(0, 0, buf.width, buf.height);
    }
    surface.commit();
    let _ = conn.flush();
    let _cb = surface.frame(&qh, ());
    surface.commit();
    let _ = conn.flush();

    app.surface = Some(surface);
    let _ = ls;

    let secs = seconds;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs));
        RUNNING.store(false, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
        println!("SUMMARY avg_over={secs:.1}s rss_kb={}", rss_kb());
        std::process::exit(0);
    });

    // blocking_dispatch is single-shot (pending -> one read -> return);
    // keep the loop alive until the timer thread prints SUMMARY and exits.
    loop {
        if let Err(e) = queue.blocking_dispatch(&mut app) {
            eprintln!("dispatch error: {e}");
            break;
        }
    }
}
