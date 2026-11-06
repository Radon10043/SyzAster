//! Color palette and Nerd Font icon set.

use super::buffer::{Rgb, Style};
use std::sync::OnceLock;

pub const BG: Rgb = Rgb(0x0a, 0x0e, 0x14);
pub const BG_ALT: Rgb = Rgb(0x10, 0x16, 0x20);
pub const BG_SEL: Rgb = Rgb(0x15, 0x2a, 0x3d);
pub const BG_TAB: Rgb = Rgb(0x13, 0x33, 0x4a);
pub const BG_MODAL: Rgb = Rgb(0x0f, 0x15, 0x1f);
pub const BG_INPUT: Rgb = Rgb(0x05, 0x08, 0x0c);
pub const BORDER: Rgb = Rgb(0x26, 0x34, 0x48);
pub const BORDER_HI: Rgb = Rgb(0x00, 0xe5, 0xff);
pub const GRID: Rgb = Rgb(0x1a, 0x24, 0x31);
pub const TEXT: Rgb = Rgb(0xc8, 0xd3, 0xe0);
pub const MUTED: Rgb = Rgb(0x5d, 0x6d, 0x82);
pub const SUBTLE: Rgb = Rgb(0x3b, 0x48, 0x5a);
pub const WHITE: Rgb = Rgb(0xf2, 0xf6, 0xfa);
pub const CYAN: Rgb = Rgb(0x00, 0xe5, 0xff);
pub const MAGENTA: Rgb = Rgb(0xff, 0x2b, 0xd6);
pub const GREEN: Rgb = Rgb(0x3d, 0xff, 0x8f);
pub const YELLOW: Rgb = Rgb(0xff, 0xd0, 0x3a);
pub const ORANGE: Rgb = Rgb(0xff, 0x9e, 0x3d);
pub const RED: Rgb = Rgb(0xff, 0x4d, 0x6a);
pub const PURPLE: Rgb = Rgb(0xa7, 0x8b, 0xfa);
pub const BLUE: Rgb = Rgb(0x4d, 0xa3, 0xff);

/// Palette for metric charts, cycled by index.
pub const SERIES: [Rgb; 6] = [CYAN, GREEN, MAGENTA, YELLOW, PURPLE, ORANGE];

pub const fn st(fg: Rgb) -> Style {
    Style::new(fg, BG)
}

pub const BASE: Style = st(TEXT);
pub const DIM: Style = st(MUTED);
pub const KEY: Style = Style::new(YELLOW, BG).bold();
pub const TITLE: Style = Style::new(CYAN, BG).bold();

/// Icons used across the UI. The Nerd Font set uses Font Awesome and
/// Powerline code points, which every patched Nerd Font ships.
pub struct Icons {
    pub kernel: &'static str,
    pub fuzzer: &'static str,
    pub monitor: &'static str,
    pub fuzzing: &'static str,
    pub metrics: &'static str,
    pub issues: &'static str,
    pub folder: &'static str,
    pub folder_open: &'static str,
    pub file: &'static str,
    pub ok: &'static str,
    pub fail: &'static str,
    pub warn: &'static str,
    pub info: &'static str,
    pub on: &'static str,
    pub off: &'static str,
    pub play: &'static str,
    pub stop: &'static str,
    pub edit: &'static str,
    pub term: &'static str,
    pub clock: &'static str,
    pub branch: &'static str,
    pub cog: &'static str,
    pub chip: &'static str,
    pub bolt: &'static str,
    pub bug: &'static str,
    pub search: &'static str,
    pub radar: &'static str,
    pub cap_l: &'static str,
    pub cap_r: &'static str,
    pub arrow: &'static str,
    pub dot: &'static str,
}

pub const NERD: Icons = Icons {
    kernel: "\u{f2db}",
    fuzzer: "\u{f188}",
    monitor: "\u{f201}",
    fuzzing: "\u{f0e7}",
    metrics: "\u{f080}",
    issues: "\u{f071}",
    folder: "\u{f07b}",
    folder_open: "\u{f07c}",
    file: "\u{f15b}",
    ok: "\u{f00c}",
    fail: "\u{f00d}",
    warn: "\u{f071}",
    info: "\u{f05a}",
    on: "\u{f205}",
    off: "\u{f204}",
    play: "\u{f04b}",
    stop: "\u{f04d}",
    edit: "\u{f044}",
    term: "\u{f120}",
    clock: "\u{f017}",
    branch: "\u{e0a0}",
    cog: "\u{f013}",
    chip: "\u{f2db}",
    bolt: "\u{f0e7}",
    bug: "\u{f188}",
    search: "\u{f002}",
    radar: "\u{f140}",
    cap_l: "\u{e0b6}",
    cap_r: "\u{e0b4}",
    arrow: "\u{f054}",
    dot: "\u{f111}",
};

pub const PLAIN: Icons = Icons {
    kernel: "K",
    fuzzer: "F",
    monitor: "M",
    fuzzing: "~",
    metrics: "#",
    issues: "!",
    folder: "+",
    folder_open: "-",
    file: "*",
    ok: "v",
    fail: "x",
    warn: "!",
    info: "i",
    on: "#",
    off: "-",
    play: ">",
    stop: "#",
    edit: "e",
    term: "$",
    clock: "@",
    branch: "@",
    cog: "*",
    chip: "K",
    bolt: "~",
    bug: "!",
    search: "?",
    radar: "o",
    cap_l: " ",
    cap_r: " ",
    arrow: ">",
    dot: "*",
};

static ICONS: OnceLock<&'static Icons> = OnceLock::new();

pub fn set_plain_icons(plain: bool) {
    let _ = ICONS.set(if plain { &PLAIN } else { &NERD });
}

pub fn icons() -> &'static Icons {
    ICONS.get_or_init(|| &NERD)
}
