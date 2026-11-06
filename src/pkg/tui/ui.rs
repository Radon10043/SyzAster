//! Shared widgets: banner, tab bar, panels, item lists, footer and modals.

use super::buffer::{
    pad, span, str_width, truncate, truncate_left, wrap, Buffer, Rect, Rgb, Span, Style, HEAVY,
    ROUNDED,
};
use super::term::Key;
use super::theme::{self, icons, st};
use super::util;
use std::path::{Path, PathBuf};

const LOGO: [&str; 2] = [
    "█▀ █▄█ ▀█ ▄▀█ █▀ ▀█▀ █▀▀ █▀█",
    "▄█  █  █▄ █▀█ ▄█  █  ██▄ █▀▄",
];

pub const BANNER_H: usize = 4;

/// Fixed banner: gradient logo and a one-line description.
pub fn banner(buf: &mut Buffer, r: Rect, now: i64) {
    let border = st(theme::BORDER);
    buf.border(Rect::new(r.x, r.y, r.w, BANNER_H), ROUNDED, border);
    let logo_w = str_width(LOGO[0]);
    for (row, line) in LOGO.iter().enumerate() {
        for (i, ch) in line.chars().enumerate() {
            let t = i as f32 / logo_w as f32;
            let c = theme::CYAN.mix(theme::MAGENTA, t);
            buf.set(
                r.x + 2 + i,
                r.y + 1 + row,
                ch,
                Style::new(c, theme::BG).bold(),
            );
        }
    }
    let tx = r.x + 2 + logo_w + 3;
    let max = r.right().saturating_sub(2);
    let avail = max.saturating_sub(tx);
    // Pick the longest description that fits the banner.
    let taglines = [
        "Coverage-guided kernel fuzzing for the Asterinas framekernel.",
        "Coverage-guided fuzzing for the Asterinas framekernel.",
        "Coverage-guided fuzzing for Asterinas.",
        "Kernel fuzzing for Asterinas.",
    ];
    let tag = taglines
        .iter()
        .find(|t| str_width(t) + 3 <= avail)
        .copied()
        .unwrap_or(taglines[taglines.len() - 1]);
    buf.spans(
        tx,
        r.y + 1,
        &[
            span("// ", st(theme::MAGENTA).bold()),
            span(tag, st(theme::WHITE).bold()),
        ],
        max,
    );
    let clock = util::clock(now);
    let full = format!("syzkaller + framekernel-selective KCOV  ·  {}", clock);
    let short = clock;
    let meta = if str_width(&full) + 5 <= avail {
        full
    } else {
        short
    };
    buf.spans(
        tx,
        r.y + 2,
        &[
            span("   ", st(theme::MUTED)),
            span(format!("{} ", icons().term), st(theme::GREEN)),
            span(meta, st(theme::MUTED)),
        ],
        max,
    );
}

/// One tab or sub-tab entry.
pub struct TabSpec<'a> {
    pub key: &'a str,
    pub icon: &'a str,
    pub label: &'a str,
}

/// Draws a row of tabs; the active one is a highlighted pill.
pub fn tabs(
    buf: &mut Buffer,
    x: usize,
    y: usize,
    max_x: usize,
    specs: &[TabSpec],
    active: usize,
    focused: bool,
) -> usize {
    let ic = icons();
    let mut cx = x;
    for (i, t) in specs.iter().enumerate() {
        let on = i == active;
        let bg = if on { theme::BG_TAB } else { theme::BG };
        let cap = Style::new(theme::BG_TAB, theme::BG);
        let label_st = if on {
            Style::new(if focused { theme::CYAN } else { theme::TEXT }, bg).bold()
        } else {
            Style::new(theme::MUTED, bg)
        };
        let icon_st = Style::new(if on { theme::CYAN } else { theme::SUBTLE }, bg);
        let key_st = Style::new(theme::YELLOW, bg).bold();
        if on {
            cx = buf.text(cx, y, ic.cap_l, cap, max_x);
        } else {
            cx = buf.text(cx, y, " ", Style::new(theme::BG, theme::BG), max_x);
        }
        cx = buf.spans(
            cx,
            y,
            &[
                span(format!("{} ", t.icon), icon_st),
                span(format!("[{}]", t.key), key_st),
                span(format!(" {}", t.label), label_st),
            ],
            max_x,
        );
        if on {
            cx = buf.text(cx, y, ic.cap_r, cap, max_x);
        } else {
            cx = buf.text(cx, y, " ", Style::new(theme::BG, theme::BG), max_x);
        }
        cx = buf.text(cx, y, " ", theme::BASE, max_x);
    }
    cx
}

/// Bordered panel with an icon title; returns the inner area.
pub fn panel(buf: &mut Buffer, r: Rect, icon: &str, title: &str, focused: bool) -> Rect {
    let bst = st(if focused {
        theme::BORDER_HI
    } else {
        theme::BORDER
    });
    buf.border(r, ROUNDED, bst);
    let tst = if focused {
        Style::new(theme::CYAN, theme::BG).bold()
    } else {
        Style::new(theme::TEXT, theme::BG).bold()
    };
    let title = truncate(title, r.w.saturating_sub(8));
    buf.spans(
        r.x + 2,
        r.y,
        &[
            span(" ", tst),
            span(
                format!("{} ", icon),
                st(if focused {
                    theme::MAGENTA
                } else {
                    theme::MUTED
                }),
            ),
            span(title, tst),
            span(" ", tst),
        ],
        r.right().saturating_sub(1),
    );
    r.inset(1, 1)
}

/// Right-aligned text embedded in a panel's top border.
/// Skipped when it would overlap the panel title.
pub fn panel_note(buf: &mut Buffer, r: Rect, spans: &[Span]) {
    if r.w < 8 || r.y >= buf.height() {
        return;
    }
    // The title ends where the plain border line resumes.
    let mut title_end = r.x + 2;
    for x in (r.x + 3)..r.right().saturating_sub(1) {
        if buf.cell(x, r.y).ch == '─' && buf.cell(x - 1, r.y).ch == ' ' {
            title_end = x;
            break;
        }
    }
    let w = Buffer::spans_width(spans);
    let x = r.right().saturating_sub(w + 3);
    if x <= title_end {
        return;
    }
    buf.spans(x, r.y, spans, r.right().saturating_sub(2));
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemKind {
    Section,
    Path,
    Choice,
    Toggle,
    Number,
    Action,
    Info,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: &'static str,
    pub kind: ItemKind,
    pub icon: &'static str,
    pub label: String,
    pub value: String,
    pub on: bool,
    pub badge: Option<Span>,
    pub help: String,
    pub enabled: bool,
}

impl Item {
    pub fn new(id: &'static str, kind: ItemKind, label: impl Into<String>) -> Self {
        Item {
            id,
            kind,
            icon: "",
            label: label.into(),
            value: String::new(),
            on: false,
            badge: None,
            help: String::new(),
            enabled: true,
        }
    }

    pub fn section(label: impl Into<String>) -> Self {
        Item::new("", ItemKind::Section, label)
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = icon;
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub fn badge(mut self, b: Option<Span>) -> Self {
        self.badge = b;
        self
    }

    pub fn help(mut self, h: impl Into<String>) -> Self {
        self.help = h.into();
        self
    }

    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }

    pub fn selectable(&self) -> bool {
        self.kind != ItemKind::Section
    }
}

/// Selection state for an item list.
#[derive(Default, Clone, Copy)]
pub struct ListState {
    pub sel: usize,
    pub scroll: usize,
}

impl ListState {
    pub fn fix(&mut self, items: &[Item]) {
        if items.is_empty() {
            self.sel = 0;
            return;
        }
        self.sel = self.sel.min(items.len() - 1);
        if !items[self.sel].selectable() {
            self.step(items, 1);
        }
    }

    pub fn step(&mut self, items: &[Item], delta: i32) {
        if items.is_empty() {
            return;
        }
        let n = items.len() as i32;
        let mut i = self.sel as i32;
        for _ in 0..n {
            i += delta.signum();
            if i < 0 || i >= n {
                return;
            }
            if items[i as usize].selectable() {
                self.sel = i as usize;
                return;
            }
        }
    }

    pub fn home(&mut self, items: &[Item]) {
        self.sel = 0;
        self.fix(items);
    }

    pub fn end(&mut self, items: &[Item]) {
        if let Some(i) = items.iter().rposition(|it| it.selectable()) {
            self.sel = i;
        }
    }

    /// Handles common navigation keys; returns true if consumed.
    pub fn nav(&mut self, items: &[Item], key: Key) -> bool {
        match key {
            Key::Up | Key::Char('k') => self.step(items, -1),
            Key::Down | Key::Char('j') => self.step(items, 1),
            Key::Home | Key::Char('g') => self.home(items),
            Key::End | Key::Char('G') => self.end(items),
            Key::PageUp => {
                for _ in 0..8 {
                    self.step(items, -1);
                }
            }
            Key::PageDown => {
                for _ in 0..8 {
                    self.step(items, 1);
                }
            }
            _ => return false,
        }
        true
    }
}

/// Renders an item list with labels in a fixed-width column.
pub fn item_list(
    buf: &mut Buffer,
    r: Rect,
    items: &[Item],
    state: &mut ListState,
    focused: bool,
    label_w: usize,
) {
    if r.h == 0 || r.w < 10 {
        return;
    }
    let ic = icons();
    if state.sel < state.scroll {
        state.scroll = state.sel.saturating_sub(1);
    }
    if state.sel >= state.scroll + r.h {
        state.scroll = state.sel + 1 - r.h;
    }
    state.scroll = state.scroll.min(items.len().saturating_sub(r.h));
    for (row, (idx, it)) in items
        .iter()
        .enumerate()
        .skip(state.scroll)
        .take(r.h)
        .enumerate()
    {
        let y = r.y + row;
        let selected = idx == state.sel;
        let bg = if selected && focused {
            theme::BG_SEL
        } else if selected {
            theme::BG_ALT
        } else {
            theme::BG
        };
        if it.kind == ItemKind::Section {
            let label = format!(" {} ", it.label.to_uppercase());
            let x = buf.text(r.x + 1, y, "──", st(theme::SUBTLE), r.right());
            let x = buf.text(x, y, &label, st(theme::MAGENTA).bold(), r.right());
            let rest = r.right().saturating_sub(x + 1);
            buf.hline(x, y, rest, '─', st(theme::SUBTLE));
            continue;
        }
        buf.fill(Rect::new(r.x, y, r.w, 1), ' ', Style::new(theme::TEXT, bg));
        if selected {
            buf.set(
                r.x,
                y,
                '▌',
                Style::new(if focused { theme::CYAN } else { theme::SUBTLE }, bg),
            );
        }
        let fg = |c| Style::new(if it.enabled { c } else { theme::SUBTLE }, bg);
        let mut x = r.x + 2;
        x = buf.text(
            x,
            y,
            &format!("{} ", it.icon),
            fg(if selected { theme::CYAN } else { theme::MUTED }),
            r.right(),
        );
        let label_st = if selected && focused {
            fg(theme::WHITE).bold()
        } else {
            fg(theme::TEXT)
        };
        let max = r.right().saturating_sub(1);
        if it.kind == ItemKind::Action {
            let st_ = if selected && focused {
                fg(theme::CYAN).bold()
            } else {
                fg(theme::CYAN)
            };
            x = buf.text(x, y, &it.label, st_, max);
            if !it.value.is_empty() {
                buf.text(x + 2, y, &it.value, fg(theme::MUTED), max);
            }
        } else {
            buf.text(x, y, &pad(&it.label, label_w), label_st, max);
            x += label_w + 1;
            let badge_w = it.badge.as_ref().map_or(0, |(s, _)| str_width(s) + 1);
            let vmax = max.saturating_sub(badge_w);
            let avail = vmax.saturating_sub(x);
            match it.kind {
                ItemKind::Toggle => {
                    let (icon, txt, c) = if it.on {
                        (ic.on, "ON ", theme::GREEN)
                    } else {
                        (ic.off, "OFF", theme::MUTED)
                    };
                    buf.spans(
                        x,
                        y,
                        &[span(format!("{} ", icon), fg(c)), span(txt, fg(c).bold())],
                        vmax,
                    );
                    if !it.value.is_empty() {
                        buf.text(x + 7, y, &it.value, fg(theme::MUTED), vmax);
                    }
                }
                ItemKind::Choice => {
                    buf.spans(
                        x,
                        y,
                        &[
                            span("‹ ", fg(theme::SUBTLE)),
                            span(it.value.clone(), fg(theme::YELLOW).bold()),
                            span(" ›", fg(theme::SUBTLE)),
                        ],
                        vmax,
                    );
                }
                ItemKind::Number => {
                    buf.text(x, y, &it.value, fg(theme::YELLOW).bold(), vmax);
                }
                ItemKind::Path => {
                    buf.text(
                        x,
                        y,
                        &truncate_left(&it.value, avail),
                        fg(theme::TEXT),
                        vmax,
                    );
                }
                _ => {
                    buf.text(x, y, &truncate(&it.value, avail), fg(theme::MUTED), vmax);
                }
            }
            if let Some((b, bst)) = &it.badge {
                buf.text_right(max, y, b, Style::new(bst.fg, bg));
            }
        }
    }
    // Scroll indicators.
    if state.scroll > 0 {
        buf.set(r.right() - 1, r.y, '▲', st(theme::SUBTLE));
    }
    if state.scroll + r.h < items.len() {
        buf.set(r.right() - 1, r.bottom() - 1, '▼', st(theme::SUBTLE));
    }
}

/// Wrapped help text for the selected item.
pub fn help_box(buf: &mut Buffer, r: Rect, text: &str) {
    if r.h == 0 {
        return;
    }
    buf.hline(r.x, r.y, r.w, '┄', st(theme::SUBTLE));
    let lines = wrap(text, r.w.saturating_sub(4));
    for (i, line) in lines.iter().take(r.h.saturating_sub(1)).enumerate() {
        let prefix = if i == 0 {
            format!("{} ", icons().info)
        } else {
            "  ".to_string()
        };
        buf.spans(
            r.x + 1,
            r.y + 1 + i,
            &[
                span(prefix, st(theme::BLUE)),
                span(line.clone(), st(theme::MUTED).italic()),
            ],
            r.right(),
        );
    }
}

/// Key hint bar: `key` badges followed by their action.
pub fn footer(
    buf: &mut Buffer,
    y: usize,
    w: usize,
    hints: &[(&str, &str)],
    global: &[(&str, &str)],
) {
    buf.fill(
        Rect::new(0, y, w, 1),
        ' ',
        Style::new(theme::TEXT, theme::BG_ALT),
    );
    let key_st = Style::new(theme::BG, theme::YELLOW).bold();
    let gkey_st = Style::new(theme::BG, theme::CYAN).bold();
    let txt_st = Style::new(theme::TEXT, theme::BG_ALT);
    let render = |hs: &[(&str, &str)], kst: Style| -> Vec<Span> {
        let mut v = Vec::new();
        for (k, a) in hs {
            v.push(span(format!(" {} ", k), kst));
            v.push(span(format!(" {}  ", a), txt_st));
        }
        v
    };
    let g = render(global, gkey_st);
    let gw = Buffer::spans_width(&g);
    let gx = w.saturating_sub(gw);
    let mut x = 1;
    for pair in hints.chunks(1) {
        let s = render(pair, key_st);
        let sw = Buffer::spans_width(&s);
        if x + sw > gx {
            break;
        }
        x = buf.spans(x, y, &s, gx);
    }
    buf.spans(gx, y, &g, w);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

impl Level {
    pub fn color(self) -> Rgb {
        match self {
            Level::Info => theme::BLUE,
            Level::Ok => theme::GREEN,
            Level::Warn => theme::YELLOW,
            Level::Error => theme::RED,
        }
    }

    pub fn icon(self) -> &'static str {
        let ic = icons();
        match self {
            Level::Info => ic.info,
            Level::Ok => ic.ok,
            Level::Warn => ic.warn,
            Level::Error => ic.fail,
        }
    }
}

/// Status line: last message on the left, context on the right.
pub fn status_line(
    buf: &mut Buffer,
    y: usize,
    w: usize,
    msg: Option<&(Level, String)>,
    right: &[Span],
) {
    buf.fill(Rect::new(0, y, w, 1), ' ', theme::BASE);
    let rw = Buffer::spans_width(right);
    let rx = w.saturating_sub(rw + 1);
    buf.spans(rx, y, right, w);
    if let Some((lvl, text)) = msg {
        let c = lvl.color();
        buf.spans(
            1,
            y,
            &[
                span(format!("{} ", lvl.icon()), st(c)),
                span(truncate(text, rx.saturating_sub(5)), st(c)),
            ],
            rx.saturating_sub(1),
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputKind {
    Path,
    Number,
    Text,
}

/// Single-line text editor shown in a modal.
#[derive(Clone, Debug)]
pub struct Input {
    pub title: String,
    pub label: String,
    pub kind: InputKind,
    pub chars: Vec<char>,
    pub cursor: usize,
    pub candidates: Vec<String>,
    pub base: PathBuf,
    pub error: Option<String>,
}

pub enum InputEvent {
    Pending,
    Submit(String),
    Cancel,
}

impl Input {
    pub fn new(
        title: impl Into<String>,
        label: impl Into<String>,
        kind: InputKind,
        value: &str,
        base: &Path,
    ) -> Self {
        let chars: Vec<char> = value.chars().collect();
        Input {
            title: title.into(),
            label: label.into(),
            kind,
            cursor: chars.len(),
            chars,
            candidates: Vec::new(),
            base: base.to_path_buf(),
            error: None,
        }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn handle(&mut self, key: Key) -> InputEvent {
        if !matches!(key, Key::Tab) {
            self.candidates.clear();
        }
        match key {
            Key::Enter => {
                let v = self.value();
                if self.kind == InputKind::Number && v.trim().parse::<u64>().is_err() {
                    self.error = Some("expected a non-negative integer".to_string());
                    return InputEvent::Pending;
                }
                return InputEvent::Submit(v);
            }
            Key::Esc | Key::Ctrl('c') | Key::Ctrl('g') => return InputEvent::Cancel,
            Key::Char(c) => {
                if self.kind == InputKind::Number && !c.is_ascii_digit() {
                    self.error = Some("digits only".to_string());
                    return InputEvent::Pending;
                }
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            Key::Backspace | Key::Ctrl('h') => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.chars.remove(self.cursor);
                }
            }
            Key::Delete | Key::Ctrl('d') => {
                if self.cursor < self.chars.len() {
                    self.chars.remove(self.cursor);
                }
            }
            Key::Left | Key::Ctrl('b') => self.cursor = self.cursor.saturating_sub(1),
            Key::Right | Key::Ctrl('f') => self.cursor = (self.cursor + 1).min(self.chars.len()),
            Key::Home | Key::Ctrl('a') => self.cursor = 0,
            Key::End | Key::Ctrl('e') => self.cursor = self.chars.len(),
            Key::Ctrl('u') => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            Key::Ctrl('k') => {
                self.chars.truncate(self.cursor);
            }
            Key::Ctrl('w') => {
                let mut i = self.cursor;
                while i > 0 && self.chars[i - 1] == '/' {
                    i -= 1;
                }
                while i > 0 && self.chars[i - 1] != '/' && self.chars[i - 1] != ' ' {
                    i -= 1;
                }
                self.chars.drain(i..self.cursor);
                self.cursor = i;
            }
            Key::Tab if self.kind == InputKind::Path => {
                let head: String = self.chars[..self.cursor].iter().collect();
                let tail: String = self.chars[self.cursor..].iter().collect();
                let (done, cands) = util::complete_path(&head, &self.base);
                self.chars = format!("{}{}", done, tail).chars().collect();
                self.cursor = done.chars().count();
                self.candidates = if cands.len() > 1 { cands } else { Vec::new() };
            }
            _ => {}
        }
        self.error = None;
        InputEvent::Pending
    }
}

pub fn modal_frame(
    buf: &mut Buffer,
    w: usize,
    h: usize,
    accent: Rgb,
    title: &str,
    icon: &str,
) -> Rect {
    buf.dim(theme::BG, 0.62);
    let area = buf.area();
    let r = area.centered(
        w.min(area.w.saturating_sub(4)),
        h.min(area.h.saturating_sub(2)),
    );
    // Drop shadow.
    let shadow = Rgb(0x02, 0x03, 0x05);
    buf.tint_bg(Rect::new(r.x + 2, r.bottom(), r.w, 1).clip(area), shadow);
    buf.tint_bg(Rect::new(r.right(), r.y + 1, 2, r.h).clip(area), shadow);
    buf.fill(r, ' ', Style::new(theme::TEXT, theme::BG_MODAL));
    buf.border(r, HEAVY, Style::new(accent, theme::BG_MODAL));
    let tst = Style::new(theme::BG, accent).bold();
    buf.spans(
        r.x + 2,
        r.y,
        &[span(format!(" {} {} ", icon, title), tst)],
        r.right().saturating_sub(2),
    );
    r.inset(2, 1)
}

/// Text input modal; the rest of the screen is dimmed.
pub fn draw_input(buf: &mut Buffer, m: &Input) {
    let ic = icons();
    let extra = if m.candidates.is_empty() { 0 } else { 2 };
    let inner = modal_frame(buf, 84, 10 + extra, theme::CYAN, &m.title, ic.edit);
    let bg = theme::BG_MODAL;
    buf.text(
        inner.x,
        inner.y,
        &m.label,
        Style::new(theme::TEXT, bg),
        inner.right(),
    );
    // Field.
    let fy = inner.y + 2;
    let field = Rect::new(inner.x, fy, inner.w, 1);
    buf.fill(field, ' ', Style::new(theme::WHITE, theme::BG_INPUT));
    let prompt_w = 2;
    buf.text(
        field.x,
        fy,
        "❯ ",
        Style::new(theme::MAGENTA, theme::BG_INPUT).bold(),
        field.right(),
    );
    let view_w = field.w.saturating_sub(prompt_w + 1);
    let start = m.cursor.saturating_sub(view_w.saturating_sub(1));
    let mut x = field.x + prompt_w;
    for (i, c) in m.chars.iter().enumerate().skip(start) {
        if x >= field.right() - 1 {
            break;
        }
        let stl = if i == m.cursor {
            Style::new(theme::BG, theme::CYAN)
        } else {
            Style::new(theme::WHITE, theme::BG_INPUT)
        };
        x += buf.set(x, fy, *c, stl).max(1);
    }
    if m.cursor >= m.chars.len() && x < field.right() {
        buf.set(x, fy, ' ', Style::new(theme::BG, theme::CYAN));
    }
    // Validation line.
    let vy = fy + 2;
    if let Some(err) = &m.error {
        buf.text(
            inner.x,
            vy,
            &format!("{} {}", ic.fail, err),
            Style::new(theme::RED, bg),
            inner.right(),
        );
    } else if m.kind == InputKind::Path {
        let p = util::expand(&m.value(), &m.base);
        let (txt, c) = if p.is_dir() {
            (format!("{} directory exists", ic.ok), theme::GREEN)
        } else if p.is_file() {
            (format!("{} file exists", ic.ok), theme::GREEN)
        } else {
            (format!("{} does not exist yet", ic.warn), theme::YELLOW)
        };
        buf.spans(
            inner.x,
            vy,
            &[
                span(txt, Style::new(c, bg)),
                span(
                    format!(
                        "   {}",
                        truncate_left(&p.display().to_string(), inner.w.saturating_sub(24))
                    ),
                    Style::new(theme::MUTED, bg),
                ),
            ],
            inner.right(),
        );
    }
    if !m.candidates.is_empty() {
        let text = m.candidates.join("  ");
        for (i, line) in wrap(&text, inner.w).iter().take(2).enumerate() {
            buf.text(
                inner.x,
                vy + 1 + i,
                line,
                Style::new(theme::BLUE, bg),
                inner.right(),
            );
        }
    }
    let hy = inner.bottom().saturating_sub(1);
    let mut hints: Vec<(&str, &str)> = vec![("Enter", "confirm"), ("Esc", "cancel")];
    if m.kind == InputKind::Path {
        hints.insert(0, ("Tab", "complete"));
    }
    hints.push(("^U", "clear"));
    hints.push(("^W", "delete word"));
    let mut x = inner.x;
    for (k, a) in hints {
        x = buf.spans(
            x,
            hy,
            &[
                span(
                    format!(" {} ", k),
                    Style::new(theme::BG, theme::YELLOW).bold(),
                ),
                span(format!(" {}   ", a), Style::new(theme::MUTED, bg)),
            ],
            inner.right(),
        );
    }
}

/// Yes/no confirmation or plain message.
#[derive(Clone, Debug)]
pub struct Dialog<A> {
    pub title: String,
    pub lines: Vec<Vec<Span>>,
    pub level: Level,
    pub yes: Option<String>,
    pub no: String,
    pub focus_yes: bool,
    pub action: Option<A>,
}

pub enum DialogEvent<A> {
    Pending,
    Confirm(A),
    Close,
}

impl<A: Clone> Dialog<A> {
    pub fn confirm(
        title: impl Into<String>,
        lines: Vec<Vec<Span>>,
        yes: impl Into<String>,
        action: A,
    ) -> Self {
        Dialog {
            title: title.into(),
            lines,
            level: Level::Warn,
            yes: Some(yes.into()),
            no: "Cancel".to_string(),
            focus_yes: true,
            action: Some(action),
        }
    }

    pub fn message(title: impl Into<String>, level: Level, lines: Vec<Vec<Span>>) -> Self {
        Dialog {
            title: title.into(),
            lines,
            level,
            yes: None,
            no: "Close".to_string(),
            focus_yes: false,
            action: None,
        }
    }

    pub fn level(mut self, l: Level) -> Self {
        self.level = l;
        self
    }

    pub fn handle(&mut self, key: Key) -> DialogEvent<A> {
        match key {
            Key::Esc | Key::Char('n') | Key::Char('q') => DialogEvent::Close,
            Key::Char('y') if self.yes.is_some() => match self.action.clone() {
                Some(a) => DialogEvent::Confirm(a),
                None => DialogEvent::Close,
            },
            Key::Left | Key::Right | Key::Tab | Key::BackTab | Key::Char('h') | Key::Char('l') => {
                if self.yes.is_some() {
                    self.focus_yes = !self.focus_yes;
                }
                DialogEvent::Pending
            }
            Key::Enter | Key::Char(' ') => {
                if self.focus_yes {
                    if let Some(a) = self.action.clone() {
                        return DialogEvent::Confirm(a);
                    }
                }
                DialogEvent::Close
            }
            _ => DialogEvent::Pending,
        }
    }
}

pub fn draw_dialog<A>(buf: &mut Buffer, d: &Dialog<A>) {
    let ic = icons();
    let accent = match d.level {
        Level::Error => theme::RED,
        Level::Warn => theme::YELLOW,
        Level::Ok => theme::GREEN,
        Level::Info => theme::CYAN,
    };
    let width = 78usize;
    let body_w = width - 6;
    // Wrap each logical line made of spans by flattening when too long.
    let mut rows: Vec<Vec<Span>> = Vec::new();
    for line in &d.lines {
        let w = Buffer::spans_width(line);
        let label_w = line.first().map_or(0, |(s, _)| str_width(s));
        if w <= body_w {
            rows.push(line.clone());
        } else if line.len() >= 2 && label_w < body_w / 3 {
            // Keep the leading label and wrap the rest in a hanging indent.
            let text: String = line[1..].iter().map(|(s, _)| s.as_str()).collect();
            let stl = line[1].1;
            for (i, l) in wrap(&text, body_w - label_w).into_iter().enumerate() {
                let head = if i == 0 {
                    line[0].clone()
                } else {
                    span(" ".repeat(label_w), theme::BASE)
                };
                rows.push(vec![head, span(l, stl)]);
            }
        } else {
            let text: String = line.iter().map(|(s, _)| s.as_str()).collect();
            let stl = line.first().map(|(_, s)| *s).unwrap_or(theme::BASE);
            for l in wrap(&text, body_w) {
                rows.push(vec![span(l, stl)]);
            }
        }
    }
    let h = rows.len() + 6;
    let icon = match d.level {
        Level::Error => ic.fail,
        Level::Warn => ic.warn,
        Level::Ok => ic.ok,
        Level::Info => ic.info,
    };
    let inner = modal_frame(buf, width, h, accent, &d.title, icon);
    let bg = theme::BG_MODAL;
    for (i, row) in rows.iter().enumerate() {
        let spans: Vec<Span> = row.iter().map(|(s, stl)| (s.clone(), stl.bg(bg))).collect();
        buf.spans(inner.x, inner.y + 1 + i, &spans, inner.right());
    }
    // Buttons.
    let by = inner.bottom().saturating_sub(1);
    let button = |label: &str, focused: bool, color| {
        if focused {
            span(
                format!("  {}  ", label),
                Style::new(theme::BG, color).bold(),
            )
        } else {
            span(format!("  {}  ", label), Style::new(color, theme::BG_ALT))
        }
    };
    let mut btns = Vec::new();
    if let Some(y) = &d.yes {
        btns.push(button(&format!("[y] {}", y), d.focus_yes, accent));
        btns.push(span("   ", Style::new(theme::TEXT, bg)));
        btns.push(button("[n] Cancel", !d.focus_yes, theme::MUTED));
    } else {
        btns.push(button(&format!("[Enter] {}", d.no), true, accent));
    }
    let bw = Buffer::spans_width(&btns);
    let bx = inner.x + inner.w.saturating_sub(bw) / 2;
    buf.spans(bx, by, &btns, inner.right());
}

/// A label/value line for info panels.
pub fn kv(
    buf: &mut Buffer,
    x: usize,
    y: usize,
    max_x: usize,
    label: &str,
    label_w: usize,
    value: &[Span],
) {
    buf.text(x, y, &pad(label, label_w), st(theme::MUTED), max_x);
    buf.spans(x + label_w + 1, y, value, max_x);
}

/// Green check / red cross / yellow warning span.
pub fn mark(ok: Option<bool>) -> Span {
    let ic = icons();
    match ok {
        Some(true) => span(format!("{} ", ic.ok), st(theme::GREEN)),
        Some(false) => span(format!("{} ", ic.fail), st(theme::RED)),
        None => span(format!("{} ", ic.warn), st(theme::YELLOW)),
    }
}

/// Simple syntax highlighting for JSON and TOML previews.
pub fn highlight_line(line: &str, toml: bool) -> Vec<Span> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return vec![span(line, st(theme::SUBTLE).italic())];
    }
    if toml && trimmed.starts_with('[') {
        return vec![span(line, st(theme::MAGENTA).bold())];
    }
    let mut out = Vec::new();
    let mut rest = line;
    if toml {
        if let Some(eq) = line.find('=') {
            out.push(span(&line[..eq], st(theme::CYAN)));
            out.push(span("=", st(theme::SUBTLE)));
            rest = &line[eq + 1..];
        }
    }
    highlight_values(rest, toml, &mut out);
    out
}

fn highlight_values(s: &str, toml: bool, out: &mut Vec<Span>) {
    let chars: Vec<char> = s.chars().collect();
    let plain_st = st(theme::MUTED);
    let mut plain = String::new();
    let mut i = 0;
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    while i < chars.len() {
        let c = chars[i];
        let prev_word = i > 0 && is_word(chars[i - 1]);
        if c == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' {
                if chars[j] == '\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            let tail: String = chars[end..].iter().collect();
            let is_key = !toml && tail.trim_start().starts_with(':');
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain), plain_st));
            }
            let text: String = chars[i..end].iter().collect();
            out.push(span(
                text,
                st(if is_key { theme::CYAN } else { theme::GREEN }),
            ));
            i = end;
        } else if toml && c == '#' {
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain), plain_st));
            }
            out.push(span(
                chars[i..].iter().collect::<String>(),
                st(theme::SUBTLE).italic(),
            ));
            return;
        } else if !prev_word
            && (c.is_ascii_digit()
                || (c == '-' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())))
        {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '.') {
                j += 1;
            }
            if !plain.is_empty() {
                out.push(span(std::mem::take(&mut plain), plain_st));
            }
            out.push(span(
                chars[i..j].iter().collect::<String>(),
                st(theme::YELLOW),
            ));
            i = j;
        } else if !prev_word && c.is_alphabetic() {
            let mut j = i;
            while j < chars.len() && is_word(chars[j]) {
                j += 1;
            }
            let w: String = chars[i..j].iter().collect();
            if matches!(w.as_str(), "true" | "false" | "null") {
                if !plain.is_empty() {
                    out.push(span(std::mem::take(&mut plain), plain_st));
                }
                out.push(span(w, st(theme::ORANGE).bold()));
            } else {
                plain.push_str(&w);
            }
            i = j;
        } else {
            plain.push(c);
            i += 1;
        }
    }
    if !plain.is_empty() {
        out.push(span(plain, plain_st));
    }
}

/// Scrollable source preview with line numbers.
pub fn source_view(buf: &mut Buffer, r: Rect, lines: &[String], scroll: &mut usize, toml: bool) {
    if r.h == 0 {
        return;
    }
    *scroll = (*scroll).min(lines.len().saturating_sub(r.h));
    let num_w = lines.len().to_string().len().max(2);
    for (row, (i, line)) in lines.iter().enumerate().skip(*scroll).take(r.h).enumerate() {
        let y = r.y + row;
        buf.text(
            r.x,
            y,
            &format!("{:>w$} ", i + 1, w = num_w),
            st(theme::SUBTLE),
            r.right(),
        );
        buf.set(r.x + num_w + 1, y, '│', st(theme::GRID));
        let expanded = line.replace('\t', "    ");
        buf.spans(
            r.x + num_w + 3,
            y,
            &highlight_line(&expanded, toml),
            r.right(),
        );
    }
    if lines.len() > r.h {
        // Scrollbar.
        let track = r.h;
        let thumb = (track * track / lines.len()).max(1);
        let pos = (track - thumb) * *scroll / (lines.len() - r.h).max(1);
        for i in 0..track {
            let ch = if i >= pos && i < pos + thumb {
                '┃'
            } else {
                '│'
            };
            let c = if i >= pos && i < pos + thumb {
                theme::CYAN
            } else {
                theme::GRID
            };
            buf.set(r.right() - 1, r.y + i, ch, st(c));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_editing_keys() {
        let mut i = Input::new("t", "l", InputKind::Text, "/vol/kernel", Path::new("/"));
        i.handle(Key::Ctrl('w'));
        assert_eq!(i.value(), "/vol/");
        i.handle(Key::Char('x'));
        i.handle(Key::Home);
        i.handle(Key::Delete);
        assert_eq!(i.value(), "vol/x");
        i.handle(Key::Ctrl('u'));
        assert_eq!(i.value(), "vol/x");
        i.handle(Key::End);
        i.handle(Key::Ctrl('u'));
        assert_eq!(i.value(), "");
    }

    #[test]
    fn number_input_rejects_letters() {
        let mut i = Input::new("t", "l", InputKind::Number, "1", Path::new("/"));
        i.handle(Key::Char('a'));
        assert_eq!(i.value(), "1");
        assert!(i.error.is_some());
        assert!(matches!(i.handle(Key::Enter), InputEvent::Submit(v) if v == "1"));
    }

    #[test]
    fn list_skips_sections() {
        let items = vec![
            Item::section("a"),
            Item::new("x", ItemKind::Path, "x"),
            Item::section("b"),
            Item::new("y", ItemKind::Toggle, "y"),
        ];
        let mut s = ListState::default();
        s.fix(&items);
        assert_eq!(s.sel, 1);
        s.step(&items, 1);
        assert_eq!(s.sel, 3);
        s.step(&items, 1);
        assert_eq!(s.sel, 3);
    }

    #[test]
    fn highlights_json_keys_and_values() {
        let spans = highlight_line("\t\"cover\": true,", false);
        let colors: Vec<_> = spans.iter().map(|(_, s)| s.fg).collect();
        assert!(colors.contains(&theme::CYAN));
        assert!(colors.contains(&theme::ORANGE));
    }
}
