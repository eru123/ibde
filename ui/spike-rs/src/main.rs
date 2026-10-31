//! IBDE toolkit spike — Rust edition, now built entirely on the
//! `ibde-ui` session glue (`delegate_ibde_session!` + `SessionHost`).
//! No direct wayland-client usage — this binary is the glue's
//! acceptance test.
//!
//! Modes:
//!   ibde-spike                 — layer-shell bar, 10 s
//!   ibde-spike --seconds N
//!   ibde-spike --offscreen N   — render N frames, no compositor

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ibde_ui::canvas::{self, Canvas};
use ibde_ui::session::{Connection, LayerSpec, QueueHandle, Session, SessionHost, WlSurface};
use ibde_ui::shm::ShmBuffer;
use ibde_ui::stats::{rss_kb, FpsCounter};
use pango::FontDescription;

const BAR_H: i32 = 32;

static RUNNING: AtomicBool = AtomicBool::new(true);

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    surface: Option<WlSurface>,
    surface_id: u64,
    buf: Option<ShmBuffer>,
    canvas: Option<Canvas>,
    font: FontDescription,
    frames: u32,
    fps: FpsCounter,
    t0: Option<Instant>,
    seconds: f64,
}

impl App {
    fn new(seconds: f64) -> Self {
        Self {
            session: None,
            conn: None,
            qh: None,
            surface: None,
            surface_id: 0,
            buf: None,
            canvas: None,
            font: FontDescription::from_string("Sans 12"),
            frames: 0,
            fps: FpsCounter::new(),
            t0: None,
            seconds,
        }
    }

    fn render_at(&mut self, t: f64) {
        let font = self.font.clone();
        if let (Some(buf), Some(c)) = (&mut self.buf, &mut self.canvas) {
            draw(c, t, self.frames, &font);
            c.copy_to(buf);
        }
    }

    fn submit(&mut self) {
        if let (Some(buf), Some(sf)) = (&self.buf, &self.surface) {
            sf.attach(Some(buf.wayland_buffer()), 0, 0);
            sf.damage_buffer(0, 0, buf.width, buf.height);
            sf.commit();
        }
    }
}

impl SessionHost for App {
    fn session_slot(&mut self) -> &mut Option<Session> {
        &mut self.session
    }

    fn on_layer_configure(&mut self, id: u64, w: i32, h: i32) {
        let w = if w > 0 { w } else { 1280 };
        let h = if h > 0 { h } else { BAR_H };
        let shm = self
            .session
            .as_ref()
            .and_then(|s| s.shm.clone())
            .expect("on_layer_configure without shm");
        let qh = self.qh.clone().expect("on_layer_configure without qh");
        self.surface_id = id;
        self.buf = Some(ShmBuffer::new(&shm, w, h, &qh));
        self.canvas = Some(Canvas::new(w, h).expect("canvas"));
        self.t0.get_or_insert_with(Instant::now);

        // initial frame: commit with damage so headless compositors
        // start delivering frame callbacks
        self.render_at(0.0);
        self.submit();
        if let Some(sf) = &self.surface {
            ibde_ui::session::request_frame(sf, self.surface_id, &qh);
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
    }

    fn on_layer_closed(&mut self, _id: u64) {
        RUNNING.store(false, Ordering::Relaxed);
    }

    fn on_frame(&mut self, surface_id: u64) {
        if surface_id != self.surface_id {
            return;
        }
        let t = match self.t0 {
            Some(t) => t.elapsed().as_secs_f64(),
            None => return,
        };
        if t >= self.seconds || !RUNNING.load(Ordering::Relaxed) {
            return;
        }
        self.frames += 1;

        self.render_at(t);
        self.submit();

        if let Some((fps, elapsed)) = self.fps.tick() {
            println!("STATS fps={fps} rss_kb={} t={elapsed:.1}", rss_kb());
        }

        let qh = self.qh.clone().unwrap();
        if let Some(sf) = &self.surface {
            ibde_ui::session::request_frame(sf, self.surface_id, &qh);
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
    }
}

ibde_ui::delegate_ibde_session!(App);

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

    // animated accent pill (ibde-ui helper)
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
        "IBDE spike (session glue) {}x{} frame {}",
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

    let conn = Connection::connect_to_env().expect("cannot connect to a Wayland compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App::new(seconds);
    app.conn = Some(conn.clone());
    app.qh = Some(qh.clone());

    Session::begin(&mut app, &conn, &qh);
    queue.roundtrip(&mut app).expect("roundtrip failed");
    assert!(
        app.session.as_ref().is_some_and(|s| s.ready()),
        "missing globals (need a wlroots compositor with layer-shell)"
    );

    let spec = LayerSpec::top_bar("ibde-spike", BAR_H);
    let (surface, _ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&spec, &qh)
        .expect("layer shell unavailable");
    app.surface = Some(surface);
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no configure arrived");
    assert!(app.buf.is_some(), "configure never created buffers");

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
