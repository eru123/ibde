//! ibde-terminal — emulation core (ADR-0011).
//!
//! The [`vte`] crate parses raw PTY bytes; [`TermModel`] applies them to
//! an [`ibde_ui::grid::TextGrid`] with damage tracking intact. No
//! windowing here — PTY handling and rendering attach above this core,
//! which keeps the emulation 100 % unit-testable.

use ibde_ui::grid::{Attrs, Cell, Rgb, TextGrid};
use vte::{Params, Perform};

/// 16-color ANSI palette (macOS-flavored light default; themeable later).
pub fn ansi(i: u8) -> Rgb {
    const PAL: [Rgb; 16] = [
        Rgb(0x1d, 0x1d, 0x20),
        Rgb(0xa6, 0x36, 0x36),
        Rgb(0x2f, 0x7e, 0x3a),
        Rgb(0x9a, 0x6b, 0x2f),
        Rgb(0x2c, 0x5d, 0xa8),
        Rgb(0x78, 0x3c, 0x8e),
        Rgb(0x2a, 0x76, 0x78),
        Rgb(0x6e, 0x6e, 0x73),
        Rgb(0x49, 0x49, 0x4e),
        Rgb(0xd3, 0x5f, 0x5f),
        Rgb(0x4c, 0xa6, 0x55),
        Rgb(0xc8, 0x8b, 0x38),
        Rgb(0x44, 0x76, 0xd0),
        Rgb(0x9a, 0x53, 0xb8),
        Rgb(0x38, 0x9c, 0x9e),
        Rgb(0xfa, 0xfa, 0xf8),
    ];
    PAL[i as usize % 16]
}

const DEFAULT_FG: Rgb = Rgb(0x1d, 0x1d, 0x20);
const DEFAULT_BG: Rgb = Rgb(0xfa, 0xfa, 0xf8);

/// Cursor + active SGR state + the grid.
pub struct TermModel {
    pub grid: TextGrid,
    pub cursor_x: usize,
    pub cursor_y: usize,
    fg: Rgb,
    bg: Rgb,
    attrs: Attrs,
    /// last OSC 0/2 title seen
    pub title: String,
}

impl TermModel {
    pub fn new(cols: usize, rows: usize) -> Self {
        TermModel {
            grid: TextGrid::new(cols, rows),
            cursor_x: 0,
            cursor_y: 0,
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: Attrs(0),
            title: String::new(),
        }
    }

    fn blank(&self) -> Cell {
        Cell {
            ch: ' ',
            fg: self.fg,
            bg: self.bg,
            attrs: Attrs(0),
        }
    }

    fn put(&mut self, ch: char) {
        // NOTE(v0): eager wrap — the cursor wraps as soon as the last
        // column is written. Real VT100s defer this with a pending-wrap
        // flag; we adopt that when edge-case fidelity matters (CR at the
        // last column currently lands on the next row).
        self.grid.set(
            self.cursor_x,
            self.cursor_y,
            Cell {
                ch,
                fg: self.fg,
                bg: self.bg,
                attrs: self.attrs,
            },
        );
        self.cursor_x += 1;
        if self.cursor_x >= self.grid.cols() {
            self.cursor_x = 0;
            self.linefeed();
        }
    }

    fn linefeed(&mut self) {
        if self.cursor_y + 1 >= self.grid.rows() {
            self.grid.scroll_up(1);
        } else {
            self.cursor_y += 1;
        }
    }

    fn erase_row_from(&mut self, from: usize) {
        let blank = self.blank();
        for x in from..self.grid.cols() {
            self.grid.set(x, self.cursor_y, blank);
        }
    }
}

fn pi(params: &Params, i: usize) -> u16 {
    params
        .iter()
        .nth(i)
        .and_then(|sub| sub.first())
        .copied()
        .unwrap_or(0)
}

fn pn(params: &Params, i: usize) -> u16 {
    let v = pi(params, i);
    if v == 0 {
        1
    } else {
        v
    }
}

impl Perform for TermModel {
    fn print(&mut self, c: char) {
        self.put(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\r' => self.cursor_x = 0,
            b'\n' | 0x0b | 0x0c => self.linefeed(),
            0x08 => self.cursor_x = self.cursor_x.saturating_sub(1),
            b'\t' => {
                let next = (self.cursor_x / 8 + 1) * 8;
                self.cursor_x = next.min(self.grid.cols() - 1);
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        match action {
            'A' => self.cursor_y = self.cursor_y.saturating_sub(pn(params, 0) as usize),
            'B' => {
                self.cursor_y = (self.cursor_y + pn(params, 0) as usize).min(self.grid.rows() - 1)
            }
            'C' => {
                self.cursor_x = (self.cursor_x + pn(params, 0) as usize).min(self.grid.cols() - 1)
            }
            'D' => self.cursor_x = self.cursor_x.saturating_sub(pn(params, 0) as usize),
            'H' | 'f' => {
                self.cursor_y = ((pn(params, 0) - 1) as usize).min(self.grid.rows() - 1);
                self.cursor_x = ((pn(params, 1) - 1) as usize).min(self.grid.cols() - 1);
            }
            'J' => match pi(params, 0) {
                0 => self.erase_row_from(self.cursor_x),
                _ => {
                    // 1 (above) and 2 (all): full clear is acceptable v0
                    let blank = self.blank();
                    for y in 0..self.grid.rows() {
                        for x in 0..self.grid.cols() {
                            self.grid.set(x, y, blank);
                        }
                    }
                    self.cursor_x = 0;
                    self.cursor_y = 0;
                }
            },
            'K' => match pi(params, 0) {
                0 => self.erase_row_from(self.cursor_x),
                1 => {
                    let blank = self.blank();
                    let end = (self.cursor_x + 1).min(self.grid.cols());
                    for x in 0..end {
                        self.grid.set(x, self.cursor_y, blank);
                    }
                }
                _ => self.erase_row_from(0),
            },
            'm' => self.sgr(params),
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_ignored: bool) {
        // OSC 0 / 2: window title
        if let Some(kind) = params.first() {
            if kind.is_empty() || *kind == b"0" || *kind == b"2" {
                if let Some(title) = params.get(1) {
                    self.title = String::from_utf8_lossy(title).into_owned();
                }
            }
        }
    }
}

impl TermModel {
    fn sgr(&mut self, params: &Params) {
        let flat: Vec<u16> = params.iter().flat_map(|sub| sub.iter().copied()).collect();
        let mut i = 0;
        while i < flat.len() {
            let c = flat[i];
            match c {
                0 => {
                    self.fg = DEFAULT_FG;
                    self.bg = DEFAULT_BG;
                    self.attrs = Attrs(0);
                }
                1 => self.attrs = self.attrs.union(Attrs::BOLD),
                3 => self.attrs = self.attrs.union(Attrs::ITALIC),
                4 => self.attrs = self.attrs.union(Attrs::UNDERLINE),
                7 => self.attrs = self.attrs.union(Attrs::REVERSE),
                22 => self.attrs = Attrs(self.attrs.0 & !Attrs::BOLD.0),
                23 => self.attrs = Attrs(self.attrs.0 & !Attrs::ITALIC.0),
                24 => self.attrs = Attrs(self.attrs.0 & !Attrs::UNDERLINE.0),
                27 => self.attrs = Attrs(self.attrs.0 & !Attrs::REVERSE.0),
                30..=37 => self.fg = ansi((c - 30) as u8),
                38 => {
                    if let Some(rgb) = sgr_extended(&flat, i) {
                        self.fg = rgb.0;
                        i = rgb.1;
                    }
                }
                39 => self.fg = DEFAULT_FG,
                40..=47 => self.bg = ansi((c - 40) as u8),
                48 => {
                    if let Some(rgb) = sgr_extended(&flat, i) {
                        self.bg = rgb.0;
                        i = rgb.1;
                    }
                }
                49 => self.bg = DEFAULT_BG,
                90..=97 => self.fg = ansi((c - 90 + 8) as u8),
                100..=107 => self.bg = ansi((c - 100 + 8) as u8),
                _ => {}
            }
            i += 1;
        }
    }
}

/// Parse `38;5;n` / `38;2;r;g;b` (and the 48 twins). Returns
/// (rgb, index of last consumed param).
fn sgr_extended(flat: &[u16], i: usize) -> Option<(Rgb, usize)> {
    match flat.get(i + 1) {
        Some(5) => flat.get(i + 2).map(|&n| (xterm256(n as u8), i + 2)),
        Some(2) => {
            let (r, g, b) = (flat.get(i + 2), flat.get(i + 3), flat.get(i + 4));
            match (r, g, b) {
                (Some(&r), Some(&g), Some(&b)) => {
                    Some((Rgb(clamp8(r), clamp8(g), clamp8(b)), i + 4))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn clamp8(v: u16) -> u8 {
    v.min(255) as u8
}

fn xterm256(n: u8) -> Rgb {
    match n {
        0..=15 => ansi(n),
        16..=231 => {
            // 6x6x6 cube
            let n = n as i32 - 16;
            let lv = [0, 95, 135, 175, 215, 255];
            let r = lv[(n / 36) as usize];
            let g = lv[((n % 36) / 6) as usize];
            let b = lv[(n % 6) as usize];
            Rgb(r as u8, g as u8, b as u8)
        }
        232..=255 => {
            let v = 8 + (n as i32 - 232) * 10;
            Rgb(v as u8, v as u8, v as u8)
        }
    }
}

/// Feed raw bytes through the vte parser into the model.
pub struct Feed {
    parser: vte::Parser,
}

impl Feed {
    pub fn new() -> Self {
        Feed {
            parser: vte::Parser::new(),
        }
    }

    pub fn advance(&mut self, model: &mut TermModel, bytes: &[u8]) {
        for b in bytes {
            self.parser.advance(model, *b);
        }
    }
}

impl Default for Feed {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(input: &str) -> TermModel {
        let mut m = TermModel::new(20, 4);
        let mut f = Feed::new();
        f.advance(&mut m, input.as_bytes());
        m
    }

    #[test]
    fn plain_text_lands_in_grid() {
        let m = run("hi");
        assert_eq!(m.grid.get(0, 0).map(|c| c.ch), Some('h'));
        assert_eq!(m.grid.get(1, 0).map(|c| c.ch), Some('i'));
        assert_eq!(m.cursor_x, 2);
    }

    #[test]
    fn newline_scroll_keeps_last_row() {
        let mut m = TermModel::new(4, 2);
        let mut f = Feed::new();
        f.advance(&mut m, b"row1\r\nrow2\r\nrow3");
        // 2-row grid: the eager auto-wrap after the last column of
        // "row3" scrolls once more, so the latest line rests on row 0
        assert_eq!(m.grid.get(0, 0).map(|c| c.ch), Some('r'));
        assert_eq!(m.grid.get(3, 0).map(|c| c.ch), Some('3'));
    }

    #[test]
    fn sgr_basic_colors() {
        let m = run("\x1b[31mred\x1b[0mplain");
        assert_eq!(m.grid.get(0, 0).unwrap().fg, ansi(1));
        assert_eq!(m.grid.get(3, 0).unwrap().fg, DEFAULT_FG);
    }

    #[test]
    fn sgr_truecolor_and_256() {
        let m = run("\x1b[38;2;255;0;0mA\x1b[38;5;196mB");
        assert_eq!(m.grid.get(0, 0).unwrap().fg, Rgb(255, 0, 0));
        assert_eq!(m.grid.get(1, 0).unwrap().fg, Rgb(255, 0, 0)); // 196 = cube red
    }

    #[test]
    fn sgr_bold_and_reverse() {
        let m = run("\x1b[1;7mx");
        assert!(m.grid.get(0, 0).unwrap().attrs.contains(Attrs::BOLD));
        assert!(m.grid.get(0, 0).unwrap().attrs.contains(Attrs::REVERSE));
    }

    #[test]
    fn cursor_movement_and_erase() {
        let mut m = run("abcdef");
        let mut f = Feed::new();
        f.advance(&mut m, b"\x1b[1;3H"); // row 1, col 3
        assert_eq!((m.cursor_x, m.cursor_y), (2, 0));
        f.advance(&mut m, b"\x1b[K"); // erase to end of line
        assert_eq!(m.grid.get(2, 0).map(|c| c.ch), Some(' '));
        assert_eq!(m.grid.get(1, 0).map(|c| c.ch), Some('b'));
    }

    #[test]
    fn wrap_at_grid_edge() {
        let mut m = TermModel::new(4, 3);
        let mut f = Feed::new();
        f.advance(&mut m, b"abcdef"); // wraps at 4 cols
        assert_eq!(m.grid.get(3, 0).map(|c| c.ch), Some('d'));
        assert_eq!(m.grid.get(1, 1).map(|c| c.ch), Some('f'));
    }

    #[test]
    fn osc_title() {
        let m = run("\x1b]0;My Title\x07hello");
        assert_eq!(m.title, "My Title");
        assert_eq!(m.grid.get(0, 0).map(|c| c.ch), Some('h'));
    }

    #[test]
    fn tab_stops() {
        let m = run("a\tb");
        assert_eq!(m.grid.get(0, 0).map(|c| c.ch), Some('a'));
        assert_eq!(m.grid.get(8, 0).map(|c| c.ch), Some('b'));
    }
}
