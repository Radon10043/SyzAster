//! Braille line charts (2x4 dots per cell).

use super::buffer::{str_width, Buffer, Rect, Rgb, Style};
use super::theme;

pub struct Series<'a> {
    pub xs: &'a [f64],
    pub ys: &'a [f64],
    pub color: Rgb,
    /// Shade the area below the line.
    pub fill: bool,
}

pub struct ChartOpts {
    pub x_range: (f64, f64),
    /// Start the y axis at zero instead of the visible minimum.
    pub y_zero: bool,
    /// Draw axis labels; sparkline mode when false.
    pub axes: bool,
    /// Formats an x value given the visible span.
    pub x_fmt: fn(f64, f64) -> String,
}

const DOT_BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

struct Canvas {
    cols: usize,
    rows: usize,
    line: Vec<u8>,
    fill: Vec<u8>,
    color: Vec<Option<Rgb>>,
    fill_color: Vec<Option<Rgb>>,
}

impl Canvas {
    fn new(cols: usize, rows: usize) -> Self {
        let n = cols * rows;
        Canvas {
            cols,
            rows,
            line: vec![0; n],
            fill: vec![0; n],
            color: vec![None; n],
            fill_color: vec![None; n],
        }
    }

    fn px_w(&self) -> i64 {
        (self.cols * 2) as i64
    }

    fn px_h(&self) -> i64 {
        (self.rows * 4) as i64
    }

    fn dot(&mut self, px: i64, py: i64, color: Rgb, fill: bool) {
        if px < 0 || py < 0 || px >= self.px_w() || py >= self.px_h() {
            return;
        }
        let (cx, cy) = ((px / 2) as usize, (py / 4) as usize);
        let bit = DOT_BITS[(px % 2) as usize][(py % 4) as usize];
        let i = cy * self.cols + cx;
        if fill {
            self.fill[i] |= bit;
            self.fill_color[i] = Some(color);
        } else {
            self.line[i] |= bit;
            self.color[i] = Some(color);
        }
    }

    fn segment(&mut self, (x0, y0): (i64, i64), (x1, y1): (i64, i64), color: Rgb, top: &mut [i64]) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.dot(x, y, color, false);
            if x >= 0 && (x as usize) < top.len() {
                top[x as usize] = top[x as usize].min(y);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }
}

/// Value of a series at `x` by linear interpolation (NaN if out of range).
pub fn value_at(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let n = xs.len().min(ys.len());
    if n == 0 {
        return f64::NAN;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let i = xs[..n].partition_point(|&v| v <= x);
    let (xa, xb, ya, yb) = (xs[i - 1], xs[i], ys[i - 1], ys[i]);
    if (xb - xa).abs() < f64::EPSILON || !ya.is_finite() || !yb.is_finite() {
        return ya;
    }
    ya + (yb - ya) * (x - xa) / (xb - xa)
}

/// Visible y range over all series within the x range.
fn y_bounds(series: &[Series], x0: f64, x1: f64, zero: bool) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for s in series {
        for (i, (&x, &y)) in s.xs.iter().zip(s.ys.iter()).enumerate() {
            let next_in = s.xs.get(i + 1).is_some_and(|&nx| nx >= x0);
            if !y.is_finite() || x > x1 || (x < x0 && !next_in) {
                continue;
            }
            let y = if x < x0 { value_at(s.xs, s.ys, x0) } else { y };
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    if !lo.is_finite() {
        return None;
    }
    if zero {
        lo = lo.min(0.0);
    }
    if (hi - lo).abs() < 1e-9 {
        // Flat series: center the line instead of pinning it to an edge.
        let d = if lo.abs() > 10.0 {
            lo.abs() * 0.02
        } else {
            1.0
        };
        if zero && lo == 0.0 {
            hi = 2.0 * d;
        } else {
            lo -= d;
            hi += d;
        }
    }
    Some((lo, hi))
}

/// Rounds a step up to 1, 2, 2.5 or 5 times a power of ten.
fn nice_step(raw: f64) -> f64 {
    if raw <= 0.0 || !raw.is_finite() {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    let n = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 2.5 {
        2.5
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    n * mag
}

/// Round axis bounds and step covering `[lo, hi]` with about `n` ticks.
pub fn nice_ticks(lo: f64, hi: f64, n: usize) -> (f64, f64, f64) {
    let step = nice_step((hi - lo) / (n.max(2) - 1) as f64);
    let lo2 = (lo / step).floor() * step;
    let mut hi2 = (hi / step).ceil() * step;
    if hi2 <= lo2 {
        hi2 = lo2 + step;
    }
    (lo2, hi2, step)
}

/// Formats a tick so that neighbouring ticks never render identically.
pub fn tick_label(v: f64, step: f64, max_abs: f64) -> String {
    let (unit, suffix) = if max_abs >= 1e9 {
        (1e9, "G")
    } else if max_abs >= 1e6 {
        (1e6, "M")
    } else if max_abs >= 1e4 {
        (1e3, "K")
    } else {
        (1.0, "")
    };
    // Fewest decimals that represent the step exactly (2.5 needs one).
    let rel = step / unit;
    let decimals = (0..3)
        .find(|&d| {
            let scaled = rel * 10f64.powi(d as i32);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .unwrap_or(3);
    if v.abs() < step * 1e-6 {
        return "0".to_string();
    }
    format!("{:.*}{}", decimals, v / unit, suffix)
}

/// Draws a line chart into `r` (no border).
pub fn draw(buf: &mut Buffer, r: Rect, series: &[Series], opts: &ChartOpts) {
    if r.w < 4 || r.h < 2 {
        return;
    }
    let (x0, x1) = opts.x_range;
    let bounds = y_bounds(series, x0, x1, opts.y_zero);
    let Some((ylo, yhi)) = bounds else {
        let msg = "awaiting samples";
        let mr = r.centered(str_width(msg).min(r.w), 1);
        buf.text(
            mr.x,
            mr.y,
            msg,
            theme::st(theme::SUBTLE).italic(),
            r.right(),
        );
        return;
    };

    let grid_st = theme::st(theme::GRID);
    let axis_st = theme::st(theme::SUBTLE);
    let label_st = theme::st(theme::MUTED);

    // Layout: [y labels][axis] plot / x axis / x labels.
    let (plot, label_rows, ylo, yhi) = if opts.axes && r.h >= 5 {
        let ph = r.h - 2;
        let (lo, hi, step) = nice_ticks(ylo, yhi, (ph / 3).clamp(2, 6));
        let max_abs = lo.abs().max(hi.abs());
        let n = ((hi - lo) / step).round() as usize;
        let mut ticks: Vec<(usize, String)> = Vec::new();
        for k in 0..=n {
            let v = hi - step * k as f64;
            let row = (((hi - v) / (hi - lo)) * (ph - 1) as f64).round() as usize;
            if ticks.last().is_none_or(|(r0, _)| row >= r0 + 2) || k == n {
                if k == n && ticks.last().is_some_and(|(r0, _)| row < r0 + 2) {
                    ticks.pop();
                }
                ticks.push((row, tick_label(v, step, max_abs)));
            }
        }
        let lw = ticks.iter().map(|(_, s)| str_width(s)).max().unwrap_or(1);
        let plot = Rect::new(r.x + lw + 2, r.y, r.w.saturating_sub(lw + 2), ph);
        for (row, label) in &ticks {
            buf.text_right(r.x + lw, plot.y + row, label, label_st);
            buf.set(r.x + lw + 1, plot.y + row, '┤', axis_st);
            for x in plot.x..plot.right() {
                buf.set(x, plot.y + row, '┈', grid_st);
            }
        }
        for row in 0..plot.h {
            if !ticks.iter().any(|(r0, _)| *r0 == row) {
                buf.set(r.x + lw + 1, plot.y + row, '│', axis_st);
            }
        }
        buf.set(r.x + lw + 1, plot.bottom(), '╰', axis_st);
        buf.hline(plot.x, plot.bottom(), plot.w, '─', axis_st);
        (plot, Some(plot.bottom() + 1), lo, hi)
    } else {
        (r, None, ylo, yhi + (yhi - ylo) * 0.08)
    };
    if plot.w == 0 || plot.h == 0 {
        return;
    }

    let mut canvas = Canvas::new(plot.w, plot.h);
    let (pw, ph) = (canvas.px_w(), canvas.px_h());
    let span = (x1 - x0).max(1e-9);
    let map = |x: f64, y: f64| -> (i64, i64) {
        let px = ((x - x0) / span * (pw - 1) as f64).round() as i64;
        let py = ((1.0 - (y - ylo) / (yhi - ylo)) * (ph - 1) as f64).round() as i64;
        (px, py)
    };
    for s in series {
        let mut top = vec![i64::MAX; pw as usize];
        let mut prev: Option<(i64, i64)> = None;
        let n = s.xs.len().min(s.ys.len());
        for i in 0..n {
            let (mut x, mut y) = (s.xs[i], s.ys[i]);
            if !y.is_finite() {
                prev = None;
                continue;
            }
            if x < x0 {
                // Clip the segment entering the window from the left.
                if i + 1 < n && s.xs[i + 1] > x0 {
                    y = value_at(s.xs, s.ys, x0);
                    x = x0;
                } else {
                    continue;
                }
            }
            if x > x1 {
                break;
            }
            let p = map(x, y);
            match prev {
                Some(q) => canvas.segment(q, p, s.color, &mut top),
                None => canvas.segment(p, p, s.color, &mut top),
            }
            prev = Some(p);
        }
        if s.fill {
            for (px, &t) in top.iter().enumerate() {
                if t == i64::MAX {
                    continue;
                }
                for py in (t + 1)..ph {
                    if (py + px as i64) % 2 == 0 {
                        canvas.dot(px as i64, py, s.color, true);
                    }
                }
            }
        }
    }

    for cy in 0..plot.h {
        for cx in 0..plot.w {
            let i = cy * plot.w + cx;
            let (bits, color) = if canvas.line[i] != 0 {
                (
                    canvas.line[i] | canvas.fill[i],
                    canvas.color[i].unwrap_or(theme::TEXT),
                )
            } else if canvas.fill[i] != 0 {
                let c = canvas.fill_color[i].unwrap_or(theme::TEXT);
                (canvas.fill[i], c.mix(theme::BG, 0.72))
            } else {
                continue;
            };
            let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
            buf.set(plot.x + cx, plot.y + cy, ch, Style::new(color, theme::BG));
        }
    }

    if let Some(ly) = label_rows {
        let n = if plot.w >= 60 {
            5
        } else if plot.w >= 30 {
            3
        } else {
            2
        };
        let mut last_end = 0usize;
        for i in 0..n {
            let frac = i as f64 / (n - 1) as f64;
            let label = (opts.x_fmt)(x0 + (x1 - x0) * frac, x1 - x0);
            let lw = str_width(&label);
            let center = plot.x + ((plot.w - 1) as f64 * frac).round() as usize;
            let start = if i == 0 {
                plot.x
            } else if i == n - 1 {
                plot.right().saturating_sub(lw)
            } else {
                center.saturating_sub(lw / 2)
            };
            if i > 0 && start <= last_end {
                continue;
            }
            buf.text(start, ly, &label, label_st, r.right());
            last_end = start + lw;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_values() {
        let xs = [0.0, 10.0, 20.0];
        let ys = [0.0, 100.0, 100.0];
        assert_eq!(value_at(&xs, &ys, 5.0), 50.0);
        assert_eq!(value_at(&xs, &ys, -1.0), 0.0);
        assert_eq!(value_at(&xs, &ys, 30.0), 100.0);
    }

    #[test]
    fn nice_axis_ticks() {
        assert_eq!(nice_ticks(0.0, 37.0, 3), (0.0, 40.0, 20.0));
        let (lo, hi, step) = nice_ticks(34700.0, 34832.0, 4);
        assert_eq!((lo, hi, step), (34700.0, 34850.0, 50.0));
        assert_eq!(tick_label(34750.0, step, hi), "34.75K");
        assert_eq!(tick_label(20.0, 20.0, 40.0), "20");
        assert_eq!(tick_label(0.0, 20000.0, 40000.0), "0");
        assert_eq!(tick_label(180e6, 2e6, 190e6), "180M");
        assert_eq!(tick_label(0.5, 0.25, 1.0), "0.50");
    }

    #[test]
    fn draws_braille_line() {
        let mut b = Buffer::new(20, 6, theme::BASE);
        let xs = [0.0, 1.0, 2.0];
        let ys = [0.0, 5.0, 10.0];
        let s = [Series {
            xs: &xs,
            ys: &ys,
            color: theme::CYAN,
            fill: false,
        }];
        let opts = ChartOpts {
            x_range: (0.0, 2.0),
            y_zero: true,
            axes: false,
            x_fmt: |x, _| format!("{}", x),
        };
        let area = b.area();
        draw(&mut b, area, &s, &opts);
        let has_braille = (0..6).any(|y| {
            b.row_text(y)
                .chars()
                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        });
        assert!(has_braille);
    }
}
