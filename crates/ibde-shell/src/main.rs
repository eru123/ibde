//! ibde-shell — the IBDE shell process (ADR-0005: one process, modules).
//!
//! v0 module: the menubar (layer-shell top bar). Draws the light theme
//! per docs/UX-SPEC.md: translucent bar, graphite accent pill with the
//! IBDE menu, clock at far right. Redraws on a 1 Hz tick — an idle bar
//! repaints nothing (ADR-0002: zero-pixel idle repaint).
//!
//! Layout (left→right): [IBDE] menu pill · module area · ... clock

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ibde_ui::canvas::{self, Canvas};
use ibde_ui::session::{Connection, LayerSpec, QueueHandle, Session, SessionHost, WlSurface};
use ibde_ui::shm::ShmBuffer;
use ibde_ui::stats::{rss_kb, FpsCounter};

const BAR_H: i32 = 28;

/// Light-theme palette (UX-SPEC §1: Light default, graphite accent).
mod theme {
    pub const BAR_BG: (f64, f64, f64, f64) = (0.961, 0.961, 0.973, 0.97);
    pub const HAIRLINE: (f64, f64, f64, f64) = (0.82, 0.82, 0.84, 1.0);
    pub const ACCENT: (f64, f64, f64, f64) = (0.29, 0.31, 0.35, 1.0);
    pub const TEXT: (f64, f64, f64, f64) = (0.11, 0.11, 0.13, 1.0);
    pub const TEXT_MUTED: (f64, f64, f64, f64) = (0.45, 0.45, 0.49, 1.0);
    pub const ON_ACCENT: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0);
}

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    surface: Option<WlSurface>,
    buf: Option<ShmBuffer>,
    canvas: Option<Canvas>,
    configured: bool,
    frames: u32,
    fps: FpsCounter,
    t0: Option<Instant>,
}

impl App {
    fn new() -> Self {
        Self {
            session: None,
            conn: None,
            qh: None,
            surface: None,
            buf: None,
            canvas: None,
            configured: false,
            frames: 0,
            fps: FpsCounter::new(),
            t0: None,
        }
    }

    fn draw_bar(&mut self) {
        let Some(c) = self.canvas.as_mut() else {
            return;
        };
        {
            let cr = c.context().unwrap();
            draw_bar_content(&cr, c.width, c.height);
        } // cairo Context must drop before surface data access
        if let Some(b) = self.buf.as_mut() {
            c.copy_to(b);
        }
    }

    fn submit(&mut self) {
        if let (Some(b), Some(sf)) = (&self.buf, &self.surface) {
            sf.attach(Some(b.wayland_buffer()), 0, 0);
            sf.damage_buffer(0, 0, b.width, b.height);
            sf.commit();
        }
    }
}

impl SessionHost for App {
    fn session_slot(&mut self) -> &mut Option<Session> {
        &mut self.session
    }

    fn on_layer_configure(&mut self, _id: u64, w: i32, h: i32) {
        let w = if w > 0 { w } else { 1280 };
        let h = if h > 0 { h } else { BAR_H };
        let shm = self
            .session
            .as_ref()
            .and_then(|s| s.shm.clone())
            .expect("configure without shm");
        let qh = self.qh.clone().expect("configure without qh");
        self.buf = Some(ShmBuffer::new(&shm, w, h, &qh));
        self.canvas = Some(Canvas::new(w, h).expect("canvas"));
        self.t0.get_or_insert_with(Instant::now);
        self.draw_bar();
        self.submit();
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
        self.configured = true;
    }

    fn on_layer_closed(&mut self, _id: u64) {
        eprintln!("[shell] layer closed");
    }
}

ibde_ui::delegate_ibde_session!(App);

/// Draw the menubar contents (pure: canvas metrics in, pixels out).
fn draw_bar_content(cr: &cairo::Context, width: i32, height: i32) {
    let w = width as f64;
    let h = height as f64;
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    // bar background + bottom hairline
    let (r, g, b, a) = theme::BAR_BG;
    cr.set_source_rgba(r, g, b, a);
    cr.rectangle(0.0, 0.0, w, h);
    let _ = cr.fill();
    let (r, g, b, a) = theme::HAIRLINE;
    cr.set_source_rgba(r, g, b, a);
    cr.rectangle(0.0, h - 1.0, w, 1.0);
    let _ = cr.fill();

    let font = pango::FontDescription::from_string("Sans 11");
    let font_semibold = {
        let mut f = pango::FontDescription::from_string("Sans 11");
        f.set_weight(pango::Weight::Semibold);
        f
    };

    // IBDE menu pill (left)
    let pill_h = 18.0;
    let pill_y = (h - pill_h) / 2.0;
    canvas::fill_pill(cr, 8.0, pill_y, 44.0, pill_h, theme::ACCENT);
    let (pw, th) = canvas::label_size(cr, "IBDE", &font_semibold);
    canvas::draw_label(
        cr,
        "IBDE",
        &font_semibold,
        8.0 + (44.0 - pw as f64) / 2.0,
        pill_y + (pill_h - th as f64) / 2.0,
        theme::ON_ACCENT,
    );

    // module area placeholder (app name when global menu lands)
    let (_mw, th) = canvas::label_size(cr, "Finder", &font);
    canvas::draw_label(
        cr,
        "Finder",
        &font,
        64.0,
        (h - th as f64) / 2.0,
        theme::TEXT,
    );

    // clock (far right)
    let clock = clock_hm();
    let (cw, th) = canvas::label_size(cr, &clock, &font);
    canvas::draw_label(
        cr,
        &clock,
        &font,
        w - cw as f64 - 12.0,
        (h - th as f64) / 2.0,
        theme::TEXT,
    );

    // muted module hint before the clock
    let hint = "shell v0";
    let (hw, hth) = canvas::label_size(cr, hint, &font);
    canvas::draw_label(
        cr,
        hint,
        &font,
        w - cw as f64 - 24.0 - hw as f64,
        (h - hth as f64) / 2.0,
        theme::TEXT_MUTED,
    );
}

/// Local HH:MM via libc localtime_r (no chrono dependency; same tzdata
/// as every other process on the machine).
fn clock_hm() -> String {
    // SAFETY: localtime_r writes into a stack `tm` from a time_t we own;
    // it is the canonical thread-safe libc API for this.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return "--:--".to_string();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

fn main() {
    let mut seconds = 6.0f64;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--seconds" {
            seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(6.0);
        }
    }

    let conn = Connection::connect_to_env().expect("no wayland compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App::new();
    app.conn = Some(conn.clone());
    app.qh = Some(qh.clone());

    Session::begin(&mut app, &conn, &qh);
    queue.roundtrip(&mut app).expect("roundtrip failed");
    assert!(
        app.session.as_ref().is_some_and(|s| s.ready()),
        "missing globals (need a wlroots compositor with layer-shell)"
    );

    let spec = LayerSpec::top_bar("ibde-bar", BAR_H);
    let (_bar_id, surface, _ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&spec, &qh)
        .expect("layer shell unavailable");
    app.surface = Some(surface);
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no bar configure");
    assert!(app.configured, "bar never configured");

    // 1 Hz tick: clock redraws once per second, nothing else repaints
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if tx.send(()).is_err() {
            break;
        }
    });

    let t0 = Instant::now();
    loop {
        if t0.elapsed().as_secs_f64() >= seconds {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                app.frames += 1;
                app.draw_bar();
                app.submit();
                if let Some(conn) = &app.conn {
                    let _ = conn.flush();
                }
                if let Some((fps, elapsed)) = app.fps.tick() {
                    println!(
                        "STATS redraws_per_s={fps} rss_kb={} t={elapsed:.1}",
                        rss_kb()
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Err(e) = queue.dispatch_pending(&mut app) {
            eprintln!("dispatch error: {e}");
            break;
        }
        if let Some(conn) = &app.conn {
            let _ = conn.flush();
        }
    }

    println!("SUMMARY seconds={seconds} rss_kb={}", rss_kb());
}
