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
    xds: Option<ibde_ui::session::XdgSurface>,
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
    search: Option<String>,
    last_match: Option<usize>,
    needs_render: bool,
    // input
    focused: bool,
    kbd: Option<ibde_ui::input::Keyboard>,
    writer: Option<Box<dyn std::io::Write + Send>>,
}

impl App {
    fn new(default_cols: usize, default_rows: usize, font_size: i32) -> Self {
        Self {
            session: None,
            conn: None,
            qh: None,
            surface: None,
            xds: None,
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
            search: None,
            last_match: None,
            needs_render: false,
            focused: false,
            kbd: None,
            writer: None,
        }
    }

    fn cell_w(&self) -> usize {
        self.atlas.as_ref().map(|a| a.cell_w as usize).unwrap_or(8)
    }

    fn cell_h(&self) -> usize {
        self.atlas.as_ref().map(|a| a.cell_h as usize).unwrap_or(16)
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
        if stats.rows_drawn == 0 && self.search.is_none() {
            return false;
        }
        if let Some(q) = self.search.as_ref() {
            draw_search_overlay(canvas, q, self.search_matches(q), self.last_match);
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
        println!("=== GRID START === (view -{} rows)", m.grid.view);
        for y in 0..m.grid.rows() {
            let row: String = (0..m.grid.cols())
                .map(|x| m.grid.visible_row(y)[x])
                .map(|c| if c.ch == '\0' { ' ' } else { c.ch })
                .collect();
            println!("|{}", row.trim_end());
        }
        println!("=== GRID END ===");
    }

    /// Write the rendered canvas as a binary PPM (debug; no deps).
    fn dump_canvas_ppm(&mut self, path: &str) {
        use std::io::Write;
        let Some(c) = self.canvas.as_mut() else {
            return;
        };
        let Ok(f) = std::fs::File::create(path) else {
            return;
        };
        let mut f = std::io::BufWriter::new(f);
        let _ = write!(f, "P6\n{} {}\n255\n", c.width, c.height);
        if let Ok(data) = c.surface_mut().data() {
            for px in data.chunks_exact(4) {
                // ARGB → RGB
                let _ = f.write(&[px[2], px[1], px[0]]);
            }
        }
    }
}

impl App {
    /// All case-insensitive matches (absolute rows) for `q` across
    /// history + live grid.
    fn search_matches(&self, q: &str) -> Vec<usize> {
        let Some(m) = self.model.as_ref() else {
            return Vec::new();
        };
        let ql = q.to_lowercase();
        if ql.is_empty() {
            return Vec::new();
        }
        let hist = m.grid.history_len();
        let mut out = Vec::new();
        for a in 0..hist + m.grid.rows() {
            let text: String = if a < hist {
                m.grid.history_row_text(a)
            } else {
                let gy = a - hist;
                (0..m.grid.cols())
                    .map(|x| m.grid.get(x, gy).map(|c| c.ch).unwrap_or(' '))
                    .collect()
            };
            if text.to_lowercase().contains(&ql) {
                out.push(a);
            }
        }
        out
    }

    /// Jump the view so absolute row `a` is the top visible row.
    fn jump_to_row(&mut self, a: usize) {
        let Some(m) = self.model.as_mut() else { return };
        let hist = m.grid.history_len();
        if a < hist {
            let view = hist - a;
            m.grid.scroll_view(view as i32);
        } else {
            m.grid.scroll_view(-(m.grid.view as i32));
        }
    }
}

/// Search overlay strip along the top of the terminal surface.
fn draw_search_overlay(canvas: &Canvas, query: &str, matches: Vec<usize>, current: Option<usize>) {
    let Ok(cr) = canvas.context() else { return };
    let w = canvas.width as f64;
    cr.set_operator(cairo::Operator::Over);
    cr.set_source_rgba(0.13, 0.14, 0.17, 0.94);
    cr.rectangle(0.0, 0.0, w, 26.0);
    let _ = cr.fill();
    let font = pango::FontDescription::from_string("Sans 11");
    let idx = matches
        .iter()
        .position(|m| Some(*m) == current)
        .map(|i| i + 1)
        .unwrap_or(0);
    let label = if query.is_empty() {
        "Search:".to_string()
    } else {
        format!("Search: {query}  [{idx}/{}]", matches.len())
    };
    let (_tw, th) = ibde_ui::canvas::label_size(&cr, &label, &font);
    ibde_ui::canvas::draw_label(
        &cr,
        &label,
        &font,
        8.0,
        (26.0 - th as f64) / 2.0,
        (1.0, 1.0, 1.0, 1.0),
    );
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
            // Complete every configure cycle immediately: tell the
            // compositor our geometry and commit a matching buffer
            // (sway tiles xdg windows and waits for this before mapping).
            if let (Some(xds), Some(m)) = (&self.xds, self.model.as_ref()) {
                let w = (m.grid.cols() * self.cell_w()) as i32;
                let h = (m.grid.rows() * self.cell_h()) as i32;
                xds.set_window_geometry(0, 0, w, h);
            }
            if let Some(m) = self.model.as_mut() {
                m.grid.mark_all_dirty();
            }
            let _ = self.render_if_dirty();
            // request a frame like every mainstream client does
            if let (Some(sf), Some(qh)) = (&self.surface, &self.qh) {
                ibde_ui::session::request_frame(sf, self.window_id, qh);
            }
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
    }

    fn on_window_close(&mut self, _id: u64) {
        RUNNING.store(false, Ordering::Relaxed);
    }

    fn on_keymap(&mut self, _format: u32, fd: std::os::fd::OwnedFd, size: u32) {
        match ibde_ui::input::Keyboard::from_fd(fd, size as usize) {
            Some(k) => self.kbd = Some(k),
            None => eprintln!("[term] keymap rejected"),
        }
    }

    fn on_keyboard_enter(&mut self, surface: &WlSurface) {
        self.focused = self.surface.as_ref() == Some(surface);
    }

    fn on_keyboard_leave(&mut self, _surface: &WlSurface) {
        self.focused = false;
    }

    fn on_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(k) = self.kbd.as_mut() {
            k.update_mask(depressed, latched, locked, group);
        }
    }

    fn on_key(&mut self, keycode: u32, pressed: bool) {
        const KEY_F: u32 = 33;
        const KEY_PGUP: u32 = 104;
        const KEY_PGDN: u32 = 109;
        if !pressed || !self.focused {
            return;
        }
        let Some(k) = self.kbd.as_ref() else { return };
        // search: Ctrl+Shift+F opens; while open, keys edit/jump/close
        if self.search.is_some() {
            match keycode {
                KEY_ESC => {
                    self.search = None;
                    self.last_match = None;
                    if let Some(m) = self.model.as_mut() {
                        m.grid.scroll_view(-(m.grid.view as i32));
                        m.grid.mark_all_dirty();
                    }
                    self.needs_render = true;
                    return;
                }
                KEY_ENTER => {
                    let q = self.search.clone().unwrap_or_default();
                    let matches = self.search_matches(&q);
                    if !matches.is_empty() {
                        let start = self
                            .last_match
                            .and_then(|cur| matches.iter().position(|m| *m > cur))
                            .unwrap_or(0);
                        let target = matches[start];
                        self.last_match = Some(target);
                        self.jump_to_row(target);
                        if let Some(m) = self.model.as_mut() {
                            m.grid.mark_all_dirty();
                        }
                        self.needs_render = true;
                    }
                    return;
                }
                KEY_BACKSPACE => {
                    if let Some(q) = self.search.as_mut() {
                        q.pop();
                    }
                    self.last_match = None;
                    if let Some(m) = self.model.as_mut() {
                        m.grid.mark_all_dirty();
                    }
                    self.needs_render = true;
                    return;
                }
                _ => {
                    let ev = k.translate(keycode + 8);
                    if !ev.utf8.is_empty() && ev.utf8.chars().all(|c| !c.is_control()) {
                        if let Some(q) = self.search.as_mut() {
                            q.push_str(&ev.utf8);
                        }
                        self.last_match = None;
                        if let Some(m) = self.model.as_mut() {
                            m.grid.mark_all_dirty();
                        }
                        self.needs_render = true;
                    }
                    return;
                }
            }
        }
        if k.ctrl_held() && k.shift_held() && keycode == KEY_F {
            self.search = Some(String::new());
            self.last_match = None;
            self.needs_render = true;
            return;
        }
        // scrollback: Shift+PageUp/Down (UX-SPEC power layer)
        if k.shift_held() && (keycode == KEY_PGUP || keycode == KEY_PGDN) {
            if let Some(m) = self.model.as_mut() {
                let step = m.grid.rows() as i32;
                if keycode == KEY_PGUP {
                    m.grid.scroll_view(step);
                } else {
                    m.grid.scroll_view(-step);
                }
            }
            self.needs_render = true;
            return;
        }
        let ev = k.translate(keycode + 8);
        if std::env::var_os("IBDE_DEBUG_KEYS").is_some() {
            eprintln!(
                "[key] code={keycode} sym={:#x} utf8={:?}",
                ev.keysym, ev.utf8
            );
        }
        let bytes = ibde_ui::input::key_to_terminal_bytes(&ev);
        if bytes.is_empty() {
            return;
        }
        if let Some(w) = self.writer.as_mut() {
            use std::io::Write;
            let _ = w.write_all(&bytes);
            let _ = w.flush();
        }
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
    let mut pty_master = Some(pair.master);
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

    let spec = WindowSpec::app("IBDE Terminal", "ibde.terminal");
    let (window_id, surface, xds, _tl) = app
        .session
        .as_mut()
        .unwrap()
        .create_toplevel(&spec, &qh)
        .expect("xdg_wm_base unavailable");
    app.window_id = window_id;
    app.surface = Some(surface);
    app.xds = Some(xds);
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no window configure");
    assert!(app.configured, "window never configured");

    // keyboard: bound automatically by the session glue when (and only
    // when) the seat advertises the capability; keymap + keys follow via
    // SessionHost. Never request one manually — see session.rs note.
    // PTY writer for keyboard input
    if let Some(m) = pty_master.as_mut() {
        app.writer = Some(m.take_writer().expect("pty writer"));
    }

    // --- Main loop: canonical wayland poll loop + PTY channel ---
    // dispatch_pending alone never READS the socket; without prepare_read
    // + poll + guard.read(), seat capability updates, keymaps and key
    // events silently never arrive (lesson paid for in bench v1).
    let mut feed = Feed::new();
    let t0 = Instant::now();
    let mut exited = false;
    loop {
        if !RUNNING.load(Ordering::Relaxed) || t0.elapsed().as_secs_f64() >= seconds {
            break;
        }

        // PTY output (drain whatever the reader thread has)
        loop {
            match rx.try_recv() {
                Ok(bytes) => {
                    if let Some(m) = app.model.as_mut() {
                        feed.advance(m, &bytes);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    exited = true;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if app.render_if_dirty() {
            if let Some(c) = &app.conn {
                let _ = c.flush();
            }
        }

        // render requested by event handlers (never render inside the
        // dispatch callback — requests during dispatch stall the queue)
        if app.needs_render {
            app.needs_render = false;
            if app.render_if_dirty() {
                let _ = conn.flush();
            }
        }

        // Wayland events
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
            // SAFETY: poll(2) on our own valid fd list
            let r = unsafe { libc::poll(fds.as_mut_ptr(), 1, 50) };
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

        if exited {
            // give the compositor a moment to show the last frame
            std::thread::sleep(Duration::from_millis(300));
            break;
        }
    }

    if dump {
        app.dump_grid();
        if let Ok(p) = std::env::var("IBDE_DUMP_CANVAS") {
            app.dump_canvas_ppm(&p);
        }

        println!("SUMMARY exited={} rss_kb={}", exited, rss_kb());
    }
    let _ = child.kill();
    let _ = child.wait();
}
