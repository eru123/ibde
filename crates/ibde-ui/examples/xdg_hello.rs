//! Minimal xdg-toplevel client on the session glue — mapping bisect.
//! Paints a solid red 400x300 window, commits once, exits after N secs.

use ibde_ui::canvas::Canvas;
use ibde_ui::session::{Connection, QueueHandle, Session, SessionHost, WindowSpec, WlSurface};
use ibde_ui::shm::ShmBuffer;

struct App {
    session: Option<Session>,
    conn: Option<Connection>,
    qh: Option<QueueHandle<Self>>,
    surface: Option<WlSurface>,
    buf: Option<ShmBuffer>,
    canvas: Option<Canvas>,
    configured: bool,
}

impl SessionHost for App {
    fn session_slot(&mut self) -> &mut Option<Session> {
        &mut self.session
    }
    fn on_window_configure(&mut self, _id: u64) {
        let shm = self.session.as_ref().and_then(|s| s.shm.clone()).unwrap();
        let qh = self.qh.clone().unwrap();
        self.buf = Some(ShmBuffer::new(&shm, 400, 300, &qh));
        self.canvas = Some(Canvas::new(400, 300).unwrap());
        {
            let c = self.canvas.as_mut().unwrap();
            let cr = c.context().unwrap();
            cr.set_source_rgb(0.8, 0.1, 0.1);
            cr.paint().unwrap();
        }
        if let Some(c) = self.canvas.as_mut() {
            c.copy_to(self.buf.as_mut().unwrap());
        }
        let sf = self.surface.as_ref().unwrap();
        sf.attach(Some(self.buf.as_ref().unwrap().wayland_buffer()), 0, 0);
        sf.damage_buffer(0, 0, 400, 300);
        sf.commit();
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
        self.configured = true;
    }
}

ibde_ui::delegate_ibde_session!(App);

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let conn = Connection::connect_to_env().expect("no compositor");
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let mut app = App {
        session: None,
        conn: Some(conn.clone()),
        qh: Some(qh.clone()),
        surface: None,
        buf: None,
        canvas: None,
        configured: false,
    };

    Session::begin(&mut app, &conn, &qh);
    queue.roundtrip(&mut app).expect("roundtrip");
    assert!(app.session.as_ref().is_some_and(|s| s.ready()));

    let spec = WindowSpec::app("xdg-hello", "ibde.xdg-hello");
    let (_id, surface, _xds, _tl) = app
        .session
        .as_mut()
        .unwrap()
        .create_toplevel(&spec, &qh)
        .expect("no xdg");
    app.surface = Some(surface);
    let _ = conn.flush();
    queue.roundtrip(&mut app).expect("no configure");
    assert!(app.configured, "never configured");
    eprintln!("[xdg-hello] configured+committed, holding {seconds}s");

    std::thread::sleep(std::time::Duration::from_secs(seconds));
}
