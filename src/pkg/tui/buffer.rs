//! Cell buffer, styles and drawing primitives.

/// A 24-bit color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Linear blend from `self` (t = 0) to `other` (t = 1).
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t).round() as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }
}

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Rgb,
    pub bg: Rgb,
    pub attrs: u8,
}

impl Style {
    pub const fn new(fg: Rgb, bg: Rgb) -> Self {
        Style { fg, bg, attrs: 0 }
    }

    pub const fn fg(mut self, fg: Rgb) -> Self {
        self.fg = fg;
        self
    }

    pub const fn bg(mut self, bg: Rgb) -> Self {
        self.bg = bg;
        self
    }

    pub const fn bold(mut self) -> Self {
        self.attrs |= BOLD;
        self
    }

    pub const fn italic(mut self) -> Self {
        self.attrs |= ITALIC;
        self
    }

    pub const fn underline(mut self) -> Self {
        self.attrs |= UNDERLINE;
        self
    }

    pub fn write_sgr(&self, out: &mut String) {
        out.push_str("\x1b[0");
        if self.attrs & BOLD != 0 {
            out.push_str(";1");
        }
        if self.attrs & ITALIC != 0 {
            out.push_str(";3");
        }
        if self.attrs & UNDERLINE != 0 {
            out.push_str(";4");
        }
        out.push_str(&format!(
            ";38;2;{};{};{};48;2;{};{};{}m",
            self.fg.0, self.fg.1, self.fg.2, self.bg.0, self.bg.1, self.bg.2
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: Style,
    /// Continuation cell covered by the wide glyph on its left.
    pub cont: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, w: usize, h: usize) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> usize {
        self.x + self.w
    }

    pub fn bottom(&self) -> usize {
        self.y + self.h
    }

    /// Shrinks the rectangle by `dx` columns and `dy` rows on every side.
    pub fn inset(&self, dx: usize, dy: usize) -> Rect {
        Rect {
            x: self.x + dx,
            y: self.y + dy,
            w: self.w.saturating_sub(dx * 2),
            h: self.h.saturating_sub(dy * 2),
        }
    }

    /// Splits off `n` rows from the top: `(top, rest)`.
    pub fn split_top(&self, n: usize) -> (Rect, Rect) {
        let n = n.min(self.h);
        (
            Rect::new(self.x, self.y, self.w, n),
            Rect::new(self.x, self.y + n, self.w, self.h - n),
        )
    }

    /// Splits off `n` columns from the left: `(left, rest)`.
    pub fn split_left(&self, n: usize) -> (Rect, Rect) {
        let n = n.min(self.w);
        (
            Rect::new(self.x, self.y, n, self.h),
            Rect::new(self.x + n, self.y, self.w - n, self.h),
        )
    }

    /// Splits the width into columns proportional to `weights`.
    pub fn columns(&self, weights: &[usize], gap: usize) -> Vec<Rect> {
        let total: usize = weights.iter().sum::<usize>().max(1);
        let gaps = gap * weights.len().saturating_sub(1);
        let avail = self.w.saturating_sub(gaps);
        let mut out = Vec::with_capacity(weights.len());
        let mut x = self.x;
        let mut used = 0;
        for (i, wt) in weights.iter().enumerate() {
            let w = if i + 1 == weights.len() {
                avail - used
            } else {
                avail * wt / total
            };
            out.push(Rect::new(x, self.y, w, self.h));
            x += w + gap;
            used += w;
        }
        out
    }

    /// Intersects this rectangle with `area`.
    pub fn clip(&self, area: Rect) -> Rect {
        let x = self.x.min(area.right());
        let y = self.y.min(area.bottom());
        let r = self.right().min(area.right());
        let b = self.bottom().min(area.bottom());
        Rect::new(x, y, r.saturating_sub(x), b.saturating_sub(y))
    }

    /// Centers a `w` x `h` rectangle inside this one.
    pub fn centered(&self, w: usize, h: usize) -> Rect {
        let w = w.min(self.w);
        let h = h.min(self.h);
        Rect::new(self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h)
    }
}

/// Display width of a character in terminal cells.
pub fn char_width(c: char) -> usize {
    let cp = c as u32;
    if cp < 0x20 || (0x7f..0xa0).contains(&cp) {
        return 0;
    }
    if (0x300..=0x36f).contains(&cp)
        || (0x200b..=0x200f).contains(&cp)
        || (0xfe00..=0xfe0f).contains(&cp)
    {
        return 0;
    }
    let wide = (0x1100..=0x115f).contains(&cp)
        || (0x2e80..=0x303e).contains(&cp)
        || (0x3041..=0x33ff).contains(&cp)
        || (0x3400..=0x4dbf).contains(&cp)
        || (0x4e00..=0x9fff).contains(&cp)
        || (0xa000..=0xa4cf).contains(&cp)
        || (0xac00..=0xd7a3).contains(&cp)
        || (0xf900..=0xfaff).contains(&cp)
        || (0xfe30..=0xfe4f).contains(&cp)
        || (0xff00..=0xff60).contains(&cp)
        || (0xffe0..=0xffe6).contains(&cp)
        || (0x1f300..=0x1f64f).contains(&cp)
        || (0x1f900..=0x1f9ff).contains(&cp)
        || (0x20000..=0x3fffd).contains(&cp);
    if wide {
        2
    } else {
        1
    }
}

/// Display width of a string.
pub fn str_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

/// Truncates `s` to at most `w` cells, ending with an ellipsis when cut.
pub fn truncate(s: &str, w: usize) -> String {
    if str_width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if used + cw > w - 1 {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push('…');
    out
}

/// Truncates `s` from the left, keeping its tail (useful for paths).
pub fn truncate_left(s: &str, w: usize) -> String {
    if str_width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0;
    for c in s.chars().rev() {
        let cw = char_width(c);
        if used + cw > w - 1 {
            break;
        }
        tail.push(c);
        used += cw;
    }
    let mut out = String::from("…");
    out.extend(tail.into_iter().rev());
    out
}

/// Pads `s` with spaces on the right to exactly `w` cells (truncating if needed).
pub fn pad(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let sw = str_width(&t);
    format!("{}{}", t, " ".repeat(w - sw))
}

/// Pads `s` with spaces on the left to exactly `w` cells.
pub fn pad_left(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let sw = str_width(&t);
    format!("{}{}", " ".repeat(w - sw), t)
}

/// Greedy word wrap to `w` cells.
pub fn wrap(s: &str, w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if w == 0 {
        return lines;
    }
    for para in s.split('\n') {
        let mut line = String::new();
        let mut lw = 0;
        for word in para.split(' ') {
            let ww = str_width(word);
            if lw > 0 && lw + 1 + ww > w {
                lines.push(std::mem::take(&mut line));
                lw = 0;
            }
            if lw > 0 {
                line.push(' ');
                lw += 1;
            }
            if ww > w {
                for c in word.chars() {
                    let cw = char_width(c);
                    if lw + cw > w {
                        lines.push(std::mem::take(&mut line));
                        lw = 0;
                    }
                    line.push(c);
                    lw += cw;
                }
            } else {
                line.push_str(word);
                lw += ww;
            }
        }
        lines.push(line);
    }
    lines
}

/// A run of text sharing one style.
pub type Span = (String, Style);

pub fn span(s: impl Into<String>, st: Style) -> Span {
    (s.into(), st)
}

/// Box drawing character sets.
#[derive(Clone, Copy)]
pub struct BorderSet {
    pub tl: char,
    pub tr: char,
    pub bl: char,
    pub br: char,
    pub h: char,
    pub v: char,
}

pub const ROUNDED: BorderSet = BorderSet {
    tl: '╭',
    tr: '╮',
    bl: '╰',
    br: '╯',
    h: '─',
    v: '│',
};

pub const HEAVY: BorderSet = BorderSet {
    tl: '┏',
    tr: '┓',
    bl: '┗',
    br: '┛',
    h: '━',
    v: '┃',
};

pub const DOUBLE: BorderSet = BorderSet {
    tl: '╔',
    tr: '╗',
    bl: '╚',
    br: '╝',
    h: '═',
    v: '║',
};

#[derive(Debug, Clone, PartialEq)]
pub struct Buffer {
    w: usize,
    h: usize,
    cells: Vec<Cell>,
}

impl Buffer {
    pub fn new(w: usize, h: usize, style: Style) -> Self {
        Buffer {
            w,
            h,
            cells: vec![
                Cell {
                    ch: ' ',
                    style,
                    cont: false
                };
                w * h
            ],
        }
    }

    pub fn width(&self) -> usize {
        self.w
    }

    pub fn height(&self) -> usize {
        self.h
    }

    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn cell(&self, x: usize, y: usize) -> &Cell {
        &self.cells[y * self.w + x]
    }

    pub fn cell_mut(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.w && y < self.h {
            Some(&mut self.cells[y * self.w + x])
        } else {
            None
        }
    }

    /// Places one character; returns its width (0 if clipped).
    pub fn set(&mut self, x: usize, y: usize, ch: char, style: Style) -> usize {
        let cw = char_width(ch);
        if cw == 0 || x + cw > self.w || y >= self.h {
            return 0;
        }
        // Writing over half of a wide glyph clears the other half.
        if self.cells[y * self.w + x].cont && x > 0 {
            let left = &mut self.cells[y * self.w + x - 1];
            left.ch = ' ';
        }
        if x + 1 < self.w && self.cells[y * self.w + x + 1].cont && cw == 1 {
            let right = &mut self.cells[y * self.w + x + 1];
            right.cont = false;
            right.ch = ' ';
        }
        self.cells[y * self.w + x] = Cell {
            ch,
            style,
            cont: false,
        };
        if cw == 2 {
            self.cells[y * self.w + x + 1] = Cell {
                ch: ' ',
                style,
                cont: true,
            };
        }
        cw
    }

    /// Draws text clipped at column `max_x`; returns the column after the text.
    pub fn text(&mut self, x: usize, y: usize, s: &str, style: Style, max_x: usize) -> usize {
        let max_x = max_x.min(self.w);
        let mut cx = x;
        for c in s.chars() {
            let c = if c == '\t' || (c as u32) < 0x20 {
                ' '
            } else {
                c
            };
            let cw = char_width(c);
            if cw == 0 {
                continue;
            }
            if cx + cw > max_x {
                break;
            }
            self.set(cx, y, c, style);
            cx += cw;
        }
        cx
    }

    /// Draws a sequence of styled spans; returns the column after the last one.
    pub fn spans(&mut self, x: usize, y: usize, spans: &[Span], max_x: usize) -> usize {
        let mut cx = x;
        for (s, st) in spans {
            cx = self.text(cx, y, s, *st, max_x);
        }
        cx
    }

    /// Draws text right-aligned so that it ends at column `end_x`.
    pub fn text_right(&mut self, end_x: usize, y: usize, s: &str, style: Style) -> usize {
        let w = str_width(s);
        let x = end_x.saturating_sub(w);
        self.text(x, y, s, style, end_x);
        x
    }

    pub fn spans_width(spans: &[Span]) -> usize {
        spans.iter().map(|(s, _)| str_width(s)).sum()
    }

    pub fn fill(&mut self, r: Rect, ch: char, style: Style) {
        for y in r.y..r.bottom().min(self.h) {
            for x in r.x..r.right().min(self.w) {
                self.set(x, y, ch, style);
            }
        }
    }

    /// Changes only the background of the cells in `r`.
    pub fn tint_bg(&mut self, r: Rect, bg: Rgb) {
        for y in r.y..r.bottom().min(self.h) {
            for x in r.x..r.right().min(self.w) {
                self.cells[y * self.w + x].style.bg = bg;
            }
        }
    }

    pub fn hline(&mut self, x: usize, y: usize, w: usize, ch: char, style: Style) {
        for i in 0..w {
            self.set(x + i, y, ch, style);
        }
    }

    pub fn vline(&mut self, x: usize, y: usize, h: usize, ch: char, style: Style) {
        for i in 0..h {
            self.set(x, y + i, ch, style);
        }
    }

    pub fn border(&mut self, r: Rect, set: BorderSet, style: Style) {
        if r.w < 2 || r.h < 2 {
            return;
        }
        let (x0, y0, x1, y1) = (r.x, r.y, r.right() - 1, r.bottom() - 1);
        self.set(x0, y0, set.tl, style);
        self.set(x1, y0, set.tr, style);
        self.set(x0, y1, set.bl, style);
        self.set(x1, y1, set.br, style);
        self.hline(x0 + 1, y0, r.w - 2, set.h, style);
        self.hline(x0 + 1, y1, r.w - 2, set.h, style);
        self.vline(x0, y0 + 1, r.h - 2, set.v, style);
        self.vline(x1, y0 + 1, r.h - 2, set.v, style);
    }

    /// Dims every cell towards `toward`, used as the modal backdrop.
    pub fn dim(&mut self, toward: Rgb, amount: f32) {
        for c in &mut self.cells {
            c.style.fg = c.style.fg.mix(toward, amount);
            c.style.bg = c.style.bg.mix(toward, amount * 0.6);
            c.style.attrs &= !BOLD;
        }
    }

    /// Returns the plain text of row `y` (for tests).
    pub fn row_text(&self, y: usize) -> String {
        let mut s = String::new();
        for x in 0..self.w {
            let c = self.cell(x, y);
            if !c.cont {
                s.push(c.ch);
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ST: Style = Style::new(Rgb(255, 255, 255), Rgb(0, 0, 0));

    #[test]
    fn widths_and_truncation() {
        assert_eq!(str_width("abc"), 3);
        assert_eq!(str_width("中文"), 4);
        assert_eq!(str_width("\u{f2db} x"), 3);
        assert_eq!(truncate("hello world", 6), "hello…");
        assert_eq!(truncate_left("/a/b/c/d", 5), "…/c/d");
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad_left("ab", 4), "  ab");
    }

    #[test]
    fn text_clips_and_handles_wide_chars() {
        let mut b = Buffer::new(6, 1, ST);
        let end = b.text(0, 0, "ab中cd", ST, 5);
        assert_eq!(end, 5);
        assert_eq!(b.row_text(0), "ab中c ");
        assert!(b.cell(3, 0).cont);
    }

    #[test]
    fn border_draws_corners() {
        let mut b = Buffer::new(4, 3, ST);
        let area = b.area();
        b.border(area, ROUNDED, ST);
        assert_eq!(b.row_text(0), "╭──╮");
        assert_eq!(b.row_text(1), "│  │");
        assert_eq!(b.row_text(2), "╰──╯");
    }

    #[test]
    fn wrap_breaks_on_words() {
        assert_eq!(wrap("aa bb cc", 5), vec!["aa bb", "cc"]);
        assert_eq!(wrap("abcdefg", 3), vec!["abc", "def", "g"]);
    }

    #[test]
    fn columns_cover_full_width() {
        let r = Rect::new(0, 0, 100, 1);
        let cols = r.columns(&[1, 1, 1], 1);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[2].right(), 100);
    }
}
