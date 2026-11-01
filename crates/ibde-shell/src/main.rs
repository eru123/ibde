//! ibde-shell — the IBDE shell process (ADR-0005: one process, modules).
//!
//! Modules (v0):
//! - **menubar** — layer-shell top bar, 1 Hz clock redraw, zero paints
//!   between ticks (UX-SPEC §2, ADR-0002 idle discipline)
//! - **dock** — floating layer-shell panel centered on the bottom edge:
//!   translucent tray, default pins (Files / Terminal / Settings),
//!   separator, trash; draws once, repaints only on change
//!
//! Layout: [IBDE] pill · app area · ... clock  /  bottom: dock tray

use std::sync::mpsc;
use std::time::{Duration, Instant};

use ibde_ui::canvas::{self, Canvas};
use ibde_ui::session::{Connection, LayerSpec, QueueHandle, Session, SessionHost, WlSurface};
use ibde_ui::shm::ShmBuffer;
use ibde_ui::stats::{rss_kb, FpsCounter};

const BAR_H: i32 = 28;
const DOCK_H: i32 = 60;
/// bottom gap between dock tray and screen edge
const DOCK_EDGE_GAP: i32 = 10;

/// Light-theme palette (UX-SPEC §1: Light default, graphite accent).
mod theme {
    pub const BAR_BG: (f64, f64, f64, f64) = (0.961, 0.961, 0.973, 0.97);
    pub const HAIRLINE: (f64, f64, f64, f64) = (0.82, 0.82, 0.84, 1.0);
    pub const ACCENT: (f64, f64, f64, f64) = (0.29, 0.31, 0.35, 1.0);
    pub const TEXT: (f64, f64, f64, f64) = (0.11, 0.11, 0.13, 1.0);
    pub const TEXT_MUTED: (f64, f64, f64, f64) = (0.45, 0.45, 0.49, 1.0);
    pub const ON_ACCENT: (f64, f64, f64, f64) = (1.0, 1.0, 1.0, 1.0);
    pub const DOCK_BG: (f64, f64, f64, f64) = (0.961, 0.961, 0.973, 0.88);
    pub const DOCK_FILES: (f64, f64, f64, f64) = (0.20, 0.42, 0.80, 1.0);
    pub const DOCK_TERM: (f64, f64, f64, f64) = (0.13, 0.13, 0.15, 1.0);
    pub const DOCK_SETTINGS: (f64, f64, f64, f64) = (0.49, 0.51, 0.55, 1.0);
    pub const DOCK_TRASH: (f64, f64, f64, f64) = (0.78, 0.78, 0.80, 1.0);
}

/// Buffers for one module surface, created at configure time.
struct Kit {
    buf: ShmBuffer,
    canvas: Canvas,
    drawn: u32,
}

/// One shell module: identity + surface before configure, kit after.
struct Module {
    id: u64,
    surface: WlSurface,
    kit: Option<Kit>,
}

impl Module {
    fn configured(&self) -> bool {
        self.kit.is_some()
    }
}

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    bar: Option<Module>,
    dock: Option<Module>,
    frames: u32,
    fps: FpsCounter,
    t0: Option<Instant>,
    // pointer/dock state
    dock_hover: Option<usize>,
    over_dock: bool,
    terminal_cmd: String,
}

impl App {
    fn new() -> Self {
        Self {
            session: None,
            conn: None,
            qh: None,
            bar: None,
            dock: None,
            frames: 0,
            fps: FpsCounter::new(),
            t0: None,
            dock_hover: None,
            over_dock: false,
            terminal_cmd: std::env::var("IBDE_TERMINAL_CMD")
                .unwrap_or_else(|_| "ibde-terminal".into()),
        }
    }

    fn module_mut(&mut self, id: u64) -> Option<&mut Module> {
        if self.bar.as_ref().is_some_and(|m| m.id == id) {
            self.bar.as_mut()
        } else if self.dock.as_ref().is_some_and(|m| m.id == id) {
            self.dock.as_mut()
        } else {
            None
        }
    }

    /// Render + commit one module; `f` draws onto a fresh-cleared context.
    fn render_module<F: FnOnce(&cairo::Context, i32, i32)>(&mut self, id: u64, f: F) {
        let Some(m) = self.module_mut(id) else { return };
        let Some(k) = m.kit.as_mut() else { return };
        {
            let cr = k.canvas.context().unwrap();
            f(&cr, k.canvas.width, k.canvas.height);
        } // cairo Context must drop before surface data access
        k.canvas.copy_to(&mut k.buf);
        m.surface.attach(Some(k.buf.wayland_buffer()), 0, 0);
        m.surface.damage_buffer(0, 0, k.buf.width, k.buf.height);
        m.surface.commit();
        k.drawn += 1;
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
    }

    fn draw_bar(&mut self) {
        let id = self.bar.as_ref().map(|m| m.id).unwrap_or(0);
        self.render_module(id, draw_bar_content);
    }

    fn draw_dock(&mut self) {
        let id = self.dock.as_ref().map(|m| m.id).unwrap_or(0);
        let hovered = self.dock_hover;
        self.render_module(id, move |cr, w, h| draw_dock_content(cr, w, h, hovered));
    }

    /// Launch the app pinned at dock index 0..2 (trash is a no-op).
    fn launch(&self, index: usize) {
        let cmd = match index {
            0 => "thunar", // Files (present on real installs)
            1 => &self.terminal_cmd,
            2 => "ibde-settings",
            _ => return, // trash / separator
        };
        match std::process::Command::new(cmd)
            .env(
                "WAYLAND_DISPLAY",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            )
            .spawn()
        {
            Ok(child) => {
                eprintln!("[shell] launched {cmd} (pid {})", child.id());
                // children outlive the handle by design (detached)
                std::mem::forget(child);
            }
            Err(e) => eprintln!("[shell] launch {cmd} failed: {e}"),
        }
    }
}

impl SessionHost for App {
    fn session_slot(&mut self) -> &mut Option<Session> {
        &mut self.session
    }

    fn on_layer_configure(&mut self, id: u64, w: i32, h: i32) {
        let shm = self
            .session
            .as_ref()
            .and_then(|s| s.shm.clone())
            .expect("configure without shm");
        let qh = self.qh.clone().expect("configure without qh");
        let is_bar = self.bar.as_ref().is_some_and(|b| b.id == id);
        let is_known = is_bar || self.dock.as_ref().is_some_and(|d| d.id == id);
        if !is_known {
            eprintln!("[shell] configure for unknown surface {id}");
            return;
        }
        let w = if w > 0 { w } else { 1280 };
        let h = if h > 0 { h } else { BAR_H };
        let kit = Kit {
            buf: ShmBuffer::new(&shm, w, h, &qh),
            canvas: Canvas::new(w, h).expect("canvas"),
            drawn: 0,
        };
        let Some(m) = self.module_mut(id) else { return };
        m.kit = Some(kit);
        self.t0.get_or_insert_with(Instant::now);
        if is_bar {
            self.draw_bar();
        } else {
            let id2 = id;
            self.render_module(id2, move |cr, w, h| draw_dock_content(cr, w, h, None));
        }
    }

    fn on_layer_closed(&mut self, id: u64) {
        eprintln!("[shell] layer {id} closed");
    }

    fn on_pointer_enter(&mut self, surface: &WlSurface, sx: f64, sy: f64) {
        if self.dock.as_ref().is_some_and(|d| &d.surface == surface) {
            self.over_dock = true;
            let h = dock_item_at(sx, sy);
            if h != self.dock_hover {
                self.dock_hover = h;
                self.draw_dock();
            }
        }
    }

    fn on_pointer_leave(&mut self, surface: &WlSurface) {
        if self.dock.as_ref().is_some_and(|d| &d.surface == surface) {
            self.over_dock = false;
            if self.dock_hover.take().is_some() {
                self.draw_dock();
            }
        }
    }

    fn on_pointer_motion(&mut self, sx: f64, sy: f64) {
        if !self.over_dock {
            return;
        }
        let h = dock_item_at(sx, sy);
        if h != self.dock_hover {
            self.dock_hover = h;
            self.draw_dock();
        }
    }

    fn on_pointer_button(&mut self, button: u32, pressed: bool) {
        const BTN_LEFT: u32 = 0x110;
        if pressed && button == BTN_LEFT {
            if let Some(i) = self.dock_hover {
                self.launch(i);
            }
        }
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

/// One dock item: glyph + tile color.
struct DockItem {
    glyph: &'static str,
    color: (f64, f64, f64, f64),
}

/// Draw the dock tray (pure).
/// Dock layout constants (shared by draw + hit test).
const DOCK_ICON: f64 = 42.0;
const DOCK_GAP: f64 = 10.0;
const DOCK_PAD: f64 = 9.0;
const DOCK_SEP_AT: usize = 3; // separator before the trash
const DOCK_SEP_SPACE: f64 = 15.0; // 1px line with 7px gaps each side

/// X position of dock item `i` (surface-local).
fn dock_item_x(i: usize) -> f64 {
    let mut x = DOCK_PAD + i as f64 * (DOCK_ICON + DOCK_GAP);
    if i >= DOCK_SEP_AT {
        x += DOCK_SEP_SPACE;
    }
    x
}

/// Which dock item is under surface-local (sx, sy)?
fn dock_item_at(sx: f64, sy: f64) -> Option<usize> {
    if sy < 9.0 || sy > 51.0 {
        return None;
    }
    (0..4).find(|&i| {
        let x = dock_item_x(i);
        sx >= x && sx <= x + DOCK_ICON
    })
}

fn draw_dock_content(cr: &cairo::Context, width: i32, height: i32, hovered: Option<usize>) {
    let w = width as f64;
    let h = height as f64;
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    // tray
    canvas::fill_rounded_rect(cr, 0.0, 0.0, w, h, 14.0, theme::DOCK_BG);

    let items = [
        DockItem {
            glyph: "F",
            color: theme::DOCK_FILES,
        },
        DockItem {
            glyph: ">_",
            color: theme::DOCK_TERM,
        },
        DockItem {
            glyph: "S",
            color: theme::DOCK_SETTINGS,
        },
        DockItem {
            glyph: "\u{232b}",
            color: theme::DOCK_TRASH,
        }, // ⌫ trash
    ];

    let icon = DOCK_ICON;
    let font = {
        let mut f = pango::FontDescription::from_string("Sans 13");
        f.set_weight(pango::Weight::Bold);
        f
    };

    for (i, it) in items.iter().enumerate() {
        let x = dock_item_x(i);
        let y = (h - icon) / 2.0;
        canvas::fill_rounded_rect(cr, x, y, icon, icon, 10.0, it.color);
        let (gw, gh) = canvas::label_size(cr, it.glyph, &font);
        canvas::draw_label(
            cr,
            it.glyph,
            &font,
            x + (icon - gw as f64) / 2.0,
            y + (icon - gh as f64) / 2.0,
            theme::ON_ACCENT,
        );
        if hovered == Some(i) {
            // hover highlight: translucent white overlay + accent dot
            canvas::fill_rounded_rect(cr, x, y, icon, icon, 10.0, (1.0, 1.0, 1.0, 0.28));
        }
        if i + 1 == DOCK_SEP_AT {
            let sx = x + icon + DOCK_GAP;
            cr.set_source_rgba(0.72, 0.72, 0.75, 0.9);
            cr.rectangle(sx + 6.0, 8.0, 1.0, h - 16.0);
            let _ = cr.fill();
        }
    }
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

/// Dock tray width for the current item set (must match draw_dock_content).
fn dock_width() -> i32 {
    let items = 4;
    let icon = 42.0;
    let gap = 10.0;
    let pad = 9.0;
    let sep_space = 15.0;
    (pad * 2.0 + items as f64 * icon + (items as f64 - 1.0) * gap + sep_space) as i32
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

    // module: menubar (full-width top bar)
    let bar_spec = LayerSpec::top_bar("ibde-bar", BAR_H);
    let (bar_id, bar_surface, _ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&bar_spec, &qh)
        .expect("layer shell unavailable");
    app.bar = Some(Module {
        id: bar_id,
        surface: bar_surface,
        kit: None,
    });

    // module: dock (floating, bottom-center, margin below)
    let dock_spec = LayerSpec::bottom_dock("ibde-dock", dock_width(), DOCK_H, DOCK_EDGE_GAP);
    let (dock_id, dock_surface, _ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&dock_spec, &qh)
        .expect("layer shell unavailable");
    app.dock = Some(Module {
        id: dock_id,
        surface: dock_surface,
        kit: None,
    });

    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no configure arrived");
    assert!(
        app.bar.as_ref().is_some_and(|m| m.configured())
            && app.dock.as_ref().is_some_and(|m| m.configured()),
        "bar/dock never configured"
    );

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
        // clock tick (non-blocking — wayland events drive everything else)
        match rx.recv_timeout(Duration::from_millis(0)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                app.frames += 1;
                app.draw_bar(); // only the bar ticks; the dock is event-driven
                if let Some((fps, elapsed)) = app.fps.tick() {
                    println!(
                        "STATS redraws_per_s={fps} rss_kb={} t={elapsed:.1}",
                        rss_kb()
                    );
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // canonical wayland poll loop (dispatch_pending alone never
        // reads the socket — pointer events would never arrive)
        if let Err(e) = queue.dispatch_pending(&mut app) {
            eprintln!("dispatch error: {e}");
            break;
        }
        let _ = conn.flush();
        if let Some(guard) = queue.prepare_read() {
            use std::os::fd::AsRawFd;
            let mut fds = [libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            }];
            // SAFETY: poll(2) on one valid fd
            let r = unsafe { libc::poll(fds.as_mut_ptr(), 1, 200) };
            if r > 0 && (fds[0].revents & libc::POLLIN) != 0 {
                if let Err(e) = guard.read() {
                    eprintln!("wayland read error: {e}");
                    break;
                }
                if let Err(e) = queue.dispatch_pending(&mut app) {
                    eprintln!("dispatch error: {e}");
                    break;
                }
                let _ = conn.flush();
            }
        }
    }

    println!(
        "SUMMARY seconds={seconds} rss_kb={} bar_draws={} dock_draws={}",
        rss_kb(),
        app.bar
            .as_ref()
            .map(|m| m.kit.as_ref().map(|k| k.drawn).unwrap_or(0))
            .unwrap_or(0),
        app.dock
            .as_ref()
            .map(|m| m.kit.as_ref().map(|k| k.drawn).unwrap_or(0))
            .unwrap_or(0),
    );
}
