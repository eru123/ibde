//! ibde-ui — the IBDE retained-mode toolkit crate (ADR-0004, Rust per
//! ADR-0012).
//!
//! Phase 1 W3 will add the widget tree, styling, animation timeline and
//! the text-grid fast path (terminal renderer). What lives here today is
//! the foundation proven by the Phase 0 spikes:
//!
//! - [`shm`] — wl_shm buffer management
//! - [`canvas`] — cairo + pango drawing surface with shm copy-out
//! - [`stats`] — RSS/FPS measurement helpers shared by bench and shell
//!
//! Design rule: no GTK, no gsettings; shared-library footprint is the
//! budget (ADR-0002), so every dependency here must earn its RSS.

pub mod atlas;
pub mod canvas;
pub mod grid;
pub mod input;
pub mod session;
pub mod shm;
pub mod stats;
