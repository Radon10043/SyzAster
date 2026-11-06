//! Raw terminal I/O for the SyzAster TUI.
//!
//! The crate has no external dependencies, so raw mode, window size queries,
//! input polling and signalling go straight to the C library through FFI.
//! Only the small, stable subset of the Linux ABI needed here is declared.

use super::buffer::{Buffer, Style};
use std::io::{self, Write};
use std::os::raw::{c_int, c_long, c_ulong, c_void};
use std::sync::Mutex;

#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
    // Slack in case the C library's struct is larger than the Linux layout.
    _reserved: [u8; 64],
}

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
    tm_gmtoff: c_long,
    tm_zone: *const u8,
    _reserved: [u8; 32],
}

extern "C" {
    fn tcgetattr(fd: c_int, termios: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, action: c_int, termios: *const Termios) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn isatty(fd: c_int) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn localtime_r(time: *const i64, tm: *mut Tm) -> *mut Tm;
    fn signal(sig: c_int, handler: usize) -> usize;
}

const STDIN: c_int = 0;
const STDOUT: c_int = 1;
const TCSAFLUSH: c_int = 2;
const TIOCGWINSZ: c_ulong = 0x5413;
const POLLIN: i16 = 0x1;

const IGNBRK: u32 = 0o1;
const BRKINT: u32 = 0o2;
const PARMRK: u32 = 0o10;
const ISTRIP: u32 = 0o40;
const INLCR: u32 = 0o100;
const IGNCR: u32 = 0o200;
const ICRNL: u32 = 0o400;
const IXON: u32 = 0o2000;
const OPOST: u32 = 0o1;
const ISIG: u32 = 0o1;
const ICANON: u32 = 0o2;
const ECHO: u32 = 0o10;
const ECHONL: u32 = 0o100;
const IEXTEN: u32 = 0o100000;
const CSIZE: u32 = 0o60;
const PARENB: u32 = 0o400;
const CS8: u32 = 0o60;
const VTIME: usize = 5;
const VMIN: usize = 6;

pub const SIGINT: i32 = 2;
pub const SIGKILL: i32 = 9;
const SIGQUIT: c_int = 3;
const SIGHUP: c_int = 1;
const SIGTERM: c_int = 15;
const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;

static ORIGINAL: Mutex<Option<Termios>> = Mutex::new(None);

const ENTER_SEQ: &str = "\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[2J";
const LEAVE_SEQ: &str = "\x1b[0m\x1b[?7h\x1b[?25h\x1b[?1049l";

/// Returns whether both stdin and stdout are attached to a terminal.
pub fn is_tty() -> bool {
    unsafe { isatty(STDIN) == 1 && isatty(STDOUT) == 1 }
}

fn enable_raw() -> io::Result<()> {
    let mut orig: Termios = unsafe { std::mem::zeroed() };
    if unsafe { tcgetattr(STDIN, &mut orig) } != 0 {
        return Err(io::Error::last_os_error());
    }
    {
        let mut saved = ORIGINAL.lock().unwrap_or_else(|e| e.into_inner());
        if saved.is_none() {
            *saved = Some(orig);
        }
    }
    let mut raw = orig;
    raw.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL | IXON);
    raw.c_oflag &= !OPOST;
    raw.c_lflag &= !(ECHO | ECHONL | ICANON | ISIG | IEXTEN);
    raw.c_cflag &= !(CSIZE | PARENB);
    raw.c_cflag |= CS8;
    raw.c_cc[VMIN] = 1;
    raw.c_cc[VTIME] = 0;
    if unsafe { tcsetattr(STDIN, TCSAFLUSH, &raw) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn disable_raw() {
    let saved = ORIGINAL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(orig) = saved.as_ref() {
        unsafe {
            tcsetattr(STDIN, TCSAFLUSH, orig);
        }
    }
}

/// Restores the terminal to cooked mode and the primary screen.
fn restore_terminal() {
    let mut out = io::stdout();
    let _ = out.write_all(LEAVE_SEQ.as_bytes());
    let _ = out.flush();
    disable_raw();
}

/// Queries the terminal size as `(columns, rows)`.
pub fn size() -> (usize, usize) {
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let ok = unsafe { ioctl(STDOUT, TIOCGWINSZ, &mut ws as *mut Winsize) } == 0;
    if ok && ws.ws_col > 0 && ws.ws_row > 0 {
        return (ws.ws_col as usize, ws.ws_row as usize);
    }
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<usize>().ok());
    (env("COLUMNS").unwrap_or(80), env("LINES").unwrap_or(24))
}

/// Sends a signal to a process.
pub fn send_signal(pid: i32, sig: i32) -> io::Result<()> {
    if unsafe { kill(pid, sig) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

static TERMINATE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_terminate(_sig: c_int) {
    TERMINATE.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Turns SIGTERM and SIGHUP into a flag polled by the event loop, so the
/// terminal is restored instead of being left in raw mode.
pub fn catch_termination() {
    let handler = on_terminate as extern "C" fn(c_int) as usize;
    unsafe {
        signal(SIGTERM, handler);
        signal(SIGHUP, handler);
    }
}

pub fn termination_requested() -> bool {
    TERMINATE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Ignores (or restores) terminal interrupts in this process. Used while a
/// foreground job owns the terminal so that Ctrl+C only reaches the job.
pub fn ignore_interrupts(ignore: bool) {
    let h = if ignore { SIG_IGN } else { SIG_DFL };
    unsafe {
        signal(SIGINT, h);
        signal(SIGQUIT, h);
    }
}

/// Returns whether a process exists (signal 0 probe; EPERM still means alive).
pub fn process_alive(pid: i32) -> bool {
    if unsafe { kill(pid, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(1)
}

/// Broken-down local time: `(year, month, day, hour, minute, second)`.
pub fn local_time(unix: i64) -> (i32, u32, u32, u32, u32, u32) {
    let mut tm: Tm = unsafe { std::mem::zeroed() };
    let res = unsafe { localtime_r(&unix, &mut tm) };
    if res.is_null() {
        return utc_time(unix);
    }
    (
        tm.tm_year + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

fn utc_time(unix: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = unix.div_euclid(86400);
    let secs = unix.rem_euclid(86400) as u32;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y } as i32;
    (y, m, d, secs / 3600, (secs / 60) % 60, secs % 60)
}

/// Keyboard input decoded from the raw byte stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Unknown,
}

/// Decodes a chunk of terminal input into keys.
pub fn parse_keys(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                let (key, used) = parse_escape(&bytes[i..]);
                keys.push(key);
                i += used;
            }
            b'\r' | b'\n' => {
                keys.push(Key::Enter);
                i += 1;
            }
            b'\t' => {
                keys.push(Key::Tab);
                i += 1;
            }
            0x7f | 0x08 => {
                keys.push(Key::Backspace);
                i += 1;
            }
            0x01..=0x1a => {
                keys.push(Key::Ctrl((b'a' + b - 1) as char));
                i += 1;
            }
            0x00..=0x1f => {
                keys.push(Key::Unknown);
                i += 1;
            }
            _ => {
                let len = utf8_len(b);
                let end = (i + len).min(bytes.len());
                match std::str::from_utf8(&bytes[i..end]) {
                    Ok(s) => {
                        if let Some(c) = s.chars().next() {
                            keys.push(Key::Char(c));
                        }
                    }
                    Err(_) => keys.push(Key::Unknown),
                }
                i = end;
            }
        }
    }
    keys
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

fn parse_escape(bytes: &[u8]) -> (Key, usize) {
    if bytes.len() == 1 {
        return (Key::Esc, 1);
    }
    match bytes[1] {
        b'[' => {
            let mut j = 2;
            while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                j += 1;
            }
            if j >= bytes.len() {
                return (Key::Unknown, bytes.len());
            }
            let params = std::str::from_utf8(&bytes[2..j]).unwrap_or("");
            let first: u32 = params
                .split(';')
                .next()
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);
            let key = match bytes[j] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'Z' => Key::BackTab,
                b'P' => Key::F(1),
                b'Q' => Key::F(2),
                b'R' => Key::F(3),
                b'S' => Key::F(4),
                b'~' => match first {
                    1 | 7 => Key::Home,
                    4 | 8 => Key::End,
                    3 => Key::Delete,
                    5 => Key::PageUp,
                    6 => Key::PageDown,
                    15 => Key::F(5),
                    17..=21 => Key::F((first - 11) as u8),
                    23 | 24 => Key::F((first - 12) as u8),
                    _ => Key::Unknown,
                },
                _ => Key::Unknown,
            };
            (key, j + 1)
        }
        b'O' if bytes.len() >= 3 => {
            let key = match bytes[2] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'P' => Key::F(1),
                b'Q' => Key::F(2),
                b'R' => Key::F(3),
                b'S' => Key::F(4),
                _ => Key::Unknown,
            };
            (key, 3)
        }
        // Alt+key or a lone Esc immediately followed by another key.
        _ => (Key::Esc, 1),
    }
}

/// Owns the terminal while the TUI runs and renders frames with diffing.
pub struct Terminal {
    prev: Option<Buffer>,
    active: bool,
}

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            prev_hook(info);
        }));
        let mut term = Terminal {
            prev: None,
            active: false,
        };
        term.resume()?;
        Ok(term)
    }

    /// Hands the terminal back to the shell (for vim or foreground commands).
    pub fn suspend(&mut self) {
        if self.active {
            restore_terminal();
            self.active = false;
        }
    }

    /// Re-enters raw mode and the alternate screen, forcing a full redraw.
    pub fn resume(&mut self) -> io::Result<()> {
        enable_raw()?;
        let mut out = io::stdout();
        out.write_all(ENTER_SEQ.as_bytes())?;
        out.flush()?;
        self.prev = None;
        self.active = true;
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.prev = None;
    }

    /// Waits up to `timeout_ms` for input and returns the decoded keys.
    pub fn read_keys(&mut self, timeout_ms: i32) -> Vec<Key> {
        let mut fds = PollFd {
            fd: STDIN,
            events: POLLIN,
            revents: 0,
        };
        let ready = unsafe { poll(&mut fds, 1, timeout_ms) };
        if ready <= 0 || fds.revents & POLLIN == 0 {
            return Vec::new();
        }
        let mut buf = [0u8; 4096];
        let n = unsafe { read(STDIN, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if n <= 0 {
            return Vec::new();
        }
        parse_keys(&buf[..n as usize])
    }

    /// Writes the cells that changed since the previous frame.
    pub fn draw(&mut self, buf: &Buffer) -> io::Result<()> {
        let full = match &self.prev {
            Some(p) => p.width() != buf.width() || p.height() != buf.height(),
            None => true,
        };
        let mut s = String::with_capacity(buf.width() * buf.height() * 4);
        s.push_str("\x1b[?2026h");
        if full {
            s.push_str("\x1b[0m\x1b[2J");
        }
        let mut cur_style: Option<Style> = None;
        let mut cursor: Option<(usize, usize)> = None;
        for y in 0..buf.height() {
            for x in 0..buf.width() {
                let cell = buf.cell(x, y);
                if cell.cont {
                    continue;
                }
                if !full {
                    if let Some(prev) = &self.prev {
                        if prev.cell(x, y) == cell {
                            continue;
                        }
                    }
                }
                if cursor != Some((x, y)) {
                    s.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
                }
                if cur_style != Some(cell.style) {
                    cell.style.write_sgr(&mut s);
                    cur_style = Some(cell.style);
                }
                s.push(cell.ch);
                let w = if x + 1 < buf.width() && buf.cell(x + 1, y).cont {
                    2
                } else {
                    1
                };
                cursor = Some((x + w, y));
            }
        }
        s.push_str("\x1b[0m\x1b[?2026l");
        let mut out = io::stdout();
        out.write_all(s.as_bytes())?;
        out.flush()?;
        self.prev = Some(buf.clone());
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.suspend();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arrows_and_specials() {
        let keys = parse_keys(b"\x1b[A\x1b[B\x1bOC\x1b[D\x1b[5~\x1b[6~\x1b[3~\x1b[Z");
        assert_eq!(
            keys,
            vec![
                Key::Up,
                Key::Down,
                Key::Right,
                Key::Left,
                Key::PageUp,
                Key::PageDown,
                Key::Delete,
                Key::BackTab
            ]
        );
    }

    #[test]
    fn parses_text_and_controls() {
        let keys = parse_keys("a\r\t\x7f\x03é".as_bytes());
        assert_eq!(
            keys,
            vec![
                Key::Char('a'),
                Key::Enter,
                Key::Tab,
                Key::Backspace,
                Key::Ctrl('c'),
                Key::Char('é')
            ]
        );
        assert_eq!(parse_keys(b"\x1b"), vec![Key::Esc]);
    }

    #[test]
    fn utc_conversion_matches_known_dates() {
        assert_eq!(utc_time(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc_time(1_700_000_000), (2023, 11, 14, 22, 13, 20));
        assert_eq!(utc_time(951_782_400), (2000, 2, 29, 0, 0, 0));
    }
}
