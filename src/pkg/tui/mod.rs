//! `syzaster tui`: interactive terminal front end for SyzAster.
//!
//! Three tabs drive the workflow end to end: preparing the Asterinas kernel
//! (Kernel Config), shaping the syz-manager configuration (Fuzzer Config) and
//! watching a running campaign (Monitor). Everything is rendered with the
//! standard library only: raw mode and polling go through libc FFI
//! (`term`), frames are diffed cell buffers (`buffer`) and charts use
//! Braille dots (`chart`).

pub mod buffer;
pub mod campaign;
pub mod cfgedit;
pub mod chart;
pub mod term;
pub mod theme;
pub mod ui;
pub mod util;

mod fuzzer;
mod kernel;
mod monitor;
mod runner;

use buffer::{span, Buffer, Rect};
use std::io::{self, BufRead, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use term::{Key, Terminal};
use theme::{icons, st};
use ui::{Dialog, DialogEvent, Input, InputEvent, Level};

const MIN_W: usize = 80;
const MIN_H: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Kernel,
    Fuzzer,
    Monitor,
}

/// What to refresh after a foreground job returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum After {
    Nothing,
    Kernel,
    Fuzzer,
}

/// A command run in the foreground with the terminal handed back.
#[derive(Clone, Debug)]
pub struct Job {
    pub title: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Wait for Enter before returning to the TUI.
    pub pause: bool,
    pub after: After,
    /// Directory created before the command runs.
    pub mkdir: Option<PathBuf>,
    /// Run inside the TUI with output streamed into a modal window instead
    /// of handing the terminal over.
    pub inline: bool,
}

impl Job {
    pub fn new(title: impl Into<String>, program: impl Into<String>, args: Vec<String>) -> Self {
        Job {
            title: title.into(),
            program: program.into(),
            args,
            cwd: None,
            pause: true,
            after: After::Nothing,
            mkdir: None,
            inline: false,
        }
    }

    /// Shell-like rendering for confirmation dialogs.
    pub fn display(&self) -> String {
        let mut parts = vec![shell_quote(&self.program)];
        parts.extend(self.args.iter().map(|a| shell_quote(a)));
        parts.join(" ")
    }
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Editable fields reached through the text input modal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    KernelDir,
    ImageDir,
    KernelCfg,
    SyzkallerDir,
    ManagerCfg,
    VmCount,
    Procs,
    VmCpu,
    VmMem,
    MonitorPath,
    MetricFilter,
}

/// Actions executed after the user confirms a dialog.
#[derive(Clone, Debug)]
pub enum Confirmed {
    Run(Job),
    Generate,
    Launch,
    Signal { pid: i32, sig: i32 },
}

/// Requests from a tab to the application shell.
pub enum Action {
    Status(Level, String),
    Input(Field, Input),
    Dialog(Dialog<Confirmed>),
    Run(Job),
    Monitor(PathBuf),
}

pub fn status(level: Level, msg: impl Into<String>) -> Vec<Action> {
    vec![Action::Status(level, msg.into())]
}

/// Picks the editor: vim first (as documented), then `$VISUAL`/`$EDITOR`.
pub fn editor() -> String {
    if crate::pkg::command_exists("vim") {
        return "vim".to_string();
    }
    for var in ["VISUAL", "EDITOR"] {
        if let Ok(e) = std::env::var(var) {
            if !e.trim().is_empty() {
                return e;
            }
        }
    }
    "vi".to_string()
}

/// Displays `p` relative to the SyzAster root when it lives inside it.
pub fn rel(root: &Path, p: &Path) -> String {
    match p.strip_prefix(root) {
        Ok(r) if !r.as_os_str().is_empty() => r.display().to_string(),
        _ => util::tilde(p),
    }
}

struct App {
    root: PathBuf,
    tab: Tab,
    kernel: kernel::KernelTab,
    fuzzer: fuzzer::FuzzerTab,
    monitor: monitor::MonitorTab,
    input: Option<(Field, Input)>,
    dialog: Option<Dialog<Confirmed>>,
    status: Option<(Level, String, Instant)>,
    settings: util::Settings,
    job: Option<Job>,
    runner: Option<runner::Runner>,
    children: Vec<Child>,
    quit: bool,
}

impl App {
    fn new(root: PathBuf, monitor_dir: Option<PathBuf>) -> Self {
        let settings = util::Settings::load(root.join(".workdir").join("tui.conf"));
        let kernel = kernel::KernelTab::new(&root, &settings);
        let fuzzer = fuzzer::FuzzerTab::new(&root, &settings);
        let mut monitor = monitor::MonitorTab::new(&root, &settings);
        let mut tab = Tab::Kernel;
        if let Some(dir) = monitor_dir {
            monitor.select(dir);
            tab = Tab::Monitor;
        }
        App {
            root,
            tab,
            kernel,
            fuzzer,
            monitor,
            input: None,
            dialog: None,
            status: Some((
                Level::Info,
                "Welcome to SyzAster. Press 1/2/3 to switch tabs, q to quit.".to_string(),
                Instant::now(),
            )),
            settings,
            job: None,
            runner: None,
            children: Vec::new(),
            quit: false,
        }
    }

    fn set_status(&mut self, level: Level, msg: impl Into<String>) {
        self.status = Some((level, msg.into(), Instant::now()));
    }

    fn apply(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Status(l, m) => self.set_status(l, m),
                Action::Input(f, i) => self.input = Some((f, i)),
                Action::Dialog(d) => self.dialog = Some(d),
                Action::Run(j) => self.run(j),
                Action::Monitor(dir) => {
                    self.monitor.select(dir.clone());
                    self.settings.set("monitor.dir", dir.display().to_string());
                    let _ = self.settings.save();
                    self.tab = Tab::Monitor;
                }
            }
        }
    }

    /// Starts a job: inline jobs stream into the log window, others take
    /// over the terminal.
    fn run(&mut self, job: Job) {
        if job.inline {
            self.runner = Some(runner::Runner::start(job));
        } else {
            self.job = Some(job);
        }
    }

    fn after(&mut self, after: After) {
        match after {
            After::Kernel => self.kernel.refresh(),
            After::Fuzzer => self.fuzzer.reload(),
            After::Nothing => {}
        }
    }

    fn submit_field(&mut self, field: Field, value: String) {
        let acts = match field {
            Field::KernelDir | Field::ImageDir | Field::KernelCfg => {
                self.kernel.set_field(field, &value, &mut self.settings)
            }
            Field::SyzkallerDir
            | Field::ManagerCfg
            | Field::VmCount
            | Field::Procs
            | Field::VmCpu
            | Field::VmMem => self.fuzzer.set_field(field, &value, &mut self.settings),
            Field::MonitorPath | Field::MetricFilter => self.monitor.set_field(field, &value),
        };
        let _ = self.settings.save();
        self.apply(acts);
    }

    fn confirm(&mut self, c: Confirmed) {
        match c {
            Confirmed::Run(job) => self.run(job),
            Confirmed::Generate => {
                let acts = self.fuzzer.generate(&self.kernel);
                self.apply(acts);
            }
            Confirmed::Launch => match self.fuzzer.launch() {
                Ok((child, workdir)) => {
                    let pid = child.id();
                    self.children.push(child);
                    self.apply(vec![Action::Monitor(workdir)]);
                    self.set_status(
                        Level::Ok,
                        format!("syz-manager started (pid {}); first bench sample arrives within a minute", pid),
                    );
                }
                Err(e) => self.set_status(Level::Error, e),
            },
            Confirmed::Signal { pid, sig } => {
                let acts = self.monitor.signal(pid, sig);
                self.apply(acts);
            }
        }
    }

    fn handle_key(&mut self, key: Key) {
        if let Some(r) = &mut self.runner {
            if let runner::RunnerEvent::Close(level, msg) = r.handle(key) {
                let after = r.job.after;
                self.runner = None;
                self.after(after);
                self.set_status(level, msg);
            }
            return;
        }
        if let Some((field, input)) = &mut self.input {
            let field = *field;
            match input.handle(key) {
                InputEvent::Pending => {}
                InputEvent::Cancel => self.input = None,
                InputEvent::Submit(v) => {
                    self.input = None;
                    self.submit_field(field, v);
                }
            }
            return;
        }
        if let Some(d) = &mut self.dialog {
            match d.handle(key) {
                DialogEvent::Pending => {}
                DialogEvent::Close => self.dialog = None,
                DialogEvent::Confirm(c) => {
                    self.dialog = None;
                    self.confirm(c);
                }
            }
            return;
        }
        match key {
            Key::Ctrl('c') | Key::Char('q') => {
                self.quit = true;
                return;
            }
            Key::Char('1') => {
                self.tab = Tab::Kernel;
                return;
            }
            Key::Char('2') => {
                self.tab = Tab::Fuzzer;
                return;
            }
            Key::Char('3') => {
                self.tab = Tab::Monitor;
                return;
            }
            _ => {}
        }
        let acts = match self.tab {
            Tab::Kernel => self.kernel.handle_key(key, &mut self.settings),
            Tab::Fuzzer => self
                .fuzzer
                .handle_key(key, &self.kernel, &mut self.settings),
            Tab::Monitor => self.monitor.handle_key(key),
        };
        let _ = self.settings.save();
        self.apply(acts);
    }

    fn tick(&mut self) {
        self.children
            .retain_mut(|c| !matches!(c.try_wait(), Ok(Some(_))));
        self.monitor.tick();
        if self.tab == Tab::Fuzzer {
            self.fuzzer.tick();
        }
        if let Some((_, _, t)) = &self.status {
            if t.elapsed() > Duration::from_secs(12) {
                self.status = None;
            }
        }
    }

    fn draw(&mut self, buf: &mut Buffer) {
        let (w, h) = (buf.width(), buf.height());
        let ic = icons();
        if w < MIN_W || h < MIN_H {
            let msg = format!("Terminal too small: {}x{} (need {}x{})", w, h, MIN_W, MIN_H);
            let r = buf.area().centered(buffer::str_width(&msg).min(w), 1);
            buf.text(r.x, r.y, &msg, st(theme::YELLOW).bold(), w);
            return;
        }
        ui::banner(buf, Rect::new(0, 0, w, ui::BANNER_H), util::now_unix());
        let tabs = [
            ui::TabSpec {
                key: "1",
                icon: ic.kernel,
                label: "Kernel Config",
            },
            ui::TabSpec {
                key: "2",
                icon: ic.fuzzer,
                label: "Fuzzer Config",
            },
            ui::TabSpec {
                key: "3",
                icon: ic.monitor,
                label: "Monitor",
            },
        ];
        let active = match self.tab {
            Tab::Kernel => 0,
            Tab::Fuzzer => 1,
            Tab::Monitor => 2,
        };
        let ty = ui::BANNER_H;
        let tabs_end = ui::tabs(buf, 1, ty, w, &tabs, active, true);
        let avail = w.saturating_sub(tabs_end + 6);
        if avail >= 12 {
            let root = buffer::truncate_left(&util::tilde(&self.root), avail - 2);
            let root_note = vec![
                span(format!("{} ", ic.folder_open), st(theme::SUBTLE)),
                span(root, st(theme::MUTED)),
            ];
            let nw = Buffer::spans_width(&root_note);
            buf.spans(w.saturating_sub(nw + 2), ty, &root_note, w);
        }

        let content = Rect::new(0, ty + 1, w, h - ty - 3);
        let hints = match self.tab {
            Tab::Kernel => self.kernel.draw(buf, content),
            Tab::Fuzzer => self.fuzzer.draw(buf, content, &self.kernel),
            Tab::Monitor => self.monitor.draw(buf, content),
        };

        let running = self
            .children
            .iter()
            .map(|c| c.id().to_string())
            .collect::<Vec<_>>();
        let mut right = Vec::new();
        if !running.is_empty() {
            right.push(span(format!("{} ", ic.play), st(theme::GREEN)));
            right.push(span(
                format!("syz-manager pid {}  ", running.join(",")),
                st(theme::GREEN),
            ));
        }
        right.push(span(format!("{} ", ic.clock), st(theme::SUBTLE)));
        right.push(span(util::clock(util::now_unix()), st(theme::MUTED)));
        let msg = self.status.as_ref().map(|(l, m, _)| (*l, m.clone()));
        ui::status_line(buf, h - 2, w, msg.as_ref(), &right);
        if let Some(r) = &mut self.runner {
            ui::footer(buf, h - 1, w, &r.hints(), &[]);
            r.draw(buf);
            return;
        }
        let global = [("1-3", "Tabs"), ("q", "Quit")];
        ui::footer(buf, h - 1, w, &hints, &global);

        if let Some((_, input)) = &self.input {
            ui::draw_input(buf, input);
        } else if let Some(d) = &self.dialog {
            ui::draw_dialog(buf, d);
        }
    }
}

/// Runs a foreground job with the terminal released; returns a status line.
fn run_job(term: &mut Terminal, job: &Job) -> (Level, String) {
    term.suspend();
    if let Some(dir) = &job.mkdir {
        let _ = std::fs::create_dir_all(dir);
    }
    if job.pause {
        println!(
            "\x1b[2J\x1b[H\x1b[1;36m▶ {}\x1b[0m\n\x1b[2m$ {}\x1b[0m\n",
            job.title,
            job.display()
        );
    }
    let mut cmd = Command::new(&job.program);
    cmd.args(&job.args);
    if let Some(cwd) = &job.cwd {
        cmd.current_dir(cwd);
    }
    // Ignored dispositions survive exec, so the child restores the defaults.
    // SAFETY: signal(2) is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            term::ignore_interrupts(false);
            Ok(())
        });
    }
    term::ignore_interrupts(true);
    let status = cmd.status();
    term::ignore_interrupts(false);
    let result = match status {
        Ok(s) if s.success() => (Level::Ok, format!("{}: finished successfully", job.title)),
        Ok(s) => (Level::Error, format!("{}: failed ({})", job.title, s)),
        Err(e) => (
            Level::Error,
            format!("{}: cannot run {}: {}", job.title, job.program, e),
        ),
    };
    if job.pause {
        let color = if result.0 == Level::Ok { "32" } else { "31" };
        print!(
            "\n\x1b[1;{}m{}\x1b[0m\n\x1b[2mPress Enter to return to SyzAster...\x1b[0m",
            color, result.1
        );
        let _ = io::stdout().flush();
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);
    }
    if let Err(e) = term.resume() {
        return (Level::Error, format!("failed to restore terminal: {}", e));
    }
    result
}

fn event_loop(app: &mut App, term: &mut Terminal) -> io::Result<()> {
    let mut last_tick = Instant::now() - Duration::from_secs(10);
    while !app.quit {
        if term::termination_requested() {
            break;
        }
        if last_tick.elapsed() >= Duration::from_millis(1000) {
            app.tick();
            last_tick = Instant::now();
        }
        let (w, h) = term::size();
        let mut buf = Buffer::new(w, h, theme::BASE);
        app.draw(&mut buf);
        term.draw(&buf)?;
        if let Some(job) = app.job.take() {
            let (lvl, msg) = run_job(term, &job);
            app.after(job.after);
            app.set_status(lvl, msg);
            continue;
        }
        if let Some(r) = &mut app.runner {
            r.poll();
        }
        // Redraw faster while a job streams its log.
        let timeout = if app.runner.is_some() { 100 } else { 250 };
        for key in term.read_keys(timeout) {
            if key == Key::Ctrl('l') {
                term.invalidate();
                continue;
            }
            app.handle_key(key);
            if app.quit || app.job.is_some() {
                break;
            }
        }
    }
    Ok(())
}

fn print_help(prog: &str) {
    println!(
        "Usage: {prog} [OPTIONS]

Interactive terminal UI for SyzAster: prepare the Asterinas kernel, edit the
syz-manager configuration and monitor fuzzing campaigns.

Options:
  -m, --monitor <DIR>   open the Monitor tab on a syz-manager workdir
      --no-icons        use ASCII icons instead of Nerd Font glyphs
  -h, --help            show this help message

Keys: 1/2/3 switch tabs, arrows or j/k move, Enter edits or runs, Space
toggles, Tab cycles panel focus, q quits. Nerd Font glyphs render best with
a patched font; use --no-icons otherwise."
    );
}

/// Entry point for `syzaster tui`.
pub fn run_tui(prog: &str, args: &[String]) -> Result<(), i32> {
    let mut monitor_dir = None;
    let mut plain = std::env::var("SYZASTER_NO_ICONS").is_ok_and(|v| !v.is_empty() && v != "0");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help(prog);
                return Ok(());
            }
            "--no-icons" => plain = true,
            "-m" | "--monitor" => {
                let Some(dir) = args.get(i + 1) else {
                    eprintln!("missing argument for {}", args[i]);
                    return Err(1);
                };
                monitor_dir = Some(crate::pkg::realpath_m(Path::new(dir)));
                i += 1;
            }
            other => {
                if let Some(dir) = other.strip_prefix("--monitor=") {
                    monitor_dir = Some(crate::pkg::realpath_m(Path::new(dir)));
                } else {
                    eprintln!("unknown option: {}", other);
                    print_help(prog);
                    return Err(1);
                }
            }
        }
        i += 1;
    }
    if !term::is_tty() {
        eprintln!("{}: an interactive terminal is required", prog);
        return Err(1);
    }
    theme::set_plain_icons(plain);
    let root = crate::pkg::find_syzaster_root();
    let mut app = App::new(root, monitor_dir);
    term::catch_termination();
    let mut term = match Terminal::enter() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}: failed to initialize terminal: {}", prog, e);
            return Err(1);
        }
    };
    let res = event_loop(&mut app, &mut term);
    term.suspend();
    let pids: Vec<String> = app.children.iter().map(|c| c.id().to_string()).collect();
    if !pids.is_empty() {
        println!(
            "syz-manager keeps running in the background (pid {}).",
            pids.join(", ")
        );
    }
    if let Err(e) = res {
        eprintln!("{}: {}", prog, e);
        return Err(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_arguments() {
        let j = Job::new(
            "t",
            "scripts/x.sh",
            vec!["-d".into(), "/a b".into(), "it's".into()],
        );
        assert_eq!(j.display(), "scripts/x.sh -d '/a b' 'it'\\''s'");
    }

    #[test]
    fn relative_display() {
        let root = Path::new("/r");
        assert_eq!(
            rel(root, Path::new("/r/.workdir/fuzz.cfg")),
            ".workdir/fuzz.cfg"
        );
    }
}
