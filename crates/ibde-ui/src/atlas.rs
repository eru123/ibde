//! Glyph atlas + damage-driven TextGrid renderer (software path).
//!
//! v0 strategy (per ADR-0004 "measure first, optimize second"): glyphs
//! are cached **already tinted** in a packed atlas and blitted with
//! plain `set_source_surface` — no masking, no shader. Terminal palettes
//! use few colors, so tinted entries multiply slowly; if bench v1 shows
//! atlas pressure, the white-atlas + mask-tint optimization is a
//! drop-in behind the same API.
//!
//! Atlas exhaustion currently resets the atlas and reports full damage
//! (correct, simple); LRU eviction is a later refinement.

use std::collections::HashMap;

use cairo::{Context as Cr, Format, ImageSurface};
use pango::FontDescription;

use crate::canvas::Canvas;
use crate::grid::{Attrs, Cell, DamageSnapshot, Rgb, TextGrid};

const ATLAS_SIZE: i32 = 512;

#[derive(Clone, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    bold: bool,
    italic: bool,
    fg: Rgb,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphInfo {
    /// atlas position
    pub u: i32,
    pub v: i32,
    /// ink size
    pub w: i32,
    pub h: i32,
}

pub struct GlyphAtlas {
    surface: ImageSurface,
    /// shelf-packing cursor
    shelf_x: i32,
    shelf_y: i32,
    shelf_h: i32,
    map: HashMap<GlyphKey, GlyphInfo>,
    font: FontDescription,
    scratch: ImageSurface,
    /// cell geometry (monospace): advance, line height, baseline offset
    pub cell_w: i32,
    pub cell_h: i32,
    pub ascent: i32,
    /// diagnostics
    pub glyphs_rendered: u32,
    pub atlas_resets: u32,
}

impl GlyphAtlas {
    pub fn new(font_size: i32) -> Self {
        let font = FontDescription::from_string(&format!("Monospace {font_size}"));
        let mut atlas = GlyphAtlas {
            surface: ImageSurface::create(Format::ARgb32, ATLAS_SIZE, ATLAS_SIZE)
                .expect("atlas alloc"),
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            map: HashMap::new(),
            font: font.clone(),
            scratch: ImageSurface::create(Format::ARgb32, 256, 256).expect("scratch alloc"),
            cell_w: font_size, // provisional; fixed by measure() below
            cell_h: font_size + 4,
            ascent: font_size,
            glyphs_rendered: 0,
            atlas_resets: 0,
        };
        atlas.measure_cells();
        atlas
    }

    fn measure_cells(&mut self) {
        // derive geometry from a monospace-wide reference pair
        let cr = Cr::new(&self.scratch).expect("scratch ctx");
        let layout = pangocairo::functions::create_layout(&cr);
        layout.set_font_description(Some(&self.font));
        layout.set_text("MM");
        let (_ink, logical) = layout.pixel_extents();
        self.cell_w = (logical.width() + 1) / 2;
        {
            let pctx = layout.context();
            let metrics = pctx.metrics(Some(&self.font), None);
            let ascent = metrics.ascent() / pango::SCALE;
            let descent = metrics.descent() / pango::SCALE;
            self.ascent = ascent;
            self.cell_h = ascent + descent + 1;
        }
    }

    fn reset(&mut self) {
        let cr = Cr::new(&self.surface).expect("atlas ctx");
        cr.set_operator(cairo::Operator::Clear);
        let _ = cr.paint();
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_h = 0;
        self.map.clear();
        self.atlas_resets += 1;
    }

    /// Fetch (rendering on miss) the tinted glyph for a cell.
    fn glyph(&mut self, cell: &Cell) -> GlyphInfo {
        let key = GlyphKey {
            ch: cell.ch,
            bold: cell.attrs.contains(Attrs::BOLD),
            italic: cell.attrs.contains(Attrs::ITALIC),
            fg: if cell.attrs.contains(Attrs::REVERSE) {
                cell.bg
            } else {
                cell.fg
            },
        };
        if let Some(info) = self.map.get(&key) {
            return *info;
        }

        // render into scratch (transparent bg, tinted glyph)
        let scratch = self.scratch.clone();
        {
            let cr = Cr::new(&scratch).expect("scratch ctx");
            cr.set_operator(cairo::Operator::Clear);
            let _ = cr.paint();
            let layout = pangocairo::functions::create_layout(&cr);
            let mut font = self.font.clone();
            if key.bold {
                font.set_weight(pango::Weight::Bold);
            }
            if key.italic {
                font.set_style(pango::Style::Italic);
            }
            layout.set_font_description(Some(&font));
            layout.set_text(&cell.ch.to_string());
            let (ink, _logical) = layout.pixel_extents();
            cr.set_source_rgba(
                key.fg.0 as f64 / 255.0,
                key.fg.1 as f64 / 255.0,
                key.fg.2 as f64 / 255.0,
                1.0,
            );
            cr.move_to(-ink.x() as f64, -ink.y() as f64);
            pangocairo::functions::show_layout(&cr, &layout);
            let w = ink.width().max(1);
            let h = ink.height().max(1);

            // shelf-pack
            if self.shelf_x + w > ATLAS_SIZE {
                self.shelf_x = 0;
                self.shelf_y += self.shelf_h + 1;
                self.shelf_h = 0;
            }
            if self.shelf_y + h > ATLAS_SIZE {
                self.reset();
            }
            let (u, v) = (self.shelf_x, self.shelf_y);
            self.shelf_x += w + 1;
            self.shelf_h = self.shelf_h.max(h);
            self.glyphs_rendered += 1;

            // blit scratch into the atlas slot
            let atlas_cr = Cr::new(&self.surface).expect("atlas ctx");
            let _ = atlas_cr.set_source_surface(&scratch, 0.0, 0.0);
            atlas_cr.rectangle(u as f64, v as f64, w as f64, h as f64);
            atlas_cr.fill().expect("atlas blit");

            let info = GlyphInfo { u, v, w, h };
            self.map.insert(key, info);
            info
        }
    }
}

/// What a render pass actually did — feeds bench gates (ADR-0002).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrawStats {
    pub rows_drawn: u32,
    pub glyphs_blitted: u32,
}

/// Render the damaged rows of `grid` into `canvas` using `atlas`.
pub fn render_grid(atlas: &mut GlyphAtlas, canvas: &Canvas, grid: &mut TextGrid) -> DrawStats {
    let mut stats = DrawStats::default();
    let rows: Vec<usize> = match grid.take_damage() {
        DamageSnapshot::Clean => return stats,
        DamageSnapshot::Rows(r) => r,
        DamageSnapshot::Full => (0..grid.rows()).collect(),
    };
    let cr = canvas.context().expect("canvas ctx");

    for y in rows {
        stats.rows_drawn += 1;
        let cy = (y * atlas.cell_h as usize) as f64;
        // background per style run, then glyphs
        for (start, len, rep) in grid.row_style_runs(y) {
            let bg = if rep.attrs.contains(Attrs::REVERSE) {
                rep.fg
            } else {
                rep.bg
            };
            let x = (start * atlas.cell_w as usize) as f64;
            let w = (len * atlas.cell_w as usize) as f64;
            cr.set_source_rgba(
                bg.0 as f64 / 255.0,
                bg.1 as f64 / 255.0,
                bg.2 as f64 / 255.0,
                1.0,
            );
            cr.rectangle(x, cy, w, atlas.cell_h as f64);
            let _ = cr.fill();
        }
        let row: Vec<Cell> = (0..grid.cols()).map(|x| grid.visible_row(y)[x]).collect();
        for x in 0..grid.cols() {
            let Some(cell) = row.get(x).copied() else {
                continue;
            };
            if cell.ch == ' ' {
                continue;
            }
            let info = atlas.glyph(&cell);
            let cx = (x * atlas.cell_w as usize) as f64;
            let _ = cr.set_source_surface(
                &atlas.surface,
                cx - info.u as f64,
                cy + (atlas.ascent - atlas.cell_h) as f64 - info.v as f64,
            );
            cr.rectangle(
                cx,
                cy + (atlas.ascent - atlas.cell_h) as f64,
                (atlas.cell_w as usize) as f64,
                atlas.cell_h as f64,
            );
            let _ = cr.fill();
            stats.glyphs_blitted += 1;
        }
    }
    canvas.surface().flush();
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_cell(ch: char) -> Cell {
        Cell {
            ch,
            ..Cell::default()
        }
    }

    #[test]
    fn damage_drives_only_changed_rows() {
        let mut atlas = GlyphAtlas::new(12);
        let cols = 10;
        let rows = (atlas.cell_h as usize * 3 + 8) / atlas.cell_h as usize; // 3 rows worth
        let mut grid = TextGrid::new(cols, rows);
        for y in 0..rows {
            for x in 0..cols {
                grid.set(x, y, text_cell((b'a' + (x + y) as u8 % 26) as char));
            }
        }
        let canvas = Canvas::new(
            (cols * atlas.cell_w as usize) as i32,
            (rows * atlas.cell_h as usize) as i32,
        )
        .expect("canvas");
        let s1 = render_grid(&mut atlas, &canvas, &mut grid);
        assert_eq!(s1.rows_drawn, rows as u32); // initial Full

        let s2 = render_grid(&mut atlas, &canvas, &mut grid);
        assert_eq!(s2.rows_drawn, 0); // Clean — nothing redrawn
        assert_eq!(s2.glyphs_blitted, 0);

        grid.set(0, 1, text_cell('Z'));
        let s3 = render_grid(&mut atlas, &canvas, &mut grid);
        assert_eq!(s3.rows_drawn, 1);
        assert!(s3.glyphs_blitted > 0);
    }

    #[test]
    fn atlas_caches_tinted_glyphs() {
        let mut atlas = GlyphAtlas::new(12);
        let before = atlas.glyphs_rendered;
        let c1 = text_cell('x');
        let _ = atlas.glyph(&c1);
        let _ = atlas.glyph(&c1); // cache hit
        assert_eq!(atlas.glyphs_rendered, before + 1);
        // different fg → separate (tinted) entry
        let c2 = Cell {
            fg: Rgb(255, 0, 0),
            ..text_cell('x')
        };
        let _ = atlas.glyph(&c2);
        assert_eq!(atlas.glyphs_rendered, before + 2);
    }
}
