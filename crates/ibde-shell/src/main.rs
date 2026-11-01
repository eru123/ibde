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
    // keyboard + spotlight
    kbd: Option<ibde_ui::input::Keyboard>,
    spotlight: Option<Module>,
    toast: Option<Module>,
    toast_data: Option<Toast>,
    spot_query: String,
    spot_sel: usize,
    spot_apps: Vec<AppEntry>,
    spot_matches: Vec<usize>,
}

/// Active toast content.
struct Toast {
    app: String,
    summary: String,
    body: String,
    expires_at: Instant,
}

/// Commands from the D-Bus notification server thread.
enum NotifyCmd {
    Show {
        #[allow(dead_code)]
        id: u32,
        app: String,
        summary: String,
        body: String,
        timeout_ms: i64,
    },
    Close(#[allow(dead_code)] u32),
}

/// org.freedesktop.Notifications (spec subset) served by the shell.
struct NotificationsIface {
    tx: std::sync::mpsc::Sender<NotifyCmd>,
    next_id: std::sync::atomic::AtomicU32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
#[allow(clippy::too_many_arguments)] // fixed by the freedesktop spec
impl NotificationsIface {
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        _actions: Vec<String>,
        _hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id == 0 {
            self.next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1
        } else {
            replaces_id
        };
        let timeout_ms = if expire_timeout <= 0 {
            5000
        } else {
            expire_timeout as i64
        };
        let _ = self.tx.send(NotifyCmd::Show {
            id,
            app: app_name,
            summary,
            body,
            timeout_ms,
        });
        id
    }

    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".to_string(), "body-markup".to_string()]
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        ("ibde".into(), "IBDE".into(), "0.1".into(), "1.2".into())
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.send(NotifyCmd::Close(id));
    }
}

/// One scanned .desktop entry.
struct AppEntry {
    name: String,
    exec: String,
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
            kbd: None,
            spotlight: None,
            toast: None,
            toast_data: None,
            spot_query: String::new(),
            spot_sel: 0,
            spot_apps: Vec::new(),
            spot_matches: Vec::new(),
        }
    }

    fn module_mut(&mut self, id: u64) -> Option<&mut Module> {
        if self.bar.as_ref().is_some_and(|m| m.id == id) {
            self.bar.as_mut()
        } else if self.dock.as_ref().is_some_and(|m| m.id == id) {
            self.dock.as_mut()
        } else if self.menu.as_ref().is_some_and(|m| m.id == id) {
            self.menu.as_mut()
        } else if self.spotlight.as_ref().is_some_and(|m| m.id == id) {
            self.spotlight.as_mut()
        } else if self.toast.as_ref().is_some_and(|m| m.id == id) {
            self.toast.as_mut()
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
        if let Ok(path) = std::env::var("IBDE_DUMP_KIT") {
            if path.ends_with(&format!("m{id}.ppm")) {
                use std::io::Write;
                if let Ok(f) = std::fs::File::create(&path) {
                    let mut f = std::io::BufWriter::new(f);
                    let _ = write!(f, "P6\n{} {}\n255\n", k.canvas.width, k.canvas.height);
                    if let Ok(d) = k.canvas.surface_mut().data() {
                        for px in d.chunks_exact(4) {
                            let _ = f.write(&[px[2], px[1], px[0]]);
                        }
                    }
                }
            }
        }
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

    fn draw_spotlight(&mut self) {
        let Some(id) = self.spotlight.as_ref().map(|m| m.id) else {
            return;
        };
        let query = self.spot_query.clone();
        let sel = self.spot_sel;
        let rows: Vec<String> = self
            .spot_matches
            .iter()
            .take(7)
            .map(|&i| self.spot_apps[i].name.clone())
            .collect();
        self.render_module(id, move |cr, w, h| {
            draw_spotlight_content(cr, w, h, &query, &rows, sel)
        });
    }

    fn rescan_apps(&mut self) {
        self.spot_apps.clear();
        let mut dirs = vec![
            std::path::PathBuf::from("/usr/share/applications"),
            std::path::PathBuf::from("/usr/local/share/applications"),
        ];
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(std::path::PathBuf::from(home).join(".local/share/applications"));
        }
        for d in dirs {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "desktop") {
                    continue;
                }
                let Ok(body) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let mut name = None;
                let mut exec = None;
                let mut skip = false;
                for line in body.lines() {
                    match line {
                        l if l.starts_with("Name=") && name.is_none() => {
                            name = Some(l[5..].to_string());
                        }
                        l if l.starts_with("Exec=") && exec.is_none() => {
                            exec = Some(l[5..].to_string());
                        }
                        "NoDisplay=true" | "Hidden=true" | "Terminal=true" => skip = true,
                        _ => {}
                    }
                }
                if skip {
                    continue;
                }
                if let (Some(n), Some(x)) = (name, exec) {
                    self.spot_apps.push(AppEntry { name: n, exec: x });
                }
            }
        }
        self.spot_apps.sort_by(|a, b| a.name.cmp(&b.name));
        eprintln!("[shell] spotlight indexed {} apps", self.spot_apps.len());
    }

    fn update_spot_matches(&mut self) {
        let q = self.spot_query.to_lowercase();
        let mut scored: Vec<(i32, usize)> = self
            .spot_apps
            .iter()
            .enumerate()
            .filter_map(|(i, a)| fuzzy_score(&q, &a.name.to_lowercase()).map(|s| (s, i)))
            .collect();
        scored.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));
        self.spot_matches = scored.into_iter().map(|(_, i)| i).collect();
        self.spot_sel = 0;
    }

    fn open_spotlight(&mut self) {
        if self.spotlight.is_some() {
            self.close_spotlight();
            return;
        }
        self.rescan_apps();
        self.spot_query.clear();
        self.update_spot_matches();
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(qh) = self.qh.clone() else { return };
        let spec = ibde_ui::session::LayerSpec {
            namespace: "ibde-spotlight".into(),
            layer: ibde_ui::session::Layer::Overlay,
            // full-anchor: the compositor gives us the whole usable area;
            // we center the card ourselves (avoids compositor-specific
            // layer-height clamping observed in sway headless)
            anchors: ibde_ui::session::Anchor::Top
                | ibde_ui::session::Anchor::Bottom
                | ibde_ui::session::Anchor::Left
                | ibde_ui::session::Anchor::Right,
            width: 0,
            height: 0,
            exclusive_zone: -1,
            margins: (150, 390, 150, 290),
            keyboard_interactive: true,
        };
        match session.create_layer(&spec, &qh) {
            Some((id, surface, ls)) => {
                self.spotlight = Some(Module {
                    id,
                    surface,
                    ls: Some(ls),
                    kit: None,
                });
            }
            None => eprintln!("[shell] spotlight layer unavailable"),
        }
    }

    fn close_spotlight(&mut self) {
        if let Some(m) = self.spotlight.take() {
            if let Some(ls) = &m.ls {
                ls.destroy();
            }
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
        self.spot_query.clear();
        self.spot_sel = 0;
    }

    fn launch_spotlight(&mut self) {
        let Some(&app_i) = self.spot_matches.get(self.spot_sel) else {
            return;
        };
        let entry = &self.spot_apps[app_i];
        // strip freedesktop field codes (%f %F %u %U %i %c %k)
        let cmd = entry
            .exec
            .split_whitespace()
            .find(|t| !t.starts_with('%'))
            .unwrap_or("")
            .to_string();
        eprintln!("[shell] spotlight launching: {cmd}");
        match std::process::Command::new(&cmd)
            .env(
                "WAYLAND_DISPLAY",
                std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            )
            .spawn()
        {
            Ok(child) => std::mem::forget(child),
            Err(e) => eprintln!("[shell] spotlight launch failed: {e}"),
        }
        self.close_spotlight();
    }

    fn draw_menu(&mut self) {
        let Some(id) = self.menu.as_ref().map(|m| m.id) else {
            return;
        };
        let hovered = self.menu_hover;
        self.render_module(id, move |cr, w, h| draw_menu_content(cr, w, h, hovered));
    }

    fn open_menu(&mut self) {
        if self.menu.is_some() {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(qh) = self.qh.clone() else { return };
        // popover below the IBDE pill (top-left), above everything else
        let spec = ibde_ui::session::LayerSpec {
            namespace: "ibde-menu".into(),
            layer: ibde_ui::session::Layer::Top,
            anchors: ibde_ui::session::Anchor::Top | ibde_ui::session::Anchor::Left,
            width: MENU_W as u32,
            height: MENU_H as u32,
            exclusive_zone: 0,
            margins: (8, 0, BAR_H + 4, 0),
            keyboard_interactive: false,
        };
        match session.create_layer(&spec, &qh) {
            Some((id, surface, ls)) => {
                self.menu = Some(Module {
                    id,
                    surface,
                    ls: Some(ls),
                    kit: None,
                });
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
        let Some(shm) = session.shm.clone() else {
            return;
        };
        let Some(compositor) = session.compositor.clone() else {
            return;
        };
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

    fn show_toast(&mut self, t: Toast) {
        if self.toast.is_none() {
            let Some(session) = self.session.as_mut() else {
                return;
            };
            let Some(qh) = self.qh.clone() else { return };
            let spec = ibde_ui::session::LayerSpec {
                namespace: "ibde-toast".into(),
                layer: ibde_ui::session::Layer::Overlay,
                anchors: ibde_ui::session::Anchor::Top | ibde_ui::session::Anchor::Right,
                width: 340,
                height: 84,
                exclusive_zone: 0,
                margins: (0, 12, 12, 0),
                keyboard_interactive: false,
            };
            match session.create_layer(&spec, &qh) {
                Some((id, surface, ls)) => {
                    self.toast = Some(Module {
                        id,
                        surface,
                        ls: Some(ls),
                        kit: None,
                    });
                }
                None => {
                    eprintln!("[shell] toast layer unavailable");
                    return;
                }
            }
        }
        self.toast_data = Some(t);
        self.draw_toast();
    }

    fn close_toast(&mut self) {
        self.toast_data = None;
        if let Some(m) = self.toast.take() {
            if let Some(ls) = &m.ls {
                ls.destroy();
            }
            if let Some(conn) = &self.conn {
                let _ = conn.flush();
            }
        }
    }

    fn draw_toast(&mut self) {
        let Some(id) = self.toast.as_ref().map(|m| m.id) else {
            return;
        };
        let Some(t) = self.toast_data.as_ref() else {
            return;
        };
        let (app, summary, body) = (t.app.clone(), t.summary.clone(), t.body.clone());
        self.render_module(id, move |cr, w, h| {
            draw_toast_content(cr, w, h, &app, &summary, &body)
        });
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
        eprintln!("[shell] configure {id} {w}x{h}");
        let shm = self
            .session
            .as_ref()
            .and_then(|s| s.shm.clone())
            .expect("configure without shm");
        let qh = self.qh.clone().expect("configure without qh");
        let is_bar = self.bar.as_ref().is_some_and(|b| b.id == id);
        let is_menu = self.menu.as_ref().is_some_and(|m| m.id == id);
        let is_spot = self.spotlight.as_ref().is_some_and(|m| m.id == id);
        let is_toast = self.toast.as_ref().is_some_and(|m| m.id == id);
        let is_known = is_bar
            || is_menu
            || is_spot
            || is_toast
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
        } else if is_spot {
            self.draw_spotlight();
        } else if is_toast {
            self.draw_toast();
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

    fn on_keymap(&mut self, _format: u32, fd: std::os::fd::OwnedFd, size: u32) {
        match ibde_ui::input::Keyboard::from_fd(fd, size as usize) {
            Some(k) => self.kbd = Some(k),
            None => eprintln!("[shell] keymap rejected"),
        }
    }

    fn on_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        if let Some(k) = self.kbd.as_mut() {
            k.update_mask(depressed, latched, locked, group);
        }
    }

    fn on_key(&mut self, keycode: u32, pressed: bool) {
        if !pressed {
            return;
        }
        let Some(k) = self.kbd.as_ref() else { return };
        const KEY_SPACE: u32 = 57;
        const KEY_ESC: u32 = 1;
        const KEY_ENTER: u32 = 28;
        const KEY_UP: u32 = 103;
        const KEY_DOWN: u32 = 108;
        const KEY_BACKSPACE: u32 = 14;

        // Ctrl+Space toggles spotlight (Linux default binding, UX-SPEC §4)
        if keycode == KEY_SPACE && k.ctrl_held() {
            self.open_spotlight();
            return;
        }
        if self.spotlight.is_none() {
            return;
        }
        match keycode {
            KEY_ESC => self.close_spotlight(),
            KEY_ENTER => self.launch_spotlight(),
            KEY_UP => {
                self.spot_sel = self.spot_sel.saturating_sub(1);
                self.draw_spotlight();
            }
            KEY_DOWN => {
                if self.spot_sel + 1 < self.spot_matches.len().min(SPOT_MAX_ROWS) {
                    self.spot_sel += 1;
                }
                self.draw_spotlight();
            }
            KEY_BACKSPACE => {
                self.spot_query.pop();
                self.update_spot_matches();
                self.draw_spotlight();
            }
            _ => {
                let ev = k.translate(keycode + 8);
                if !ev.utf8.is_empty() && ev.utf8.chars().all(|c| !c.is_control()) {
                    self.spot_query.push_str(&ev.utf8);
                    self.update_spot_matches();
                    self.draw_spotlight();
                }
            }
        }
    }

    fn on_pointer_enter(&mut self, surface: &WlSurface, serial: u32, sx: f64, sy: f64) {
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

/// Spotlight layout.
const SPOT_W: i32 = 480;
const SPOT_H: i32 = 258;
const SPOT_ROW_H: f64 = 30.0;
const SPOT_MAX_ROWS: usize = 7;

/// Subsequence fuzzy score; None = no match. Higher is better.
fn fuzzy_score(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let mut score = 0i32;
    let mut qi = query.chars().peekable();
    let mut prev_match = false;
    let mut matched_any = false;
    for c in name.chars() {
        if qi.peek() == Some(&c) {
            qi.next();
            score += if prev_match { 3 } else { 1 };
            if name.starts_with(query) {
                score += 5; // prefix bonus (checked once is enough)
            }
            prev_match = true;
            matched_any = true;
        } else {
            prev_match = false;
        }
    }
    if qi.peek().is_some() {
        return None;
    }
    matched_any.then_some(score)
}

fn draw_toast_content(
    cr: &cairo::Context,
    width: i32,
    height: i32,
    app: &str,
    summary: &str,
    body: &str,
) {
    let w = width as f64;
    let h = height as f64;
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    canvas::fill_rounded_rect(cr, 0.0, 0.0, w, h, 12.0, (1.0, 1.0, 1.0, 0.97));
    cr.set_source_rgba(0.85, 0.85, 0.87, 1.0);
    cr.set_line_width(1.0);
    cr.new_path();
    cr.arc(
        12.0,
        12.0,
        11.5,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.arc(w - 12.0, 12.0, 11.5, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(w - 12.0, h - 12.0, 11.5, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(
        12.0,
        h - 12.0,
        11.5,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.close_path();
    let _ = cr.stroke();

    let small = pango::FontDescription::from_string("Sans 9");
    let semi = {
        let mut f = pango::FontDescription::from_string("Sans 12");
        f.set_weight(pango::Weight::Semibold);
        f
    };
    let body_font = pango::FontDescription::from_string("Sans 11");

    canvas::draw_label(cr, app, &small, 14.0, 8.0, theme::TEXT_MUTED);
    let (_sw, sh) = canvas::label_size(cr, summary, &semi);
    canvas::draw_label(
        cr,
        summary,
        &semi,
        14.0,
        24.0 + (18.0 - sh as f64) / 2.0,
        theme::TEXT,
    );
    // body: single line, truncated to card width (wrap comes with the
    // widget tree)
    let (_bw, bh) = canvas::label_size(cr, body, &body_font);
    canvas::draw_label(
        cr,
        body,
        &body_font,
        14.0,
        46.0 + (16.0 - bh as f64) / 2.0,
        theme::TEXT_MUTED,
    );
}

fn draw_spotlight_content(
    cr: &cairo::Context,
    width: i32,
    height: i32,
    query: &str,
    rows: &[String],
    sel: usize,
) {
    let w = width as f64;
    let h = height as f64;
    cr.set_operator(cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(cairo::Operator::Over);

    // card centered inside the (full-anchor) surface
    let cw = SPOT_W as f64;
    let ch = SPOT_H as f64;
    let cx = (w - cw) / 2.0;
    let cy = (h - ch) / 2.0;
    canvas::fill_rounded_rect(cr, cx, cy, cw, ch, 14.0, (1.0, 1.0, 1.0, 0.98));
    cr.set_source_rgba(0.85, 0.85, 0.87, 1.0);
    cr.set_line_width(1.0);
    cr.new_path();
    cr.arc(
        cx + 14.0,
        cy + 14.0,
        13.5,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.arc(
        cx + cw - 14.0,
        cy + 14.0,
        13.5,
        -0.5 * std::f64::consts::PI,
        0.0,
    );
    cr.arc(
        cx + cw - 14.0,
        cy + ch - 14.0,
        13.5,
        0.0,
        0.5 * std::f64::consts::PI,
    );
    cr.arc(
        cx + 14.0,
        cy + ch - 14.0,
        13.5,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.close_path();
    let _ = cr.stroke();

    let font = pango::FontDescription::from_string("Sans 15");
    let row_font = pango::FontDescription::from_string("Sans 12");
    let hint_font = pango::FontDescription::from_string("Sans 9");

    // search line
    let shown = if query.is_empty() {
        "Application Search"
    } else {
        query
    };
    let color = if query.is_empty() {
        theme::TEXT_MUTED
    } else {
        theme::TEXT
    };
    let (_tw, th) = canvas::label_size(cr, shown, &font);
    canvas::draw_label(
        cr,
        shown,
        &font,
        cx + 20.0,
        cy + 20.0 + (30.0 - th as f64) / 2.0,
        color,
    );
    cr.set_source_rgba(0.88, 0.88, 0.90, 1.0);
    cr.rectangle(cx + 12.0, cy + 58.5, cw - 24.0, 1.0);
    let _ = cr.fill();

    // result rows
    for (i, name) in rows.iter().take(SPOT_MAX_ROWS).enumerate() {
        let y = cy + 64.0 + i as f64 * SPOT_ROW_H;
        if i == sel {
            canvas::fill_rounded_rect(
                cr,
                cx + 10.0,
                y,
                cw - 20.0,
                SPOT_ROW_H - 4.0,
                7.0,
                theme::ACCENT,
            );
        }
        let (_tw, rh) = canvas::label_size(cr, name, &row_font);
        let color = if i == sel {
            theme::ON_ACCENT
        } else {
            theme::TEXT
        };
        canvas::draw_label(
            cr,
            name,
            &row_font,
            cx + 22.0,
            y + (SPOT_ROW_H - 4.0 - rh as f64) / 2.0,
            color,
        );
    }

    // footer
    let hint = "up-down choose - enter open - esc close";
    let (_hw, hh) = canvas::label_size(cr, hint, &hint_font);
    canvas::draw_label(
        cr,
        hint,
        &hint_font,
        cx + (cw - _hw as f64) / 2.0,
        cy + ch - 22.0 + (14.0 - hh as f64) / 2.0,
        theme::TEXT_MUTED,
    );
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
    cr.arc(
        10.0,
        10.0,
        9.5,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.arc(w - 10.0, 10.0, 9.5, -0.5 * std::f64::consts::PI, 0.0);
    cr.arc(w - 10.0, h - 10.0, 9.5, 0.0, 0.5 * std::f64::consts::PI);
    cr.arc(
        10.0,
        h - 10.0,
        9.5,
        0.5 * std::f64::consts::PI,
        std::f64::consts::PI,
    );
    cr.close_path();
    let _ = cr.stroke();

    let font = pango::FontDescription::from_string("Sans 11");
    // header
    let font_semibold = {
        let mut f = pango::FontDescription::from_string("Sans 11");
        f.set_weight(pango::Weight::Semibold);
        f
    };
    canvas::draw_label(cr, "IBDE", &font_semibold, 14.0, 7.0, theme::TEXT);
    cr.set_source_rgba(0.88, 0.88, 0.90, 1.0);
    cr.rectangle(8.0, 23.5, w - 16.0, 1.0);
    let _ = cr.fill();

    for (i, row) in MENU_ROWS.iter().enumerate() {
        let y = 24.0 + i as f64 * MENU_ROW_H;
        if hovered == Some(i) {
            canvas::fill_rounded_rect(
                cr,
                6.0,
                y + 2.0,
                w - 12.0,
                MENU_ROW_H - 6.0,
                6.0,
                (0.90, 0.91, 0.94, 1.0),
            );
        }
        let (_tw, th) = canvas::label_size(cr, row, &font);
        canvas::draw_label(
            cr,
            row,
            &font,
            16.0,
            y + (MENU_ROW_H - th as f64) / 2.0,
            theme::TEXT,
        );
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

fn app_shell_show_toast(app: &mut App, t: Toast) {
    app.show_toast(t);
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

    // notifications: serve org.freedesktop.Notifications on the session
    // bus; toasts render as layer surfaces and auto-expire in the loop
    let (ntx, nrx) = std::sync::mpsc::channel::<NotifyCmd>();
    {
        let ntx = ntx.clone();
        std::thread::spawn(move || {
            match zbus::blocking::Connection::session() {
                Ok(conn) => {
                    let iface = NotificationsIface {
                        tx: ntx,
                        next_id: std::sync::atomic::AtomicU32::new(1),
                    };
                    match conn
                        .object_server()
                        .at("/org/freedesktop/Notifications", iface)
                    {
                        Ok(_) => {}
                        Err(e) => eprintln!("[shell] notif object-server error: {e}"),
                    }
                    if let Err(e) = conn.request_name("org.freedesktop.Notifications") {
                        eprintln!("[shell] notif name error: {e}");
                    }
                    eprintln!("[shell] notifications serving on the session bus");
                    // block this thread serving requests
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(3600));
                    }
                }
                Err(e) => eprintln!("[shell] no session bus ({e}); notifications off"),
            }
        });
    }

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
        // notifications: drain commands, expire toasts
        loop {
            match nrx.try_recv() {
                Ok(NotifyCmd::Show {
                    id: _,
                    app: app_name,
                    summary,
                    body,
                    timeout_ms,
                }) => {
                    let t = Toast {
                        app: app_name,
                        summary,
                        body,
                        expires_at: Instant::now()
                            + Duration::from_millis(timeout_ms.max(500) as u64),
                    };
                    app_shell_show_toast(&mut app, t);
                }
                Ok(NotifyCmd::Close(_)) => app.close_toast(),
                Err(_) => break,
            }
        }
        if app
            .toast_data
            .as_ref()
            .is_some_and(|t| Instant::now() >= t.expires_at)
        {
            app.close_toast();
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
