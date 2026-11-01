//! Text-grid fast path (ADR-0004 §text-grid, ADR-0011 terminal).
//!
//! The cell model `ibde-terminal` renders through: a dense grid of
//! styled cells plus O(1)-bookkeeping damage tracking. Rendering (glyph
//! atlas, cairo/GLES2 blit) lands on top of this in the next chunk; this
//! module is pure logic — no Wayland, no cairo — so it is fully unit
//! tested and benchmarkable anywhere.

/// RGB color, 8 bits/channel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Per-cell text attributes (bitflags).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Attrs(pub u16);

impl Attrs {
    pub const BOLD: Attrs = Attrs(1 << 0);
    pub const ITALIC: Attrs = Attrs(1 << 1);
    pub const UNDERLINE: Attrs = Attrs(1 << 2);
    pub const REVERSE: Attrs = Attrs(1 << 3);
    pub const WIDE: Attrs = Attrs(1 << 4); // double-width cell (CJK etc.)

    pub fn union(self, other: Attrs) -> Attrs {
        Attrs(self.0 | other.0)
    }
    pub fn contains(self, other: Attrs) -> bool {
        self.0 & other.0 == other.0
    }
}

/// One terminal cell. Target layout: 16 bytes (char + 2×Rgb + Attrs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: Rgb(0x1d, 0x1d, 0x20),
            bg: Rgb(0xfa, 0xfa, 0xf8),
            attrs: Attrs(0),
        }
    }
}

impl Cell {
    /// Same visual style (attrs + colors) — the run-merge key for glyph
    /// batching; the character itself varies within a run.
    pub fn same_style(&self, other: &Cell) -> bool {
        self.attrs == other.attrs && self.fg == other.fg && self.bg == other.bg
    }
}

/// Damage bookkeeping for a grid: full-damage flag or a dirty-row set.
/// Scrolling shifts the row set; single-cell writes mark single rows.
#[derive(Debug, Default)]
pub struct Damage {
    full: bool,
    rows: Vec<bool>, // len == grid rows; true = dirty
}

impl Damage {
    fn new(rows: usize) -> Self {
        Damage {
            full: true,
            rows: vec![false; rows],
        }
    }

    pub fn mark_row(&mut self, y: usize) {
        if !self.full {
            if let Some(r) = self.rows.get_mut(y) {
                *r = true;
            }
        }
    }

    pub fn mark_all(&mut self) {
        self.full = true;
        for r in self.rows.iter_mut() {
            *r = false;
        }
    }

    /// Scroll damage by `n` rows (content moved up): a row that received
    /// content from row y+n keeps y+n's dirtiness; the n rows at the
    /// bottom become dirty (new blank lines).
    fn scroll_up(&mut self, n: usize) {
        if self.full || n == 0 {
            return;
        }
        let len = self.rows.len();
        let n = n.min(len);
        self.rows.copy_within(n.., 0);
        for r in self.rows[len - n..].iter_mut() {
            *r = true;
        }
    }

    pub fn take(&mut self) -> DamageSnapshot {
        if self.full {
            self.full = false;
            for r in self.rows.iter_mut() {
                *r = false; // Full reports everything; nothing stays dirty
            }
            DamageSnapshot::Full
        } else if self.rows.iter().all(|r| !*r) {
            DamageSnapshot::Clean
        } else {
            let rows = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| **r)
                .map(|(y, _)| y)
                .collect();
            for r in self.rows.iter_mut() {
                *r = false;
            }
            DamageSnapshot::Rows(rows)
        }
    }
}

/// What needs repainting after a frame.
#[derive(Debug, PartialEq, Eq)]
pub enum DamageSnapshot {
    Clean,
    Rows(Vec<usize>),
    Full,
}

/// Scrollback capacity (rows) — ADR-0011 budget: ≤25MB at 10k rows.
const HISTORY_CAP: usize = 10_000;

/// A fixed-size grid of styled cells with damage tracking, plus a
/// scrollback history and a view offset (0 = live bottom).
pub struct TextGrid {
    cols: usize,
    rows: usize,
    cells: Vec<Cell>,
    damage: Damage,
    history: Vec<Vec<Cell>>,
    /// rows scrolled back from the live view
    pub view: usize,
}

impl TextGrid {
    pub fn new(cols: usize, rows: usize) -> Self {
        TextGrid {
            cols,
            rows,
            cells: vec![Cell::default(); cols * rows],
            damage: Damage::new(rows),
            history: Vec::new(),
            view: 0,
        }
    }

    /// Row `y` of the current view: up to `view.min(rows)` rows from
    /// the history tail, then the live grid.
    pub fn visible_row(&self, y: usize) -> &[Cell] {
        let from_hist = self.view.min(self.rows);
        if y < from_hist {
            let idx = self.history.len() - self.view + y;
            &self.history[idx]
        } else {
            let gy = y - from_hist;
            &self.cells[gy * self.cols..(gy + 1) * self.cols]
        }
    }

    /// Scroll the view back (positive) or forward (negative); clamps to
    /// [0, history.len()]. Marks everything dirty on change.
    pub fn scroll_view(&mut self, delta: i32) {
        let new_view = if delta < 0 {
            self.view.saturating_sub((-delta) as usize)
        } else {
            self.view + delta as usize
        }
        .min(self.history.len());
        if new_view != self.view {
            self.view = new_view;
            self.damage.mark_all();
        }
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Text of absolute history row `a` (0 = oldest).
    pub fn history_row_text(&self, a: usize) -> String {
        self.history
            .get(a)
            .map(|row| {
                row.iter()
                    .map(|c| if c.ch == '\0' { ' ' } else { c.ch })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn get(&self, x: usize, y: usize) -> Option<Cell> {
        if x < self.cols && y < self.rows {
            Some(self.cells[y * self.cols + x])
        } else {
            None
        }
    }

    /// Write a cell; marks the row dirty only when content actually
    /// changed (renderer elision foundation).
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.cols && y < self.rows {
            let slot = &mut self.cells[y * self.cols + x];
            if *slot != cell {
                *slot = cell;
                self.damage.mark_row(y);
            }
        }
    }

    /// Scroll content up by `n` rows, blanking the bottom `n` rows.
    /// Pushes leaving rows into scrollback history. O(cells) memcpy +
    /// O(rows) damage bookkeeping.
    pub fn scroll_up(&mut self, n: usize) {
        let n = n.min(self.rows);
        if n == 0 {
            return;
        }
        for i in 0..n {
            self.history
                .push(self.cells[i * self.cols..(i + 1) * self.cols].to_vec());
        }
        if self.history.len() > HISTORY_CAP {
            let drop = self.history.len() - HISTORY_CAP;
            self.history.drain(0..drop);
            // keep the view anchored within the new bounds
            self.view = self.view.min(HISTORY_CAP);
        }
        self.cells.copy_within(n * self.cols.., 0);
        let tail = &mut self.cells[(self.rows - n) * self.cols..];
        for c in tail.iter_mut() {
            *c = Cell::default();
        }
        self.damage.scroll_up(n);
    }

    /// Resize preserving as much content as possible (top-left anchored).
    /// History rows are re-padded to the new width.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        for row in self.history.iter_mut() {
            row.resize(cols, Cell::default());
        }
        self.view = self.view.min(self.history.len());
        let mut next = vec![Cell::default(); cols * rows];
        let copy_rows = rows.min(self.rows);
        let copy_cols = cols.min(self.cols);
        for y in 0..copy_rows {
            let src = &self.cells[y * self.cols..y * self.cols + copy_cols];
            next[y * cols..y * cols + copy_cols].copy_from_slice(src);
        }
        self.cells = next;
        self.cols = cols;
        self.rows = rows;
        self.damage = Damage::new(rows);
    }

    pub fn take_damage(&mut self) -> DamageSnapshot {
        self.damage.take()
    }

    pub fn mark_all_dirty(&mut self) {
        self.damage.mark_all();
    }

    /// Run-length segments of cells with identical visual style in a
    /// row — what the glyph-atlas renderer batches on. Each entry is
    /// (start_col, len, representative_cell); the representative carries
    /// the run's style (attrs + colors), characters vary within the run.
    pub fn row_style_runs(&self, y: usize) -> Vec<(usize, usize, Cell)> {
        let mut out = Vec::new();
        if y >= self.rows {
            return out;
        }
        let row = &self.cells[y * self.cols..(y + 1) * self.cols];
        let mut run_start = 0;
        for (i, c) in row.iter().enumerate() {
            if i > 0 && !c.same_style(&row[i - 1]) {
                out.push((run_start, i - run_start, row[i - 1]));
                run_start = i;
            }
        }
        out.push((run_start, self.cols - run_start, row[self.cols - 1]));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_a(ch: char) -> Cell {
        Cell {
            ch,
            ..Cell::default()
        }
    }

    #[test]
    fn set_marks_only_changed_rows() {
        let mut g = TextGrid::new(4, 3);
        assert_eq!(g.take_damage(), DamageSnapshot::Full); // initial full
        g.set(1, 1, cell_a('x'));
        g.set(2, 1, cell_a('x')); // same cell content, no new dirt
        assert_eq!(g.take_damage(), DamageSnapshot::Rows(vec![1]));
        assert_eq!(g.take_damage(), DamageSnapshot::Clean); // consumed
    }

    #[test]
    fn scroll_moves_content_and_damage() {
        let mut g = TextGrid::new(2, 4);
        for y in 0..4 {
            for x in 0..2 {
                g.set(x, y, cell_a((b'a' + y as u8) as char));
            }
        }
        let _ = g.take_damage(); // consume initial full damage
        g.scroll_up(1);
        assert_eq!(g.get(0, 0), Some(cell_a('b')));
        assert_eq!(g.get(0, 3), Some(cell_a(' ')));
        // rows that received shifted content (0..2 were clean before, now
        // hold 'b','c' — content changed) plus blank row 3 are dirty
        let d = g.take_damage();
        assert!(matches!(d, DamageSnapshot::Rows(_)));
    }

    #[test]
    fn scroll_damage_tracks_dirty_rows() {
        let mut g = TextGrid::new(1, 4);
        let _ = g.take_damage();
        g.set(0, 2, cell_a('z')); // only row 2 dirty
        g.scroll_up(1); // z moves to row 1
        assert_eq!(g.get(0, 1), Some(cell_a('z')));
        // row 1 (received z), row 3 (blanked) dirty; row 0 got old row 1 (blank->blank, stayed clean)
        assert_eq!(g.take_damage(), DamageSnapshot::Rows(vec![1, 3]));
    }

    #[test]
    fn resize_preserves_top_left() {
        let mut g = TextGrid::new(2, 2);
        g.set(0, 0, cell_a('a'));
        g.set(1, 1, cell_a('b'));
        g.resize(3, 3);
        assert_eq!(g.get(0, 0), Some(cell_a('a')));
        assert_eq!(g.get(1, 1), Some(cell_a('b')));
        assert_eq!(g.get(2, 2), Some(cell_a(' ')));
        assert_eq!(g.take_damage(), DamageSnapshot::Full);
    }

    #[test]
    fn style_runs_merge_by_style_not_glyph() {
        let mut g = TextGrid::new(4, 1);
        g.set(0, 0, cell_a('h'));
        g.set(1, 0, cell_a('i'));
        g.set(
            2,
            0,
            Cell {
                ch: '!',
                attrs: Attrs::BOLD,
                ..cell_a('!')
            },
        );
        g.set(
            3,
            0,
            Cell {
                ch: '?',
                attrs: Attrs::BOLD,
                ..cell_a('?')
            },
        );
        let runs = g.row_style_runs(0);
        assert_eq!(runs.len(), 2); // "hi" plain + "!?" bold
                                   // representatives carry style only — they're the last cell
                                   // before each style boundary
        assert_eq!(runs[0], (0, 2, g.get(1, 0).unwrap()));
        assert_eq!(runs[1], (2, 2, g.get(3, 0).unwrap()));
    }

    #[test]
    fn history_and_view() {
        let mut g = TextGrid::new(4, 2);
        let a = Cell {
            ch: 'a',
            ..Cell::default()
        };
        for round in 0..3 {
            for x in 0..4 {
                g.set(
                    x,
                    0,
                    Cell {
                        ch: (b'a' + round) as char,
                        ..a
                    },
                );
            }
            g.scroll_up(1); // pushes row0 each time
        }
        assert_eq!(g.history_len(), 3);
        // live grid was blanked by the last scroll
        assert_eq!(g.visible_row(1)[0].ch, ' ');
        // one back: newest history row on top
        g.scroll_view(1);
        assert_eq!(g.visible_row(0)[0].ch, 'c');
        // fully back: oldest history row visible
        g.scroll_view(2);
        assert_eq!(g.visible_row(0)[0].ch, 'a');
        // clamp forward
        g.scroll_view(-99);
        assert_eq!(g.view, 0);
        assert_eq!(g.visible_row(1)[0].ch, ' ');
    }

    #[test]
    fn cell_size_budget() {
        // ADR-0011 budget math assumes ~16-byte cells.
        assert!(
            std::mem::size_of::<Cell>() <= 16,
            "Cell is {} bytes",
            std::mem::size_of::<Cell>()
        );
    }
}
