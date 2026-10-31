//! Measurement helpers shared by the shell, terminal and ibde-bench
//! (ADR-0002 budget accounting).

use std::fs::File;
use std::io::Read;
use std::time::Instant;

/// Resident set size of this process, in KiB (includes shared pages —
/// same accounting as the Phase 0 spikes; PSS lands with bench v1).
pub fn rss_kb() -> i64 {
    let mut s = String::new();
    if File::open("/proc/self/statm")
        .and_then(|mut f| f.read_to_string(&mut s))
        .is_err()
    {
        return -1;
    }
    s.split_whitespace()
        .nth(1)
        .and_then(|r| r.parse::<i64>().ok())
        .map(|pages| pages * 4)
        .unwrap_or(-1)
}

/// Per-second FPS accounting.
pub struct FpsCounter {
    t0: Instant,
    last_report: Instant,
    frames: u32,
}

impl FpsCounter {
    pub fn new() -> Self {
        Self {
            t0: Instant::now(),
            last_report: Instant::now(),
            frames: 0,
        }
    }

    /// Record a frame. Returns `Some((fps_since_last_report, elapsed_s))`
    /// when a full second has elapsed.
    pub fn tick(&mut self) -> Option<(u32, f64)> {
        self.frames += 1;
        if self.last_report.elapsed().as_millis() >= 1000 {
            let out = (self.frames, self.t0.elapsed().as_secs_f64());
            self.frames = 0;
            self.last_report = Instant::now();
            Some(out)
        } else {
            None
        }
    }
}

impl Default for FpsCounter {
    fn default() -> Self {
        Self::new()
    }
}
