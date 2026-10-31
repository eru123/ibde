//! wl_shm buffer management: one memfd-backed ARGB8888 buffer.
//!
//! Single-buffered by design here; the production shell moves to a
//! double-buffered pool when it starts tracking wl_buffer.release
//! (Phase 1 W3 — see SPIKE-REPORT "known limitations").

use std::ffi::CString;
use std::fs::File;
use std::os::unix::io::{AsFd, AsRawFd, FromRawFd};

use memmap2::MmapMut;
use wayland_client::protocol::{wl_buffer, wl_shm, wl_shm_pool};
use wayland_client::{Dispatch, QueueHandle};

/// One wl_shm ARGB8888 buffer of `width × height`.
pub struct ShmBuffer {
    _fd: File,
    mmap: MmapMut,
    _pool: wl_shm_pool::WlShmPool,
    buffer: wl_buffer::WlBuffer,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
}

impl ShmBuffer {
    /// Create a buffer on `shm`. The caller's state type `D` must noop or
    /// handle `WlShmPool`/`WlBuffer` events (`delegate_noop!` with
    /// `ignore wl_buffer::WlBuffer` covers it).
    pub fn new<D>(
        shm: &wl_shm::WlShm,
        width: i32,
        height: i32,
        qh: &QueueHandle<D>,
    ) -> Self
    where
        D: Dispatch<wl_shm_pool::WlShmPool, ()>
            + Dispatch<wl_buffer::WlBuffer, ()>
            + 'static,
    {
        let stride = width * 4;
        let size = (stride * height) as usize;
        let name = CString::new("ibde-shm").unwrap();
        // SAFETY: plain libc memfd_create; result checked immediately.
        let fd = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
        assert!(fd >= 0, "memfd_create failed");
        // SAFETY: fd owned by `file` from here; closed on drop.
        let file = unsafe { File::from_raw_fd(fd) };
        let rc = unsafe { libc::ftruncate(fd, size as libc::off_t) };
        assert!(rc == 0, "ftruncate failed");
        // SAFETY: fd is a fresh memfd of exactly `size` bytes.
        let mmap = unsafe { MmapMut::map_mut(file.as_raw_fd()) }.expect("mmap failed");
        let pool = shm.create_pool(file.as_fd(), size as i32, qh, ());
        let buffer =
            pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, qh, ());
        Self { _fd: file, mmap, _pool: pool, buffer, width, height, stride }
    }

    pub fn wayland_buffer(&self) -> &wl_buffer::WlBuffer {
        &self.buffer
    }

    /// Copy `bytes` (a full frame, ARGB8888, exactly `stride × height`)
    /// into the buffer.
    pub fn write_frame(&mut self, bytes: &[u8]) {
        assert_eq!(bytes.len(), (self.stride * self.height) as usize,
            "frame size mismatch");
        self.mmap.copy_from_slice(bytes);
    }
}
