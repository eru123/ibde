//! ibde-terminal — the flagship window (ADR-0011).
//!
//! End-to-end path: PTY bytes → vte parser (TermModel) → TextGrid damage
//! → glyph atlas → wl_shm canvas → xdg toplevel commit. Power layer:
//! scrollback (Shift+PgUp/PgDn), search (Ctrl+Shift+F), tabs
//! (Ctrl+Shift+T new, Ctrl+Shift+W close, Ctrl+PgUp/PgDn switch).
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

use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySystem};

static RUNNING: AtomicBool = AtomicBool::new(true);

/// One terminal tab: its own PTY, model, and parser.
struct Tab {
    model: TermModel,
    feed: Feed,
    writer: Box<dyn std::io::Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
    title: String,
}

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
    tabs: Vec<Tab>,
    active: usize,
    exec_cmd: Option<String>,
    default_cols: usize,
    default_rows: usize,
    font_size: i32,
    configured: bool,
    search: Option<String>,
    last_match: Option<usize>,
    needs_render: bool,
    pending_new_tab: bool,
    // input
    focused: bool,
    kbd: Option<ibde_ui::input::Keyboard>,
}

impl App {
    fn new(
        exec_cmd: Option<String>,
        default_cols: usize,
        default_rows: usize,
        font_size: i32,
    ) -> Self {
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
            tabs: Vec::new(),
            active: 0,
            exec_cmd,
            default_cols,
            default_rows,
            font_size,
            configured: false,
            search: None,
            last_match: None,
            needs_render: false,
            pending_new_tab: false,
            focused: false,
            kbd: None,
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

    /// Spawn a new tab running the configured command; its reader thread
    /// sends (tab_index, bytes) on `tx`.
    fn spawn_tab(&mut self, tx: &mpsc::Sender<(usize, Vec<u8>)>) -> Option<()> {
        let idx = self.tabs.len();
        let (cols, rows) = self.cells();
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows: rows as u16,
                cols: cols as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .ok()?;
        let spawn = match &self.exec_cmd {
            Some(sh_line) => {
                let mut c = CommandBuilder::new("/bin/sh");
                c.arg("-c");
                c.arg(sh_line);
                c
            }
            None => CommandBuilder::new_default_prog(),
        };
        let child = pair.slave.spawn_command(spawn).ok()?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().ok()?;
        let writer = pair.master.take_writer().ok()?;
        let tx2 = tx.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 65536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx2.send((idx, buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        self.tabs.push(Tab {
            model: TermModel::new(cols, rows),
            feed: Feed::new(),
            writer,
            child,
            _master: pair.master,
            title: format!("sh {}", idx + 1),
        });
        Some(())
    }

    fn close_tab(&mut self) {
        if self.tabs.len() <= 1 {
            RUNNING.store(false, Ordering::Relaxed);
            return;
        }
        let mut t = self.tabs.remove(self.active);
        let _ = t.child.kill();
        let _ = t.child.wait();
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        self.needs_render = true;
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
        for t in self.tabs.iter_mut() {
            t.model.grid.resize(cols, rows);
            t.model.grid.mark_all_dirty();
        }
        eprintln!("[term] window {cols}x{rows} cells ({w}x{h} px)");
    }

    /// Render if the active grid is damaged; true when a commit happened.
    fn render_if_dirty(&mut self) -> bool {
        if self.atlas.is_none() || self.canvas.is_none() || self.tabs.is_empty() {
            return false;
        }
        let atlas = self.atlas.as_mut().unwrap();
        let canvas = self.canvas.as_ref().unwrap();
        let tab = self.tabs.get_mut(self.active).unwrap();
        let stats = render_grid(atlas, canvas, &mut tab.model.grid);
        if stats.rows_drawn == 0 && self.search.is_none() && self.tabs.len() <= 1 {
            return false;
        }
        if self.tabs.len() > 1 {
            draw_tab_bar(canvas, &self.tab_labels(), self.active);
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

    fn tab_labels(&self) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}: {}", i + 1, t.title))
            .collect()
    }

    fn dump_grid(&self) {
        let Some(m) = self.tabs.get(self.active).map(|t| &t.model) else {
            return;
        };
        println!(
            "=== GRID START === (tab {} of {}, view -{} rows)",
            self.active + 1,
            self.tabs.len(),
            m.grid.view
        );
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
            for px in data.as_chunks::<4>().0 {
                let _ = f.write(&[px[2], px[1], px[0]]);
            }
        }
    }

    /// All case-insensitive matches (absolute rows) for `q` across the
    /// active tab's history + live grid.
    fn search_matches(&self, q: &str) -> Vec<usize> {
        let Some(m) = self.tabs.get(self.active).map(|t| &t.model) else {
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
        let Some(m) = self.tabs.get_mut(self.active).map(|t| &mut t.model) else {
            return;
        };
        let hist = m.grid.history_len();
        if a < hist {
            let view = hist - a;
            m.grid.scroll_view(view as i32);
        } else {
            m.grid.scroll_view(-(m.grid.view as i32));
        }
    }
}

/// Tab bar strip along the top of the terminal surface.
fn draw_tab_bar(canvas: &Canvas, labels: &[String], active: usize) {
    let Ok(cr) = canvas.context() else { return };
    let w = canvas.width as f64;
    cr.set_operator(cairo::Operator::Over);
    cr.set_source_rgba(0.10, 0.11, 0.13, 0.96);
    cr.rectangle(0.0, 0.0, w, 24.0);
    let _ = cr.fill();
    let font = pango::FontDescription::from_string("Sans 10");
    let mut x = 8.0;
    for (i, label) in labels.iter().enumerate().take(9) {
        let (lw, lh) = ibde_ui::canvas::label_size(&cr, label, &font);
        if i == active {
            cr.set_source_rgba(0.29, 0.31, 0.35, 1.0);
            cr.rectangle(x - 4.0, 2.0, lw as f64 + 8.0, 20.0);
            let _ = cr.fill();
        }
        let color = if i == active {
            (1.0, 1.0, 1.0, 1.0)
        } else {
            (0.75, 0.76, 0.79, 1.0)
        };
        ibde_ui::canvas::draw_label(&cr, label, &font, x, (24.0 - lh as f64) / 2.0, color);
        x += lw as f64 + 18.0;
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
            // compositor our geometry and commit a matching buffer.
            if let (Some(xds), Some(m)) = (
                self.xds.as_ref(),
                self.tabs.get(self.active).map(|t| &t.model),
            ) {
                let w = (m.grid.cols() * self.cell_w()) as i32;
                let h = (m.grid.rows() * self.cell_h()) as i32;
                xds.set_window_geometry(0, 0, w, h);
            }
            let _ = self.render_if_dirty();
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
        const KEY_ESC: u32 = 1;
        const KEY_ENTER: u32 = 28;
        const KEY_BACKSPACE: u32 = 14;
        const KEY_F: u32 = 33;
        const KEY_T: u32 = 20;
        const KEY_W: u32 = 17;
        const KEY_PGUP: u32 = 104;
        const KEY_PGDN: u32 = 109;
        if !pressed || !self.focused {
            return;
        }
        let Some(k) = self.kbd.as_ref() else { return };
        // tabs (Ctrl+Shift+T/W, Ctrl+PgUp/PgDn switch)
        if k.ctrl_held() && k.shift_held() {
            if keycode == KEY_T {
                // spawned by the main loop (needs the channel); flag it
                self.needs_render = true;
                self.pending_new_tab = true;
                return;
            }
            if keycode == KEY_W {
                self.close_tab();
                return;
            }
        }
        if k.ctrl_held() && (keycode == KEY_PGUP || keycode == KEY_PGDN) && self.tabs.len() > 1 {
            if keycode == KEY_PGDN {
                self.active = (self.active + 1) % self.tabs.len();
            } else {
                self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
            }
            if let Some(t) = self.tabs.get_mut(self.active) {
                t.model.grid.mark_all_dirty();
            }
            self.needs_render = true;
            return;
        }
        // search: Ctrl+Shift+F opens; while open, keys edit/jump/close
        if self.search.is_some() {
            match keycode {
                KEY_ESC => {
                    self.search = None;
                    self.last_match = None;
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.model.grid.scroll_view(-(t.model.grid.view as i32));
                        t.model.grid.mark_all_dirty();
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
                        if let Some(t) = self.tabs.get_mut(self.active) {
                            t.model.grid.mark_all_dirty();
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
                    if let Some(t) = self.tabs.get_mut(self.active) {
                        t.model.grid.mark_all_dirty();
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
                        if let Some(t) = self.tabs.get_mut(self.active) {
                            t.model.grid.mark_all_dirty();
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
        // scrollback: Shift+PageUp/Down
        if k.shift_held() && (keycode == KEY_PGUP || keycode == KEY_PGDN) {
            if let Some(t) = self.tabs.get_mut(self.active) {
                let step = t.model.grid.rows() as i32;
                if keycode == KEY_PGUP {
                    t.model.grid.scroll_view(step);
                } else {
                    t.model.grid.scroll_view(-step);
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
        if let Some(t) = self.tabs.get_mut(self.active) {
            use std::io::Write;
            let _ = t.writer.write_all(&bytes);
            let _ = t.writer.flush();
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

    let conn = Connection::connect_to_env().expect("no wayland compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App::new(exec_cmd, cols, rows, font_size);
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

    // --- PTY tabs: one channel for all reader threads ---
    let (tx, rx) = mpsc::channel::<(usize, Vec<u8>)>();
    app.spawn_tab(&tx).expect("first tab");

    // --- Main loop: canonical wayland poll loop + PTY channel ---
    // dispatch_pending alone never READS the socket; without prepare_read
    // + poll + guard.read(), seat capability updates, keymaps and key
    // events silently never arrive (lesson paid for in bench v1).
    let t0 = Instant::now();
    let mut exited = false;
    loop {
        if !RUNNING.load(Ordering::Relaxed) || t0.elapsed().as_secs_f64() >= seconds {
            break;
        }

        // PTY output (drain whatever the reader threads have)
        loop {
            match rx.try_recv() {
                Ok((idx, bytes)) => {
                    if let Some(t) = app.tabs.get_mut(idx) {
                        t.feed.advance(&mut t.model, &bytes);
                        // switching to a tab with fresh content repaints it
                        let _ = idx;
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

        // deferred actions from event handlers (never spawn/render inside
        // the dispatch callback)
        if app.pending_new_tab {
            app.pending_new_tab = false;
            app.spawn_tab(&tx);
            app.active = app.tabs.len() - 1;
            app.needs_render = true;
        }
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
    for t in app.tabs.iter_mut() {
        let _ = t.child.kill();
        let _ = t.child.wait();
    }
}
