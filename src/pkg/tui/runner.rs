//! Runs a command inside the TUI and streams its output into a modal.
//!
//! Used for long kernel steps (download, KCOV check, image build): the log
//! scrolls in a focused window over the dimmed UI instead of leaving the TUI.

use super::buffer::{span, truncate, Buffer, Rect, Rgb, Style};
use super::term::{self, Key};
use super::theme::{self, icons, st};
use super::ui::{self, Level};
use super::Job;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Lines kept in memory; older output is dropped.
const MAX_LINES: usize = 20_000;

#[derive(Default)]
struct Log {
    lines: Vec<String>,
    /// Unterminated (e.g. `\r` progress) line of each stream.
    live: [String; 2],
}

impl Log {
    fn push(&mut self, line: String) {
        self.lines.push(line);
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
        }
    }
}

/// Splits raw output into lines, dropping ANSI escapes and honoring `\r`.
struct Decoder {
    stream: usize,
    cur: String,
    esc: u8,
    utf8: Vec<u8>,
}

impl Decoder {
    fn feed(&mut self, bytes: &[u8], log: &Mutex<Log>) {
        let mut done: Vec<String> = Vec::new();
        for &b in bytes {
            match self.esc {
                // ESC seen: '[' starts CSI, ']' starts OSC, anything else ends it.
                1 => {
                    self.esc = match b {
                        b'[' => 2,
                        b']' => 3,
                        _ => 0,
                    };
                    continue;
                }
                2 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.esc = 0;
                    }
                    continue;
                }
                3 => {
                    if b == 0x07 {
                        self.esc = 0;
                    } else if b == 0x1b {
                        self.esc = 1;
                    }
                    continue;
                }
                _ => {}
            }
            match b {
                0x1b => self.esc = 1,
                b'\n' => done.push(std::mem::take(&mut self.cur)),
                b'\r' => self.cur.clear(),
                b'\t' => self.cur.push_str("    "),
                0x00..=0x1f | 0x7f => {}
                _ => {
                    self.utf8.push(b);
                    match std::str::from_utf8(&self.utf8) {
                        Ok(s) => {
                            self.cur.push_str(s);
                            self.utf8.clear();
                        }
                        Err(e) if e.error_len().is_some() => {
                            self.cur.push('\u{fffd}');
                            self.utf8.clear();
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
        for l in done {
            log.push(l);
        }
        log.live[self.stream] = self.cur.clone();
    }
}

fn pump(mut src: impl Read + Send + 'static, stream: usize, log: Arc<Mutex<Log>>) {
    thread::spawn(move || {
        let mut dec = Decoder {
            stream,
            cur: String::new(),
            esc: 0,
            utf8: Vec::new(),
        };
        let mut buf = [0u8; 8192];
        loop {
            match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => dec.feed(&buf[..n], &log),
            }
        }
        let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
        let rest = std::mem::take(&mut l.live[stream]);
        if !rest.is_empty() {
            l.push(rest);
        }
    });
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Running,
    Done(Option<i32>),
    Cancelled,
    Error(String),
}

pub enum RunnerEvent {
    Pending,
    /// The user closed the finished job window.
    Close(Level, String),
}

pub struct Runner {
    pub job: Job,
    log: Arc<Mutex<Log>>,
    child: Option<Child>,
    state: State,
    started: Instant,
    elapsed: Duration,
    /// Lines scrolled up from the bottom; 0 follows the output.
    offset: usize,
    page: usize,
    cancel_at: Option<Instant>,
}

impl Runner {
    pub fn start(job: Job) -> Runner {
        let log = Arc::new(Mutex::new(Log::default()));
        if let Some(dir) = &job.mkdir {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut cmd = Command::new(&job.program);
        cmd.args(&job.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Own process group, so cancelling reaches the whole pipeline.
            .process_group(0);
        if let Some(cwd) = &job.cwd {
            cmd.current_dir(cwd);
        }
        let (child, state) = match cmd.spawn() {
            Ok(mut c) => {
                if let Some(o) = c.stdout.take() {
                    pump(o, 0, log.clone());
                }
                if let Some(e) = c.stderr.take() {
                    pump(e, 1, log.clone());
                }
                (Some(c), State::Running)
            }
            Err(e) => (
                None,
                State::Error(format!("cannot run {}: {}", job.program, e)),
            ),
        };
        Runner {
            job,
            log,
            child,
            state,
            started: Instant::now(),
            elapsed: Duration::ZERO,
            offset: 0,
            page: 10,
            cancel_at: None,
        }
    }

    pub fn running(&self) -> bool {
        self.state == State::Running
    }

    /// Reaps the child once it exits.
    pub fn poll(&mut self) {
        if self.state != State::Running {
            return;
        }
        self.elapsed = self.started.elapsed();
        let Some(c) = &mut self.child else { return };
        if let Ok(Some(status)) = c.try_wait() {
            self.state = if self.cancel_at.is_some() && !status.success() {
                State::Cancelled
            } else {
                State::Done(status.code())
            };
        }
    }

    fn cancel(&mut self) {
        let Some(c) = &self.child else { return };
        let pgid = -(c.id() as i32);
        // First press interrupts, a later one kills.
        let sig = match self.cancel_at {
            Some(t) if t.elapsed() >= Duration::from_secs(2) => term::SIGKILL,
            Some(_) => return,
            None => term::SIGINT,
        };
        let _ = term::send_signal(pgid, sig);
        if self.cancel_at.is_none() {
            self.cancel_at = Some(Instant::now());
        }
        if sig == term::SIGKILL {
            self.cancel_at = Some(Instant::now() - Duration::from_secs(60));
        }
    }

    fn result(&self) -> (Level, String) {
        match &self.state {
            State::Done(Some(0)) => (
                Level::Ok,
                format!("{}: finished successfully", self.job.title),
            ),
            State::Done(Some(c)) => (
                Level::Error,
                format!("{}: failed (exit code {})", self.job.title, c),
            ),
            State::Done(None) => (
                Level::Error,
                format!("{}: killed by a signal", self.job.title),
            ),
            State::Cancelled => (Level::Warn, format!("{}: cancelled", self.job.title)),
            State::Error(e) => (Level::Error, format!("{}: {}", self.job.title, e)),
            State::Running => (Level::Info, format!("{}: running", self.job.title)),
        }
    }

    pub fn handle(&mut self, key: Key) -> RunnerEvent {
        let total = self.log.lock().map(|l| l.lines.len()).unwrap_or(0);
        match key {
            Key::Up | Key::Char('k') => self.offset = (self.offset + 1).min(total),
            Key::Down | Key::Char('j') => self.offset = self.offset.saturating_sub(1),
            Key::PageUp => self.offset = (self.offset + self.page).min(total),
            Key::PageDown => self.offset = self.offset.saturating_sub(self.page),
            Key::Home | Key::Char('g') => self.offset = total,
            Key::End | Key::Char('G') => self.offset = 0,
            Key::Char('x') | Key::Ctrl('c') if self.running() => self.cancel(),
            Key::Enter | Key::Esc | Key::Char('q') if !self.running() => {
                let (l, m) = self.result();
                return RunnerEvent::Close(l, m);
            }
            _ => {}
        }
        RunnerEvent::Pending
    }

    pub fn hints(&self) -> Vec<(&'static str, &'static str)> {
        let mut h = vec![("↑↓/PgUp/PgDn", "Scroll"), ("End", "Follow")];
        if self.running() {
            h.push(if self.cancel_at.is_some() {
                ("x", "Force kill")
            } else {
                ("x", "Cancel")
            });
        } else {
            h.push(("Enter/Esc", "Close"));
        }
        h
    }

    pub fn draw(&mut self, buf: &mut Buffer) {
        let ic = icons();
        let (accent, badge): (Rgb, String) = match &self.state {
            State::Running => {
                let spin = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
                let i = (self.started.elapsed().as_millis() / 120) as usize % spin.len();
                let what = if self.cancel_at.is_some() {
                    "CANCELLING"
                } else {
                    "RUNNING"
                };
                (theme::CYAN, format!(" {} {} ", spin[i], what))
            }
            State::Done(Some(0)) => (theme::GREEN, format!(" {} SUCCESS ", ic.ok)),
            State::Done(Some(c)) => (theme::RED, format!(" {} FAILED · exit {} ", ic.fail, c)),
            State::Done(None) => (theme::RED, format!(" {} KILLED ", ic.fail)),
            State::Cancelled => (theme::YELLOW, format!(" {} CANCELLED ", ic.warn)),
            State::Error(_) => (theme::RED, format!(" {} ERROR ", ic.fail)),
        };
        let area = buf.area();
        let w = (area.w * 9 / 10).max(60);
        let h = (area.h * 85 / 100).max(14);
        let inner = ui::modal_frame(buf, w, h, accent, &self.job.title, ic.term);
        let bg = theme::BG_MODAL;

        // Header: status badge, elapsed time, command line.
        let elapsed = util_dur(self.elapsed);
        let x = buf.spans(
            inner.x,
            inner.y,
            &[
                span(badge, Style::new(theme::BG, accent).bold()),
                span(
                    format!("  {} {}", ic.clock, elapsed),
                    Style::new(theme::MUTED, bg),
                ),
            ],
            inner.right(),
        );
        let cmd = format!("$ {}", self.job.display());
        buf.text(
            x + 3,
            inner.y,
            &truncate(&cmd, inner.right().saturating_sub(x + 3)),
            Style::new(theme::GREEN, bg),
            inner.right(),
        );
        buf.hline(
            inner.x,
            inner.y + 1,
            inner.w,
            '─',
            Style::new(theme::BORDER, bg),
        );

        // Log body.
        let body = Rect::new(inner.x, inner.y + 2, inner.w, inner.h.saturating_sub(4));
        self.page = body.h.max(1);
        let log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        let mut lines: Vec<&str> = log.lines.iter().map(|s| s.as_str()).collect();
        for l in &log.live {
            if !l.is_empty() {
                lines.push(l);
            }
        }
        if let State::Error(e) = &self.state {
            lines.push(e);
        }
        self.offset = self.offset.min(lines.len().saturating_sub(body.h));
        let end = lines.len() - self.offset;
        let start = end.saturating_sub(body.h);
        if lines.is_empty() {
            let msg = if self.running() {
                "waiting for output…"
            } else {
                "(no output)"
            };
            buf.text(
                body.x + 1,
                body.y,
                msg,
                Style::new(theme::SUBTLE, bg).italic(),
                body.right(),
            );
        }
        for (i, l) in lines[start..end].iter().enumerate() {
            buf.text(
                body.x,
                body.y + i,
                l,
                Style::new(line_color(l), bg),
                body.right(),
            );
        }
        if lines.len() > body.h && body.h > 0 {
            let track = body.h;
            let thumb = (track * track / lines.len()).max(1);
            let pos = (track - thumb) * start / (lines.len() - body.h).max(1);
            for i in 0..track {
                let on = i >= pos && i < pos + thumb;
                buf.set(
                    body.right(),
                    body.y + i,
                    if on { '┃' } else { '│' },
                    Style::new(if on { accent } else { theme::GRID }, bg),
                );
            }
        }
        drop(log);

        // Footer: position and keys.
        let fy = inner.bottom().saturating_sub(1);
        buf.hline(inner.x, fy - 1, inner.w, '─', Style::new(theme::BORDER, bg));
        let pos = if self.offset == 0 {
            format!("{} lines · following", lines_len(&self.log))
        } else {
            format!(
                "{} lines · {} above bottom",
                lines_len(&self.log),
                self.offset
            )
        };
        let mut x = inner.x;
        for (k, a) in self.hints() {
            x = buf.spans(
                x,
                fy,
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
        buf.text_right(inner.right(), fy, &pos, st(theme::SUBTLE).bg(bg));
    }
}

fn lines_len(log: &Mutex<Log>) -> usize {
    log.lock().map(|l| l.lines.len()).unwrap_or(0)
}

fn util_dur(d: Duration) -> String {
    super::util::dur_long(d.as_secs_f64())
}

fn line_color(l: &str) -> Rgb {
    let t = l.trim_start();
    let lower = t.to_lowercase();
    if t.starts_with("PASS") || lower.starts_with("done") || lower.contains("validated") {
        theme::GREEN
    } else if t.starts_with("FAIL")
        || lower.starts_with("error")
        || lower.contains("fatal")
        || lower.contains("panicked")
    {
        theme::RED
    } else if lower.starts_with("warning") || lower.starts_with("warn") {
        theme::YELLOW
    } else if t.starts_with("Applying")
        || t.starts_with("Checking out")
        || t.starts_with("Cloning")
        || t.starts_with("==>")
    {
        theme::CYAN
    } else {
        theme::TEXT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[&[u8]]) -> Log {
        let log = Mutex::new(Log::default());
        let mut d = Decoder {
            stream: 0,
            cur: String::new(),
            esc: 0,
            utf8: Vec::new(),
        };
        for c in chunks {
            d.feed(c, &log);
        }
        log.into_inner().unwrap()
    }

    #[test]
    fn strips_ansi_and_handles_carriage_returns() {
        let log = decode(&[
            b"\x1b[1;32mPASS\x1b[0m ok\n",
            b"Receiving 10%\rReceiving 50%",
        ]);
        assert_eq!(log.lines, vec!["PASS ok"]);
        assert_eq!(log.live[0], "Receiving 50%");
    }

    #[test]
    fn decodes_utf8_split_across_chunks() {
        let s = "✓ done\n".as_bytes();
        let log = decode(&[&s[..1], &s[1..]]);
        assert_eq!(log.lines, vec!["✓ done"]);
    }

    #[test]
    fn runs_command_and_reports_exit_code() {
        let mut job = Job::new(
            "t",
            "sh",
            vec!["-c".into(), "echo hi; echo err >&2; exit 3".into()],
        );
        job.inline = true;
        let mut r = Runner::start(job);
        let t = Instant::now();
        while r.running() && t.elapsed() < Duration::from_secs(5) {
            r.poll();
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(50));
        assert_eq!(r.state, State::Done(Some(3)));
        let lines = r.log.lock().unwrap().lines.clone();
        assert!(lines.contains(&"hi".to_string()) && lines.contains(&"err".to_string()));
        assert!(matches!(
            r.handle(Key::Enter),
            RunnerEvent::Close(Level::Error, _)
        ));
    }
}
