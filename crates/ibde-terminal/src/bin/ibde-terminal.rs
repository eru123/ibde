//! ibde-terminal — the flagship window (ADR-0011), v0.5-milestone slice.
//!
//! End-to-end path proven here: PTY bytes → vte parser (TermModel) →
//! TextGrid damage → glyph atlas → wl_shm canvas → xdg toplevel commit.
//! Input (wl_keyboard + xkbcommon) lands in the next chunk; until then
//! this runs a command non-interactively, which the bench and the
//! headless test rig exploit via `--dump`.
//!
//! Usage:
//!   ibde-terminal [--exec "CMD"] [--seconds N] [--cols N] [--rows N]
//!                 [--font-size N] [--dump]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ibde_terminal::{Feed, TermModel};
use ibde_ui::atlas::{render_grid, GlyphAtlas};
use ibde_ui::canvas::Canvas;
use ibde_ui::session::{Connection, QueueHandle, Session, SessionHost, WindowSpec, WlSurface};
use ibde_ui::shm::ShmBuffer;
use ibde_ui::stats::rss_kb;

use portable_pty::{CommandBuilder, NativePtySystem, PtySystem};

static RUNNING: AtomicBool = AtomicBool::new(true);

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    surface: Option<WlSurface>,
    window_id: u64,
    /// last toplevel dims (0 = unset)
    top_w: i32,
    top_h: i32,
    buf: Option<ShmBuffer>,
    canvas: Option<Canvas>,
    atlas: Option<GlyphAtlas>,
    model: Option<TermModel>,
    default_cols: usize,
    default_rows: usize,
    font_size: i32,
    configured: bool,
}

impl App {
    fn new(default_cols: usize, default_rows: usize, font_size: i32) -> Self {
        Self {
            session: None,
            conn: None,
            qh: None,
            surface: None,
            window_id: 0,
            top_w: 0,
            top_h: 0,
            buf: None,
            canvas: None,
            atlas: None,
            model: None,
            default_cols,
            default_rows,
            font_size,
            configured: false,
        }
    }

    fn cells(&self) -> (usize, usize) {
        let (cw, ch) = self
            .atlas
            .as_ref()
            .map(|a| (a.cell_w as usize, a.cell_h as usize))
            .unwrap_or((8, 16));
        let (w, h) = if self.top_w > 0 && self.top_h > 0 {
            (self.top_w as usize, self.top_h as usize)
        } else {
            (self.default_cols * cw, self.default_rows * ch)
        };
        ((w / cw).max(8), (h / ch).max(2))
    }

    fn size_buffers(&mut self) {
        let shm = self
            .session
            .as_ref()
            .and_then(|s| s.shm.clone())
            .expect("size_buffers without shm");
        let qh = self.qh.clone().expect("size_buffers without qh");
        if self.atlas.is_none() {
            self.atlas = Some(GlyphAtlas::new(self.font_size));
        }
        let (cols, rows) = self.cells();
        let cw = self.atlas.as_ref().unwrap().cell_w as usize;
        let ch = self.atlas.as_ref().unwrap().cell_h as usize;
        let w = (cols * cw) as i32;
        let h = (rows * ch) as i32;
        self.buf = Some(ShmBuffer::new(&shm, w, h, &qh));
        self.canvas = Some(Canvas::new(w, h).expect("canvas"));
        if let Some(m) = self.model.as_mut() {
            m.grid.resize(cols, rows);
        } else {
            self.model = Some(TermModel::new(cols, rows));
        }
        eprintln!("[term] window {cols}x{rows} cells ({w}x{h} px)");
    }

    /// Render if the grid is damaged; true when a commit happened.
    fn render_if_dirty(&mut self) -> bool {
        if self.atlas.is_none() || self.canvas.is_none() || self.model.is_none() {
            return false;
        }
        let atlas = self.atlas.as_mut().unwrap();
        let canvas = self.canvas.as_ref().unwrap();
        let model = self.model.as_mut().unwrap();
        let stats = render_grid(atlas, canvas, &mut model.grid);
        if stats.rows_drawn == 0 {
            return false;
        }
        if let (Some(c), Some(b)) = (self.canvas.as_mut(), self.buf.as_mut()) {
            c.copy_to(b);
        }
        if let (Some(b), Some(sf)) = (&self.buf, &self.surface) {
            sf.attach(Some(b.wayland_buffer()), 0, 0);
            sf.damage_buffer(0, 0, b.width, b.height);
            sf.commit();
        }
        true
    }

    fn dump_grid(&self) {
        let Some(m) = self.model.as_ref() else { return };
        println!("=== GRID START ===");
        for y in 0..m.grid.rows() {
            let row: String = (0..m.grid.cols())
                .filter_map(|x| m.grid.get(x, y))
                .map(|c| if c.ch == '\0' { ' ' } else { c.ch })
                .collect();
            println!("|{}", row.trim_end());
        }
        println!("=== GRID END ===");
    }
}

impl SessionHost for App {
    fn session_slot(&mut self) -> &mut Option<Session> {
        &mut self.session
    }

    fn on_toplevel_configure(&mut self, id: u64, width: i32, height: i32) {
        if id == self.window_id {
            self.top_w = width;
            self.top_h = height;
        }
    }

    fn on_window_configure(&mut self, id: u64) {
        if id == self.window_id {
            self.size_buffers();
            self.configured = true;
        }
    }

    fn on_window_close(&mut self, _id: u64) {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

ibde_ui::delegate_ibde_session!(App);

fn main() {
    let mut exec_cmd: Option<String> = None;
    let mut seconds = f64::INFINITY;
    let mut cols = 80usize;
    let mut rows = 24usize;
    let mut font_size = 14i32;
    let mut dump = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().expect("missing value for flag");
        match a.as_str() {
            "--exec" => exec_cmd = Some(val()),
            "--seconds" => seconds = val().parse().expect("bad --seconds"),
            "--cols" => cols = val().parse().expect("bad --cols"),
            "--rows" => rows = val().parse().expect("bad --rows"),
            "--font-size" => font_size = val().parse().expect("bad --font-size"),
            "--dump" => dump = true,
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(1);
            }
        }
    }

    // --- PTY ---
    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(portable_pty::PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let spawn = match &exec_cmd {
        Some(sh_line) => {
            let mut c = CommandBuilder::new("/bin/sh");
            c.arg("-c");
            c.arg(sh_line);
            c
        }
        None => CommandBuilder::new_default_prog(),
    };
    let mut child = pair.slave.spawn_command(spawn).expect("spawn");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().expect("pty reader");
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 65536];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // --- Wayland ---
    let conn = Connection::connect_to_env().expect("no wayland compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App::new(cols, rows, font_size);
    app.conn = Some(conn.clone());
    app.qh = Some(qh.clone());

    Session::begin(&mut app, &conn, &qh);
    queue.roundtrip(&mut app).expect("roundtrip failed");
    assert!(
        app.session.as_ref().is_some_and(|s| s.ready()),
        "missing globals (need a wlroots compositor)"
    );

    let spec = WindowSpec::titled("IBDE Terminal");
    let (window_id, surface, _xds, _tl) = app
        .session
        .as_mut()
        .unwrap()
        .create_toplevel(&spec, &qh)
        .expect("xdg_wm_base unavailable");
    app.window_id = window_id;
    app.surface = Some(surface);
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no window configure");
    assert!(app.configured, "window never configured");

    // --- Main loop: PTY channel + wayland, poll-free via recv_timeout ---
    let mut feed = Feed::new();
    let t0 = Instant::now();
    let mut exited = false;
    loop {
        if !RUNNING.load(Ordering::Relaxed) || t0.elapsed().as_secs_f64() >= seconds {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                if let Some(m) = app.model.as_mut() {
                    feed.advance(m, &bytes);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // drain remaining then finish
                while let Ok(bytes) = rx.try_recv() {
                    if let Some(m) = app.model.as_mut() {
                        feed.advance(m, &bytes);
                    }
                }
                exited = true;
            }
        }
        if app.render_if_dirty() {
            if let Some(c) = &app.conn {
                let _ = c.flush();
            }
        }
        if let Err(e) = queue.dispatch_pending(&mut app) {
            eprintln!("dispatch error: {e}");
            break;
        }
        if let Some(c) = &app.conn {
            let _ = c.flush();
        }
        if exited {
            // give the compositor a moment to show the last frame
            std::thread::sleep(Duration::from_millis(300));
            break;
        }
    }

    if dump {
        app.dump_grid();
        println!("SUMMARY exited={} rss_kb={}", exited, rss_kb());
    }
    let _ = child.kill();
    let _ = child.wait();
}
