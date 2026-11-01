//! Session glue: connection + globals + layer surfaces, routed to the
//! app's state via [`SessionHost`] (ADR-0005's "shell as layer-shell
//! clients" made turnkey).
//!
//! Apps bring one trait impl and one macro invocation:
//!
//! ```ignore
//! struct App { session: Option<ibde_ui::session::Session>, /* … */ }
//! impl ibde_ui::session::SessionHost for App {
//!     fn session_slot(&mut self) -> &mut Option<Session> { &mut self.session }
//!     fn on_layer_configure(&mut self, id: u64, w: i32, h: i32) { /* size buffers */ }
//!     fn on_frame(&mut self, surface_id: u64) { /* render, commit, re-request */ }
//! }
//! ibde_ui::delegate_ibde_session!(App);
//! ```
//!
//! The macro generates the `wayland_client::Dispatch` impls in the app's
//! crate (orphan-rule compliant — same pattern as wayland-client's own
//! `delegate_noop!`). This API is a v0 draft contract; freezing it is a
//! Phase 1 exit criterion (`docs/contracts/`).

// Public re-exports: everything the delegate macro references lives
// under $crate::session so its expansion is path-hygienic.
pub use wayland_client::protocol::wl_buffer;
pub use wayland_client::protocol::wl_buffer::WlBuffer;
pub use wayland_client::protocol::wl_callback;
pub use wayland_client::protocol::wl_callback::WlCallback;
pub use wayland_client::protocol::wl_compositor;
pub use wayland_client::protocol::wl_compositor::WlCompositor;
pub use wayland_client::protocol::wl_keyboard;
pub use wayland_client::protocol::wl_keyboard::WlKeyboard;
pub use wayland_client::protocol::wl_output;
pub use wayland_client::protocol::wl_output::WlOutput;
pub use wayland_client::protocol::wl_pointer;
pub use wayland_client::protocol::wl_pointer::WlPointer;
pub use wayland_client::protocol::wl_registry;
pub use wayland_client::protocol::wl_registry::WlRegistry;
pub use wayland_client::protocol::wl_seat;
pub use wayland_client::protocol::wl_seat::WlSeat;
pub use wayland_client::protocol::wl_shm;
pub use wayland_client::protocol::wl_shm::WlShm;
pub use wayland_client::protocol::wl_shm_pool;
pub use wayland_client::protocol::wl_shm_pool::WlShmPool;
pub use wayland_client::protocol::wl_surface;
pub use wayland_client::protocol::wl_surface::WlSurface;
pub use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
pub use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
pub use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::ExtSessionLockSurfaceV1;
pub use wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::ExtSessionLockV1;
pub use wayland_protocols::ext::session_lock::v1::client::{
    ext_session_lock_manager_v1, ext_session_lock_surface_v1, ext_session_lock_v1,
};
pub use wayland_protocols::xdg::shell::client::xdg_surface;
pub use wayland_protocols::xdg::shell::client::xdg_surface::XdgSurface;
pub use wayland_protocols::xdg::shell::client::xdg_toplevel;
pub use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
pub use wayland_protocols::xdg::shell::client::xdg_wm_base;
pub use wayland_protocols::xdg::shell::client::xdg_wm_base::XdgWmBase;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::Layer;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::ZwlrLayerShellV1;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;
pub use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::ZwlrLayerSurfaceV1;

/// User-data carried by a layer surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerData {
    pub id: u64,
}

/// User-data carried by a frame callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameData {
    pub surface_id: u64,
}

/// User-data carried by an xdg window (surface + toplevel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowData {
    pub id: u64,
}

/// User-data for the registry (unit).
#[derive(Clone, Copy, Debug)]
pub struct RegistryData;

/// Everything the host app must provide; the callbacks are optional.
pub trait SessionHost {
    /// Storage for the session. `Session::begin` fills it.
    fn session_slot(&mut self) -> &mut Option<Session>;
    /// Fired once, when compositor + shm are both bound.
    fn on_ready(&mut self) {}
    /// Layer surface configured (ack already sent). Create/size buffers.
    fn on_layer_configure(&mut self, id: u64, width: i32, height: i32) {
        let _ = (id, width, height);
    }
    /// Layer surface closed by the compositor.
    fn on_layer_closed(&mut self, id: u64) {
        let _ = id;
    }
    /// xdg surface configured (ack already sent); toplevel dims arrive
    /// in `on_toplevel_configure` just before this.
    fn on_window_configure(&mut self, id: u64) {
        let _ = id;
    }
    /// Toplevel dimensions (0 = pick your own). Precedes
    /// `on_window_configure`.
    fn on_toplevel_configure(&mut self, id: u64, width: i32, height: i32) {
        let _ = (id, width, height);
    }
    /// Toplevel closed by the compositor or user.
    fn on_window_close(&mut self, id: u64) {
        let _ = id;
    }
    /// Compositor keymap (fd owned by the host; feed it to
    /// `ibde_ui::input::Keyboard::from_fd`).
    fn on_keymap(&mut self, format: u32, fd: std::os::fd::OwnedFd, size: u32) {
        let _ = (format, fd, size);
    }
    /// Keyboard focus entered one of our surfaces.
    fn on_keyboard_enter(&mut self, surface: &WlSurface) {
        let _ = surface;
    }
    /// Keyboard focus left one of our surfaces.
    fn on_keyboard_leave(&mut self, surface: &WlSurface) {
        let _ = surface;
    }
    /// Raw key event (evdev keycode; add 8 for xkb).
    fn on_key(&mut self, keycode: u32, pressed: bool) {
        let _ = (keycode, pressed);
    }
    /// wl_keyboard.modifiers payload.
    fn on_modifiers(&mut self, depressed: u32, latched: u32, locked: u32, group: u32) {
        let _ = (depressed, latched, locked, group);
    }
    /// Pointer entered one of our surfaces (surface-local coords).
    fn on_pointer_enter(&mut self, surface: &WlSurface, serial: u32, sx: f64, sy: f64) {
        let _ = (surface, serial, sx, sy);
    }
    /// Pointer left one of our surfaces.
    fn on_pointer_leave(&mut self, surface: &WlSurface) {
        let _ = surface;
    }
    /// Pointer motion (surface-local coords of the entered surface).
    fn on_pointer_motion(&mut self, sx: f64, sy: f64) {
        let _ = (sx, sy);
    }
    /// Pointer button (evdev code; e.g. BTN_LEFT = 0x110).
    fn on_pointer_button(&mut self, button: u32, pressed: bool) {
        let _ = (button, pressed);
    }
    /// The session lock is engaged: all outputs will get lock surfaces.
    fn on_lock_locked(&mut self) {}
    /// A lock surface was configured (ack it, then attach + commit).
    fn on_lock_surface_configure(
        &mut self,
        surface: &ExtSessionLockSurfaceV1,
        serial: u32,
        width: i32,
        height: i32,
    ) {
        let _ = (surface, serial, width, height);
    }
    /// The compositor finished/unlocked the session lock.
    fn on_lock_finished(&mut self) {}
    /// Frame callback for a surface: render, commit, re-request.
    fn on_frame(&mut self, surface_id: u64) {
        let _ = surface_id;
    }
}

/// Bound globals + surface-id allocator.
pub struct Session {
    pub compositor: Option<WlCompositor>,
    pub shm: Option<WlShm>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub xdg_wm_base: Option<XdgWmBase>,
    pub seat: Option<WlSeat>,
    /// ext-session-lock manager (lock screen infra).
    pub lock_manager: Option<ExtSessionLockManagerV1>,
    /// first bound output (single-head v0; lock surfaces are per-output).
    pub output: Option<WlOutput>,
    /// Bound when the seat advertises Pointer — hosts use it for
    /// set_cursor during on_pointer_enter.
    pub pointer: Option<WlPointer>,
    next_id: u64,
}

impl Session {
    /// Create the session slot and the registry. Follow with one
    /// `queue.roundtrip(&mut app)` — globals land during it.
    pub fn begin<D>(state: &mut D, conn: &Connection, qh: &QueueHandle<D>)
    where
        D: SessionHost
            + Dispatch<WlRegistry, RegistryData>
            + Dispatch<WlCompositor, ()>
            + Dispatch<WlShm, ()>
            + Dispatch<ZwlrLayerShellV1, ()>
            + Dispatch<XdgWmBase, ()>
            + Dispatch<WlSeat, ()>
            + Dispatch<ExtSessionLockManagerV1, ()>
            + Dispatch<WlOutput, ()>
            + 'static,
    {
        *state.session_slot() = Some(Session {
            compositor: None,
            shm: None,
            layer_shell: None,
            xdg_wm_base: None,
            seat: None,
            lock_manager: None,
            output: None,
            pointer: None,
            next_id: 0,
        });
        let _registry = conn.display().get_registry(qh, RegistryData);
    }

    pub fn next_surface_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn ready(&self) -> bool {
        self.compositor.is_some() && self.shm.is_some()
    }

    // NOTE: there is deliberately no manual `keyboard()` binder. The
    // delegate macro binds `wl_keyboard` when (and only when) the seat
    // advertises the Keyboard capability — requesting a keyboard from a
    // seat that lacks one is a protocol error (wl_seat
    // `missing_capability`) that kills the connection.

    // Create a layer surface per `spec` and commit it; the configure
    // arrives asynchronously in `on_layer_configure`.
    pub fn create_layer<D>(
        &mut self,
        spec: &LayerSpec,
        qh: &QueueHandle<D>,
    ) -> Option<(u64, WlSurface, ZwlrLayerSurfaceV1)>
    where
        D: Dispatch<WlSurface, ()>
            + Dispatch<WlOutput, ()>
            + Dispatch<ZwlrLayerSurfaceV1, LayerData>
            + 'static,
    {
        let _id = self.next_surface_id();
        let compositor = self.compositor.as_ref()?;
        let shell = self.layer_shell.as_ref()?;
        let surface = compositor.create_surface(qh, ());
        let ls = shell.get_layer_surface(
            &surface,
            None::<&WlOutput>,
            spec.layer,
            spec.namespace.clone(),
            qh,
            LayerData { id: _id },
        );
        ls.set_anchor(spec.anchors);
        ls.set_size(spec.width, spec.height);
        ls.set_margin(
            spec.margins.0,
            spec.margins.1,
            spec.margins.2,
            spec.margins.3,
        );
        if spec.exclusive_zone != 0 {
            ls.set_exclusive_zone(spec.exclusive_zone);
        }
        if spec.keyboard_interactive {
            ls.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
        }
        surface.commit();
        Some((_id, surface, ls))
    }

    /// Create an xdg toplevel window per `spec` and commit it; the
    /// toplevel configure (dims) and xdg configure (ack + ready)
    /// arrive asynchronously.
    pub fn create_toplevel<D>(
        &mut self,
        spec: &WindowSpec,
        qh: &QueueHandle<D>,
    ) -> Option<(u64, WlSurface, XdgSurface, XdgToplevel)>
    where
        D: Dispatch<WlSurface, ()>
            + Dispatch<XdgSurface, WindowData>
            + Dispatch<XdgToplevel, WindowData>
            + 'static,
    {
        let id = self.next_surface_id();
        let compositor = self.compositor.as_ref()?;
        let wm = self.xdg_wm_base.as_ref()?;
        let surface = compositor.create_surface(qh, ());
        let xds = wm.get_xdg_surface(&surface, qh, WindowData { id });
        let tl = xds.get_toplevel(qh, WindowData { id });
        tl.set_title(spec.title.clone());
        tl.set_app_id(spec.app_id.clone());
        surface.commit();
        Some((id, surface, xds, tl))
    }
}

/// Declarative xdg-window specification.
pub struct WindowSpec {
    pub title: String,
    /// e.g. "ibde-terminal" — identifies the app to the compositor
    /// (taskbars, switchers, app menus).
    pub app_id: String,
    /// Preferred size in px; None lets the app choose on configure.
    pub size: Option<(i32, i32)>,
}

impl WindowSpec {
    pub fn titled(title: &str) -> Self {
        WindowSpec {
            title: title.to_string(),
            app_id: "ibde.app".to_string(),
            size: None,
        }
    }

    pub fn app(title: &str, app_id: &str) -> Self {
        WindowSpec {
            title: title.to_string(),
            app_id: app_id.to_string(),
            size: None,
        }
    }
}

/// Declarative layer-surface specification.
pub struct LayerSpec {
    pub namespace: String,
    pub layer: Layer,
    pub anchors: Anchor,
    pub width: u32,
    pub height: u32,
    pub exclusive_zone: i32,
    /// (left, right, top, bottom) margins from the anchored edges.
    pub margins: (i32, i32, i32, i32),
    /// Request exclusive keyboard focus (popovers, spotlight).
    pub keyboard_interactive: bool,
}

impl LayerSpec {
    /// Full-width top bar of `height` px (menubar-style). The bar takes
    /// keyboard focus when no app window has it — the shell owns global
    /// shortcuts (Ctrl+Space) until an app is focused (v0 approximation;
    /// compositor-level bindings are the ADR-0008 endgame).
    pub fn top_bar(namespace: &str, height: i32) -> Self {
        LayerSpec {
            namespace: namespace.to_string(),
            layer: Layer::Top,
            anchors: Anchor::Top | Anchor::Left | Anchor::Right,
            width: 0, // derived from anchors
            height: height as u32,
            exclusive_zone: height,
            margins: (0, 0, 0, 0),
            keyboard_interactive: true,
        }
    }

    /// Floating panel centered on the bottom edge (dock-style):
    /// explicit size, bottom margin, no exclusive zone (overlay).
    pub fn bottom_dock(namespace: &str, width: i32, height: i32, gap: i32) -> Self {
        LayerSpec {
            namespace: namespace.to_string(),
            layer: Layer::Top,
            anchors: Anchor::Bottom,
            width: width as u32,
            height: height as u32,
            exclusive_zone: 0,
            margins: (0, 0, 0, gap),
            keyboard_interactive: false,
        }
    }
}

/// Request the next frame callback for a surface.
pub fn request_frame<D>(surface: &WlSurface, surface_id: u64, qh: &QueueHandle<D>)
where
    D: Dispatch<WlCallback, FrameData> + 'static,
{
    let _cb = surface.frame(qh, FrameData { surface_id });
}

/// Registry-event handler used by the delegate macro.
pub fn handle_registry_global<D>(
    state: &mut D,
    registry: &WlRegistry,
    name: u32,
    interface: String,
    version: u32,
    qh: &QueueHandle<D>,
) where
    D: SessionHost
        + Dispatch<WlCompositor, ()>
        + Dispatch<WlShm, ()>
        + Dispatch<ZwlrLayerShellV1, ()>
        + Dispatch<XdgWmBase, ()>
        + Dispatch<WlSeat, ()>
        + Dispatch<ExtSessionLockManagerV1, ()>
        + Dispatch<WlOutput, ()>
        + 'static,
{
    let was_ready = state.session_slot().as_ref().is_some_and(|s| s.ready());
    if let Some(session) = state.session_slot().as_mut() {
        match interface.as_str() {
            "wl_compositor" => {
                session.compositor = Some(registry.bind(name, version.min(4), qh, ()))
            }
            "wl_shm" => session.shm = Some(registry.bind(name, version.min(1), qh, ())),
            "zwlr_layer_shell_v1" => {
                session.layer_shell = Some(registry.bind(name, version.min(4), qh, ()))
            }
            "xdg_wm_base" => {
                session.xdg_wm_base = Some(registry.bind(name, version.min(5), qh, ()))
            }
            "wl_seat" => session.seat = Some(registry.bind(name, version.min(4), qh, ())),
            "wl_output" => {
                if session.output.is_none() {
                    session.output = Some(registry.bind(name, version.min(1), qh, ()))
                }
            }
            "ext_session_lock_manager_v1" => {
                session.lock_manager = Some(registry.bind(name, version.min(1), qh, ()))
            }
            _ => {}
        }
    }
    if !was_ready && state.session_slot().as_ref().is_some_and(|s| s.ready()) {
        state.on_ready();
    }
}

/// Generate the `wayland_client::Dispatch` impls that route protocol
/// events into your [`SessionHost`] implementation. Invoke in the crate
/// that defines `$app`.
#[macro_export]
macro_rules! delegate_ibde_session {
    ($app:ty) => {
        impl $crate::session::Dispatch<$crate::session::WlRegistry, $crate::session::RegistryData>
            for $app
        {
            fn event(
                state: &mut Self,
                registry: &$crate::session::WlRegistry,
                event: $crate::session::wl_registry::Event,
                _data: &$crate::session::RegistryData,
                _conn: &$crate::session::Connection,
                qh: &$crate::session::QueueHandle<Self>,
            ) {
                if let $crate::session::wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } = event
                {
                    $crate::session::handle_registry_global::<$app>(
                        state, registry, name, interface, version, qh,
                    );
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlCompositor, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlCompositor,
                _: $crate::session::wl_compositor::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlShm, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlShm,
                _: $crate::session::wl_shm::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
                // wl_shm format events: ignored by design
            }
        }

        impl $crate::session::Dispatch<$crate::session::ZwlrLayerShellV1, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::ZwlrLayerShellV1,
                _: $crate::session::zwlr_layer_shell_v1::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
            }
        }

        impl
            $crate::session::Dispatch<
                $crate::session::ZwlrLayerSurfaceV1,
                $crate::session::LayerData,
            > for $app
        {
            fn event(
                state: &mut Self,
                ls: &$crate::session::ZwlrLayerSurfaceV1,
                event: $crate::session::zwlr_layer_surface_v1::Event,
                data: &$crate::session::LayerData,
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                match event {
                    $crate::session::zwlr_layer_surface_v1::Event::Configure {
                        serial,
                        width,
                        height,
                    } => {
                        ls.ack_configure(serial);
                        state.on_layer_configure(data.id, width as i32, height as i32);
                    }
                    $crate::session::zwlr_layer_surface_v1::Event::Closed => {
                        state.on_layer_closed(data.id);
                    }
                    _ => {}
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlCallback, $crate::session::FrameData>
            for $app
        {
            fn event(
                state: &mut Self,
                _cb: &$crate::session::WlCallback,
                _ev: $crate::session::wl_callback::Event,
                data: &$crate::session::FrameData,
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                state.on_frame(data.surface_id);
            }
        }

        impl $crate::session::Dispatch<$crate::session::XdgWmBase, ()> for $app {
            fn event(
                state: &mut Self,
                wm: &$crate::session::XdgWmBase,
                event: $crate::session::xdg_wm_base::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                if let $crate::session::xdg_wm_base::Event::Ping { serial } = event {
                    wm.pong(serial);
                    let _ = state;
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::XdgSurface, $crate::session::WindowData>
            for $app
        {
            fn event(
                state: &mut Self,
                xds: &$crate::session::XdgSurface,
                event: $crate::session::xdg_surface::Event,
                data: &$crate::session::WindowData,
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                if let $crate::session::xdg_surface::Event::Configure { serial } = event {
                    xds.ack_configure(serial);
                    state.on_window_configure(data.id);
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::XdgToplevel, $crate::session::WindowData>
            for $app
        {
            fn event(
                state: &mut Self,
                _tl: &$crate::session::XdgToplevel,
                event: $crate::session::xdg_toplevel::Event,
                data: &$crate::session::WindowData,
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                match event {
                    $crate::session::xdg_toplevel::Event::Configure { width, height, .. } => {
                        state.on_toplevel_configure(data.id, width, height);
                    }
                    $crate::session::xdg_toplevel::Event::Close => {
                        state.on_window_close(data.id);
                    }
                    _ => {}
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlSeat, ()> for $app {
            fn event(
                state: &mut Self,
                seat: &$crate::session::WlSeat,
                event: $crate::session::wl_seat::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                qh: &$crate::session::QueueHandle<Self>,
            ) {
                if let $crate::session::wl_seat::Event::Capabilities { capabilities } = event {
                    use $crate::session::wl_seat::Capability;
                    if let $crate::session::WEnum::Value(caps) = capabilities {
                        if caps.contains(Capability::Keyboard) {
                            let _ = seat.get_keyboard(qh, ());
                        }
                        if caps.contains(Capability::Pointer) {
                            if let Some(session) = state.session_slot().as_mut() {
                                session.pointer = Some(seat.get_pointer(qh, ()));
                            }
                        }
                    }
                }
                let _ = state;
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlKeyboard, ()> for $app {
            fn event(
                state: &mut Self,
                _kb: &$crate::session::WlKeyboard,
                event: $crate::session::wl_keyboard::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                use $crate::session::wl_keyboard::{KeyState, KeymapFormat};
                match event {
                    $crate::session::wl_keyboard::Event::Keymap { format, fd, size } => {
                        if matches!(format, $crate::session::WEnum::Value(KeymapFormat::XkbV1)) {
                            // fd arrives as an OwnedFd — we take ownership
                            state.on_keymap(1, fd, size);
                        }
                    }
                    $crate::session::wl_keyboard::Event::Enter { surface, .. } => {
                        state.on_keyboard_enter(&surface);
                    }
                    $crate::session::wl_keyboard::Event::Leave { surface, .. } => {
                        state.on_keyboard_leave(&surface);
                    }
                    $crate::session::wl_keyboard::Event::Key {
                        key,
                        state: keystate,
                        ..
                    } => {
                        let pressed =
                            matches!(keystate, $crate::session::WEnum::Value(KeyState::Pressed));
                        state.on_key(key, pressed);
                    }
                    $crate::session::wl_keyboard::Event::Modifiers {
                        mods_depressed,
                        mods_latched,
                        mods_locked,
                        group,
                        ..
                    } => {
                        state.on_modifiers(mods_depressed, mods_latched, mods_locked, group);
                    }
                    _ => {}
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlPointer, ()> for $app {
            fn event(
                state: &mut Self,
                _p: &$crate::session::WlPointer,
                event: $crate::session::wl_pointer::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                use $crate::session::wl_pointer::{ButtonState, Event};
                match event {
                    Event::Enter {
                        surface,
                        serial,
                        surface_x,
                        surface_y,
                        ..
                    } => {
                        state.on_pointer_enter(&surface, serial, surface_x, surface_y);
                    }
                    Event::Leave { surface, .. } => {
                        state.on_pointer_leave(&surface);
                    }
                    Event::Motion {
                        surface_x,
                        surface_y,
                        ..
                    } => {
                        state.on_pointer_motion(surface_x, surface_y);
                    }
                    Event::Button {
                        button, state: bs, ..
                    } => {
                        let pressed =
                            matches!(bs, $crate::session::WEnum::Value(ButtonState::Pressed));
                        state.on_pointer_button(button, pressed);
                    }
                    _ => {}
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::ExtSessionLockManagerV1, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::ExtSessionLockManagerV1,
                _: $crate::session::ext_session_lock_manager_v1::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
            }
        }

        impl $crate::session::Dispatch<$crate::session::ExtSessionLockV1, ()> for $app {
            fn event(
                state: &mut Self,
                _lock: &$crate::session::ExtSessionLockV1,
                event: $crate::session::ext_session_lock_v1::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                match event {
                    $crate::session::ext_session_lock_v1::Event::Locked => {
                        state.on_lock_locked();
                    }
                    $crate::session::ext_session_lock_v1::Event::Finished => {
                        state.on_lock_finished();
                    }
                    _ => {}
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::ExtSessionLockSurfaceV1, ()> for $app {
            fn event(
                state: &mut Self,
                surface: &$crate::session::ExtSessionLockSurfaceV1,
                event: $crate::session::ext_session_lock_surface_v1::Event,
                _data: &(),
                _conn: &$crate::session::Connection,
                _qh: &$crate::session::QueueHandle<Self>,
            ) {
                if let $crate::session::ext_session_lock_surface_v1::Event::Configure {
                    serial,
                    width,
                    height,
                } = event
                {
                    surface.ack_configure(serial);
                    state.on_lock_surface_configure(surface, serial, width as i32, height as i32);
                }
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlShmPool, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlShmPool,
                _: $crate::session::wl_shm_pool::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlBuffer, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlBuffer,
                _: $crate::session::wl_buffer::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
                // buffer release: tracked when the shell goes double-buffered
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlSurface, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlSurface,
                _: $crate::session::wl_surface::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
                // enter/leave/preferred-scale: handled with output support
            }
        }

        impl $crate::session::Dispatch<$crate::session::WlOutput, ()> for $app {
            fn event(
                _: &mut Self,
                _: &$crate::session::WlOutput,
                _: $crate::session::wl_output::Event,
                _: &(),
                _: &$crate::session::Connection,
                _: &$crate::session::QueueHandle<Self>,
            ) {
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_bar_spec_defaults() {
        let s = LayerSpec::top_bar("bar", 32);
        assert_eq!(s.height, 32);
        assert_eq!(s.exclusive_zone, 32);
        assert_eq!(s.width, 0); // width comes from Left|Right anchors
        assert!(s
            .anchors
            .contains(Anchor::Top | Anchor::Left | Anchor::Right));
        assert!(matches!(s.layer, Layer::Top));
    }
}
