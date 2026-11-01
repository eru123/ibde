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
    ls: Option<ibde_ui::session::ZwlrLayerSurfaceV1>,
    kit: Option<Kit>,
}

impl Module {
    fn configured(&self) -> bool {
        self.kit.is_some()
    }
}

/// A tiny hand-drawn cursor surface (no cursor-theme dependency).
struct CursorKit {
    surface: WlSurface,
    _buf: ShmBuffer,
}

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    bar: Option<Module>,
    dock: Option<Module>,
    menu: Option<Module>,
    cursor: Option<CursorKit>,
    frames: u32,
    fps: FpsCounter,
    t0: Option<Instant>,
    // pointer/dock state
    dock_hover: Option<usize>,
    over_dock: bool,
    over_bar: bool,
    over_pill: bool,
    menu_hover: Option<usize>,
    over_menu: bool,
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
            menu: None,
            cursor: None,
            frames: 0,
            fps: FpsCounter::new(),
            t0: None,
            dock_hover: None,
            over_dock: false,
            over_bar: false,
            over_pill: false,
            menu_hover: None,
            over_menu: false,
            terminal_cmd: std::env::var("IBDE_TERMINAL_CMD")
                .unwrap_or_else(|_| "ibde-terminal".into()),
        }
    }

    fn module_mut(&mut self, id: u64) -> Option<&mut Module> {
        if self.bar.as_ref().is_some_and(|m| m.id == id) {
            self.bar.as_mut()
        } else if self.dock.as_ref().is_some_and(|m| m.id == id) {
            self.dock.as_mut()
        } else if self.menu.as_ref().is_some_and(|m| m.id == id) {
            self.menu.as_mut()
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

    fn draw_menu(&mut self) {
        let Some(id) = self.menu.as_ref().map(|m| m.id) else { return };
        let hovered = self.menu_hover;
        self.render_module(id, move |cr, w, h| draw_menu_content(cr, w, h, hovered));
    }

    fn open_menu(&mut self) {
        if self.menu.is_some() {
            return;
        }
        let Some(session) = self.session.as_mut() else { return };
        let Some(qh) = self.qh.clone() else { return };
        // popover below the IBDE pill (top-left), above everything else
        let spec = ibde_ui::session::LayerSpec {
            namespace: "ibde-menu".into(),
            layer: ibde_ui::session::Layer::Top,
            anchors: ibde_ui::session::Anchor::Top | ibde_ui::session::Anchor::Left,
            width: MENU_W,
            height: MENU_H,
            exclusive_zone: 0,
            margins: (8, 0, BAR_H + 4, 0),
        };
        match session.create_layer(&spec, &qh) {
            Some((id, surface, ls)) => {
                self.menu = Some(Module { id, surface, ls: Some(ls), kit: None });
            }
            None => eprintln!("[shell] menu layer unavailable"),
        }
    }

    fn close_menu(&mut self) {
        if let Some(m) = self.menu.take() {
            if let Some(ls) = &m.ls {
                ls.destroy();
            }
            self.menu_hover = None;
            self.over_menu = false;
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
    }

    /// Our 12×18 arrow cursor on the entered surface.
    fn set_cursor(&self, serial: u32) {
        let (Some(cursor), Some(pointer)) = (
            self.cursor.as_ref(),
            self.session.as_ref().and_then(|s| s.pointer.clone()),
        ) else {
            return;
        };
        pointer.set_cursor(serial, Some(&cursor.surface), 0, 0);
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
    }

    fn build_cursor(&mut self) {
        let (Some(session), Some(qh)) = (self.session.as_ref(), self.qh.clone()) else {
            return;
        };
        let Some(shm) = session.shm.clone() else { return };
        let Some(compositor) = session.compositor.clone() else { return };
        let surface = compositor.create_surface(&qh, ());
        let mut buf = ShmBuffer::new(&shm, 12, 18, &qh);
        {
            let cs = Canvas::new(12, 18).expect("cursor canvas");
            {
                let cr = cs.context().unwrap();
                // classic arrow: white fill, graphite outline
                cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cr.move_to(1.0, 1.0);
                cr.line_to(1.0, 13.0);
                cr.line_to(4.5, 9.8);
                cr.line_to(7.0, 15.0);
                cr.line_to(9.0, 14.0);
                cr.line_to(6.6, 9.0);
                cr.line_to(11.0, 9.0);
                cr.close_path();
                cr.fill_preserve().unwrap();
                cr.set_source_rgba(0.15, 0.16, 0.19, 1.0);
                let _ = cr.stroke();
            }
            let mut cs = cs;
            cs.copy_to(&mut buf);
        }
        surface.attach(Some(buf.wayland_buffer()), 0, 0);
        surface.damage_buffer(0, 0, 12, 18);
        surface.commit();
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
        self.cursor = Some(CursorKit { surface, _buf: buf });
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
        let is_menu = self.menu.as_ref().is_some_and(|m| m.id == id);
        let is_known = is_bar
            || is_menu
            || self.dock.as_ref().is_some_and(|d| d.id == id);
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
        } else if is_menu {
            self.draw_menu();
        } else {
            let id2 = id;
            self.render_module(id2, move |cr, w, h| draw_dock_content(cr, w, h, None));
        }
    }

    fn on_layer_closed(&mut self, id: u64) {
        eprintln!("[shell] layer {id} closed");
        if self.menu.as_ref().is_some_and(|m| m.id == id) {
            self.menu = None;
        }
    }

    fn on_pointer_enter(
        &mut self,
        surface: &WlSurface,
        serial: u32,
        sx: f64,
        sy: f64,
    ) {
        self.set_cursor(serial);
        if self.dock.as_ref().is_some_and(|d| &d.surface == surface) {
            self.over_dock = true;
            let h = dock_item_at(sx, sy);
            if h != self.dock_hover {
                self.dock_hover = h;
                self.draw_dock();
            }
        } else if self.bar.as_ref().is_some_and(|b| &b.surface == surface) {
            self.over_bar = true;
            self.over_pill = bar_pill_at(sx, sy);
        } else if self.menu.as_ref().is_some_and(|m| &m.surface == surface) {
            self.over_menu = true;
            let h = menu_item_at(sx, sy);
            if h != self.menu_hover {
                self.menu_hover = h;
                self.draw_menu();
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
        if self.over_dock {
            let h = dock_item_at(sx, sy);
            if h != self.dock_hover {
                self.dock_hover = h;
                self.draw_dock();
            }
        } else if self.over_bar {
            self.over_pill = bar_pill_at(sx, sy);
        } else if self.over_menu {
            let h = menu_item_at(sx, sy);
            if h != self.menu_hover {
                self.menu_hover = h;
                self.draw_menu();
            }
        }
    }

    fn on_pointer_button(&mut self, button: u32, pressed: bool) {
        const BTN_LEFT: u32 = 0x110;
        if !pressed || button != BTN_LEFT {
            return;
        }
        if self.over_menu {
            match self.menu_hover {
                Some(0) => {
                    // About This Machine (v0: close; panel arrives later)
                    self.close_menu();
                }
                Some(1) => {
                    eprintln!("[shell] quit requested from IBDE menu");
                    std::process::exit(0);
                }
                _ => {}
            }
            return;
        }
        if self.over_bar {
            if self.over_pill {
                if self.menu.is_some() {
                    self.close_menu();
                } else {
                    self.open_menu();
                }
            } else if self.menu.is_some() {
                self.close_menu(); // click-away on the bar
            }
            return;
        }
        if let Some(i) = self.dock_hover {
            self.launch(i);
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
    if !(9.0..=51.0).contains(&sy) {
        return None;
    }
    (0..4).find(|&i| {
        let x = dock_item_x(i);
        sx >= x && sx <= x + DOCK_ICON
    })
}

/// Is (sx, sy) on the IBDE pill? (bar surface-local; matches draw)
fn bar_pill_at(sx: f64, sy: f64) -> bool {
    (8.0..=52.0).contains(&sx) && (5.0..=23.0).contains(&sy)
}

/// IBDE menu rows.
const MENU_ROWS: [&str; 2] = ["About This Machine", "Quit IBDE"];
const MENU_W: i32 = 208;
const MENU_H: i32 = 96;
const MENU_ROW_H: f64 = 34.0;

fn menu_item_at(sx: f64, sy: f64) -> Option<usize> {
    if sx < 0.0 || sx > MENU_W as f64 || sy < 24.0 {
        return None;
    }
    let i = ((sy - 24.0) / MENU_ROW_H) as usize;
    (i < MENU_ROWS.len()).then_some(i)
}

fn draw_menu_content(cr: &cairo::Context, width: i32, height: i32, hovered: Option<usize>) {
    let w = width as f64;
    let h = height as f64;
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    // popover card
    canvas::fill_rounded_rect(cr, 0.0, 0.0, w, h, 10.0, (1.0, 1.0, 1.0, 0.97));
    cr.set_source_rgba(0.85, 0.85, 0.87, 1.0);
    cr.set_line_width(1.0);
    cr.new_path();
    cr.arc(10.0, 10.0, 9.5, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.arc(w - 10.0, 10.0, 9.5, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(w - 10.0, h - 10.0, 9.5, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(10.0, h - 10.0, 9.5, 0.5 * std::f64::consts::PI, std::f64::consts::PI);
    cr.close_path();
    let _ = cr.stroke();

    let font = pango::FontDescription::from_string("Sans 11");
    // header
    let font_semibold = {
        let mut f = pango::FontDescription::from_string("Sans 11");
        f.set_weight(pango::Weight::Semibold);
        f
    };
    canvas::draw_label(
        cr,
        "IBDE",
        &font_semibold,
        14.0,
        7.0,
        theme::TEXT,
    );
    cr.set_source_rgba(0.88, 0.88, 0.90, 1.0);
    cr.rectangle(8.0, 23.5, w - 16.0, 1.0);
    let _ = cr.fill();

    for (i, row) in MENU_ROWS.iter().enumerate() {
        let y = 24.0 + i as f64 * MENU_ROW_H;
        if hovered == Some(i) {
            canvas::fill_rounded_rect(cr, 6.0, y + 2.0, w - 12.0, MENU_ROW_H - 6.0, 6.0, (0.90, 0.91, 0.94, 1.0));
        }
        let (_tw, th) = canvas::label_size(cr, row, &font);
        canvas::draw_label(cr, row, &font, 16.0, y + (MENU_ROW_H - th as f64) / 2.0, theme::TEXT);
    }
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
    let (bar_id, bar_surface, ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&bar_spec, &qh)
        .expect("layer shell unavailable");
    app.bar = Some(Module {
        id: bar_id,
        surface: bar_surface,
        ls: Some(ls),
        kit: None,
    });

    // module: dock (floating, bottom-center, margin below)
    let dock_spec = LayerSpec::bottom_dock("ibde-dock", dock_width(), DOCK_H, DOCK_EDGE_GAP);
    let (dock_id, dock_surface, ls) = app
        .session
        .as_mut()
        .unwrap()
        .create_layer(&dock_spec, &qh)
        .expect("layer shell unavailable");
    app.dock = Some(Module {
        id: dock_id,
        surface: dock_surface,
        ls: Some(ls),
        kit: None,
    });

    // hand-drawn cursor (set on every pointer enter)
    app.build_cursor();

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
