//! Cairo + pango drawing surface (software path). The shell draws here,
//! then copies the frame out to a [`crate::shm::ShmBuffer`].
//!
//! The GLES2 backend (ADR-0004) arrives in Phase 1 W3 behind the same
//! "produce an ARGB frame" contract; consumers must not depend on cairo
//! specifics beyond the exposed drawing helpers.

use crate::shm::ShmBuffer;
use pango::FontDescription;

/// An ARGB8888 in-memory canvas.
pub struct Canvas {
    surface: cairo::ImageSurface,
    pub width: i32,
    pub height: i32,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> cairo::Result<Self> {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, width, height)?;
        Ok(Self {
            surface,
            width,
            height,
        })
    }

    pub fn context(&self) -> cairo::Result<cairo::Context> {
        cairo::Context::new(&self.surface)
    }

    pub fn surface(&self) -> &cairo::ImageSurface {
        &self.surface
    }

    /// Mutable access for `data()` (cairo requires &mut to flush).
    pub fn surface_mut(&mut self) -> &mut cairo::ImageSurface {
        &mut self.surface
    }

    /// Copy the current contents into a shm buffer (full-frame copy;
    /// damage tracking lands with the widget tree in Phase 1).
    pub fn copy_to(&mut self, buf: &mut ShmBuffer) {
        let data = self.surface.data().expect("canvas data");
        buf.write_frame(&data);
    }
}

/// Measure a text label with pango (no drawing).
pub fn label_size(cr: &cairo::Context, text: &str, font: &FontDescription) -> (i32, i32) {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_font_description(Some(font));
    layout.set_text(text);
    layout.pixel_size()
}

/// Draw a text label with pango; returns the pixel size.
pub fn draw_label(
    cr: &cairo::Context,
    text: &str,
    font: &FontDescription,
    x: f64,
    y: f64,
    rgba: (f64, f64, f64, f64),
) -> (i32, i32) {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_font_description(Some(font));
    layout.set_text(text);
    let (w, h) = layout.pixel_size();
    cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    (w, h)
}

/// Fill a rounded rectangle (corner radius clamped to half-extents).
pub fn fill_rounded_rect(
    cr: &cairo::Context,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    rgba: (f64, f64, f64, f64),
) {
    use std::f64::consts::FRAC_PI_2;
    let r = radius.min(w / 2.0).min(h / 2.0);
    cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    cr.new_path();
    cr.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, FRAC_PI_2, std::f64::consts::PI);
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
    let _ = cr.fill();
}

/// Fill a rounded "pill" (fully-rounded end caps).
pub fn fill_pill(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, rgba: (f64, f64, f64, f64)) {
    let r = h / 2.0;
    let cy = y + r;
    cr.set_source_rgba(rgba.0, rgba.1, rgba.2, rgba.3);
    cr.arc(
        x + r,
        cy,
        r,
        std::f64::consts::FRAC_PI_2,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + w - r,
        cy,
        r,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
    let _ = cr.fill();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_corners_clamp_and_cover() {
        let mut c = Canvas::new(40, 40).unwrap();
        {
            let cr = c.context().unwrap();
            fill_rounded_rect(&cr, 4.0, 4.0, 32.0, 32.0, 10.0, (0.2, 0.4, 0.6, 1.0));
        }
        let d = c.surface_mut().data().unwrap();
        let px = |x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 40 * 4) + x * 4;
            (d[i + 2], d[i + 1], d[i]) // ARGB → RGB
        };
        // center covered
        assert_eq!(px(20, 20), (51, 102, 153));
        // outside the corner radius (top-left corner) untouched
        assert_eq!(px(5, 5), (0, 0, 0));
        // edge midpoint covered (straight section)
        assert_eq!(px(20, 5), (51, 102, 153));
    }

    /// Frames must be deterministic: same input state → identical bytes
    /// (foundation of the render-hash golden tests, ADR-0004).
    #[test]
    fn canvas_frames_are_deterministic() {
        let font = FontDescription::from_string("Sans 12");
        let render = |t: f64| -> Vec<u8> {
            let mut c = Canvas::new(320, 32).unwrap();
            {
                let cr = c.context().unwrap();
                cr.set_source_rgba(0.92, 0.92, 0.94, 0.88);
                cr.rectangle(0.0, 0.0, 320.0, 32.0);
                let _ = cr.fill();
                fill_pill(
                    &cr,
                    8.0,
                    4.0,
                    80.0 + t * 10.0,
                    24.0,
                    (0.35, 0.38, 0.45, 1.0),
                );
                let _ = draw_label(&cr, "determinism", &font, 100.0, 8.0, (0.1, 0.1, 0.12, 1.0));
            }
            let x = c.surface_mut().data().unwrap().to_vec();
            x
        };
        assert_eq!(render(1.0), render(1.0));
        assert_ne!(render(1.0), render(2.0));
    }
}
