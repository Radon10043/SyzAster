//! Monitor tab: pick a syz-manager workdir and watch the campaign.

use super::buffer::{
    pad, pad_left, span, str_width, truncate, truncate_left, Buffer, Rect, Span, Style,
};
use super::campaign::{self, BenchLog, Issue, ManagerProc, RunState};
use super::chart::{self, ChartOpts, Series};
use super::term::{self, Key};
use super::theme::{self, icons, st};
use super::ui::{self, Dialog, Input, InputKind, Level};
use super::util::{self, Settings};
use super::{status, Action, Confirmed, Field, Job};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WINDOWS: [(&str, f64); 8] = [
    ("5m", 300.0),
    ("10m", 600.0),
    ("30m", 1800.0),
    ("1h", 3600.0),
    ("6h", 21600.0),
    ("12h", 43200.0),
    ("24h", 86400.0),
    ("All", f64::INFINITY),
];

const TILE_W: usize = 18;
const MINI_W: usize = 34;
const MINI_H: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sub {
    Fuzzing,
    Metrics,
    Issues,
}

struct Entry {
    name: String,
    path: PathBuf,
    campaign: bool,
}

/// Directory chooser with automatic campaign discovery.
struct Picker {
    focus: usize,
    found: Vec<PathBuf>,
    found_sel: usize,
    cwd: PathBuf,
    entries: Vec<Entry>,
    sel: usize,
    scroll: usize,
    found_scroll: usize,
    hidden: bool,
    scanned: bool,
}

impl Picker {
    fn new(start: PathBuf) -> Self {
        let mut p = Picker {
            focus: 0,
            found: Vec::new(),
            found_sel: 0,
            cwd: start,
            entries: Vec::new(),
            sel: 0,
            scroll: 0,
            found_scroll: 0,
            hidden: false,
            scanned: false,
        };
        p.load();
        p
    }

    fn load(&mut self) {
        let mut entries = Vec::new();
        if let Some(parent) = self.cwd.parent() {
            entries.push(Entry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                campaign: false,
            });
        }
        let mut dirs: Vec<Entry> = fs::read_dir(&self.cwd)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with('.') && !self.hidden && name != ".workdir" {
                            return None;
                        }
                        let path = e.path();
                        let campaign = campaign::is_campaign_dir(&path);
                        Some(Entry {
                            name,
                            path,
                            campaign,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        dirs.sort_by(|a, b| b.campaign.cmp(&a.campaign).then(a.name.cmp(&b.name)));
        entries.extend(dirs);
        self.entries = entries;
        self.sel = self.sel.min(self.entries.len().saturating_sub(1));
    }

    fn cd(&mut self, dir: PathBuf) {
        let from = self.cwd.clone();
        self.cwd = dir;
        self.sel = 0;
        self.scroll = 0;
        self.load();
        // Returning to the parent keeps the previous directory highlighted.
        if let Some(i) = self.entries.iter().position(|e| e.path == from) {
            self.sel = i;
        }
    }

    fn scan(&mut self, root: &Path) {
        let mut roots = vec![root.join(".workdir")];
        for r in ["/vol/experiment", "/vol"] {
            roots.push(PathBuf::from(r));
        }
        roots.push(root.to_path_buf());
        self.found = campaign::discover(&roots, 5, 6000);
        self.found_sel = self.found_sel.min(self.found.len().saturating_sub(1));
        self.scanned = true;
        if self.found.is_empty() {
            self.focus = 1;
        }
    }
}

/// Live state of the monitored workdir.
struct Campaign {
    dir: PathBuf,
    bench: BenchLog,
    issues: Vec<Issue>,
    state: RunState,
    manager: Option<ManagerProc>,
    tail: Vec<String>,
    cfg_workdir: Option<PathBuf>,
    t_fast: Option<Instant>,
    t_issues: Option<Instant>,
    t_procs: Option<Instant>,
    report: Option<(String, Vec<String>)>,
    gone_at: Option<std::time::SystemTime>,
}

impl Campaign {
    fn new(dir: PathBuf) -> Self {
        let cfg_workdir = campaign::config_workdir(&dir.join("fuzz.cfg"));
        let mut c = Campaign {
            bench: BenchLog::new(dir.join("bench.log")),
            dir,
            issues: Vec::new(),
            state: RunState::Stopped,
            manager: None,
            tail: Vec::new(),
            cfg_workdir,
            t_fast: None,
            t_issues: None,
            t_procs: None,
            report: None,
            gone_at: None,
        };
        c.refresh(true);
        c
    }

    fn refresh(&mut self, force: bool) {
        let due = |t: &Option<Instant>, secs: u64| {
            force || t.is_none_or(|t| t.elapsed() >= Duration::from_secs(secs))
        };
        if due(&self.t_fast, 2) {
            self.bench.refresh();
            self.tail = campaign::tail_lines(&self.dir.join("fuzz.log"), 32 * 1024, 60);
            self.t_fast = Some(Instant::now());
        }
        if due(&self.t_issues, 5) {
            self.issues = campaign::scan_issues(&self.dir);
            self.t_issues = Some(Instant::now());
        }
        if due(&self.t_procs, 3) {
            let procs = campaign::scan_managers();
            self.manager = campaign::find_manager(&self.dir, &procs).cloned();
            let was_local = matches!(self.state, RunState::Local(_));
            if was_local && self.manager.is_none() {
                self.gone_at = Some(std::time::SystemTime::now());
            }
            self.state = campaign::run_state(&self.dir, &procs, self.gone_at);
            self.t_procs = Some(Instant::now());
        }
    }

    /// Campaign uptime, extrapolated while syz-manager is alive.
    fn uptime(&self) -> Option<f64> {
        let last = self.bench.t.last().copied()?;
        let extra = match (&self.state, self.bench.mtime) {
            (RunState::Stopped, _) | (_, None) => 0.0,
            (_, Some(m)) => campaign::age_secs(m).min(campaign::BENCH_PERIOD * 2.0),
        };
        Some(last + extra)
    }

    /// The config's workdir when it differs from the selected directory,
    /// which hints that the campaign was started in another mount namespace.
    fn foreign_workdir(&self) -> Option<&Path> {
        let w = self.cfg_workdir.as_deref()?;
        let same = fs::canonicalize(w).ok() == fs::canonicalize(&self.dir).ok();
        if same {
            None
        } else {
            Some(w)
        }
    }
}

pub struct MonitorTab {
    root: PathBuf,
    picker: Picker,
    picking: bool,
    camp: Option<Campaign>,
    sub: Sub,
    window: usize,
    metric_sel: usize,
    metric_row: usize,
    metric_cols: usize,
    filter: String,
    zoom: bool,
    issue_sel: usize,
    issue_scroll: usize,
    report_scroll: usize,
    sigint_at: Option<(i32, Instant)>,
}

impl MonitorTab {
    pub fn new(root: &Path, s: &Settings) -> Self {
        let last = s
            .get("monitor.dir")
            .map(PathBuf::from)
            .filter(|p| p.is_dir());
        let start = last
            .as_ref()
            .and_then(|p| p.parent().map(|q| q.to_path_buf()))
            .or_else(|| Some(root.join(".workdir")).filter(|p| p.is_dir()))
            .unwrap_or_else(|| root.to_path_buf());
        let mut picker = Picker::new(start);
        if let Some(l) = &last {
            if let Some(i) = picker.entries.iter().position(|e| &e.path == l) {
                picker.sel = i;
                picker.focus = 1;
            }
        }
        MonitorTab {
            root: root.to_path_buf(),
            picker,
            picking: true,
            camp: None,
            sub: Sub::Fuzzing,
            window: 3,
            metric_sel: 0,
            metric_row: 0,
            metric_cols: 1,
            filter: String::new(),
            zoom: false,
            issue_sel: 0,
            issue_scroll: 0,
            report_scroll: 0,
            sigint_at: None,
        }
    }

    pub fn select(&mut self, dir: PathBuf) {
        self.camp = Some(Campaign::new(dir));
        self.picking = false;
        self.issue_sel = 0;
        self.report_scroll = 0;
        self.metric_sel = 0;
        self.zoom = false;
        self.sigint_at = None;
    }

    pub fn tick(&mut self) {
        if !self.picking {
            if let Some(c) = &mut self.camp {
                c.refresh(false);
            }
        }
    }

    pub fn set_field(&mut self, field: Field, value: &str) -> Vec<Action> {
        match field {
            Field::MonitorPath => {
                let p = util::expand(value, &self.picker.cwd);
                if !p.is_dir() {
                    return status(
                        Level::Error,
                        format!("{} is not a directory", util::tilde(&p)),
                    );
                }
                if campaign::is_campaign_dir(&p) {
                    vec![Action::Monitor(p)]
                } else {
                    self.picker.cd(p);
                    self.picker.focus = 1;
                    status(
                        Level::Info,
                        "Not a syz-manager workdir; browsing it instead",
                    )
                }
            }
            Field::MetricFilter => {
                self.filter = value.trim().to_string();
                self.metric_sel = 0;
                self.metric_row = 0;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sends a signal to the monitored syz-manager.
    pub fn signal(&mut self, pid: i32, sig: i32) -> Vec<Action> {
        let name = if sig == term::SIGKILL {
            "SIGKILL"
        } else {
            "SIGINT"
        };
        match term::send_signal(pid, sig) {
            Ok(()) => {
                if sig == term::SIGINT {
                    self.sigint_at = Some((pid, Instant::now()));
                }
                if let Some(c) = &mut self.camp {
                    c.t_procs = None;
                }
                status(
                    Level::Ok,
                    if sig == term::SIGINT {
                        format!(
                            "{} sent to syz-manager (pid {}): shutting down gracefully",
                            name, pid
                        )
                    } else {
                        format!("{} sent to syz-manager (pid {})", name, pid)
                    },
                )
            }
            Err(e) => status(Level::Error, format!("cannot signal pid {}: {}", pid, e)),
        }
    }

    fn range(&self, b: &BenchLog) -> (f64, f64) {
        let x1 = b.last_t();
        let first = b.first_t().min(x1);
        let win = WINDOWS[self.window].1;
        let mut x0 = if win.is_finite() {
            (x1 - win).max(first)
        } else {
            first
        };
        if x1 - x0 < campaign::BENCH_PERIOD {
            x0 = x1 - campaign::BENCH_PERIOD;
        }
        (x0, x1)
    }

    fn metric_names(&self) -> Vec<String> {
        let Some(c) = &self.camp else {
            return Vec::new();
        };
        let f = self.filter.to_lowercase();
        c.bench
            .series
            .keys()
            .filter(|k| f.is_empty() || k.to_lowercase().contains(&f))
            .cloned()
            .collect()
    }

    fn stop(&mut self) -> Vec<Action> {
        let ic = icons();
        let Some(c) = &self.camp else {
            return Vec::new();
        };
        match &c.state {
            RunState::Local(pid) => {
                let pid = *pid;
                let forced = self
                    .sigint_at
                    .is_some_and(|(p, t)| p == pid && t.elapsed() >= Duration::from_secs(3));
                let (sig, title, yes, extra) = if forced {
                    (term::SIGKILL, "Force kill syz-manager", "Kill", "syz-manager is still alive after SIGINT. SIGKILL terminates it immediately; QEMU instances are reaped with it.")
                } else {
                    (term::SIGINT, "Stop fuzzing", "Stop", "syz-manager receives SIGINT and shuts down gracefully: VMs are stopped and the corpus is kept.")
                };
                let mut lines = vec![vec![
                    span("Process  ", st(theme::MUTED)),
                    span(format!("syz-manager pid {}", pid), st(theme::YELLOW).bold()),
                ]];
                if let Some(m) = &c.manager {
                    if let Some(cfg) = &m.config {
                        lines.push(vec![
                            span("Config   ", st(theme::MUTED)),
                            span(util::tilde(cfg), st(theme::CYAN)),
                        ]);
                    }
                }
                lines.push(vec![
                    span("Workdir  ", st(theme::MUTED)),
                    span(util::tilde(&c.dir), st(theme::CYAN)),
                ]);
                lines.push(vec![]);
                lines.push(vec![span(extra, theme::BASE)]);
                vec![Action::Dialog(
                    Dialog::confirm(title, lines, yes, Confirmed::Signal { pid, sig })
                        .level(Level::Error),
                )]
            }
            RunState::Remote => {
                let mut lines = vec![
                    vec![span(
                        "This campaign is still being written, but no syz-manager",
                        theme::BASE,
                    )],
                    vec![span(
                        "process in this container's PID namespace owns it.",
                        theme::BASE,
                    )],
                    vec![],
                    vec![span(
                        "It is most likely driven by another container (for example a",
                        st(theme::MUTED),
                    )],
                    vec![span(
                        "docker compose experiment service) and cannot be stopped here.",
                        st(theme::MUTED),
                    )],
                ];
                if let Some(w) = c.foreign_workdir() {
                    lines.push(vec![]);
                    lines.push(vec![
                        span("Config workdir  ", st(theme::MUTED)),
                        span(util::tilde(w), st(theme::YELLOW)),
                    ]);
                }
                lines.push(vec![]);
                lines.push(vec![
                    span(
                        format!("{} Stop it from the host, e.g. ", ic.info),
                        st(theme::BLUE),
                    ),
                    span("docker stop <container>", st(theme::GREEN)),
                ]);
                vec![Action::Dialog(Dialog::message(
                    "Cannot stop from this container",
                    Level::Warn,
                    lines,
                ))]
            }
            RunState::Stopped => status(Level::Info, "No running syz-manager for this workdir"),
        }
    }

    fn open_issue(&self) -> Vec<Action> {
        let Some(c) = &self.camp else {
            return Vec::new();
        };
        let Some(issue) = c.issues.get(self.issue_sel) else {
            return Vec::new();
        };
        let file = ["report0", "log0", "description"]
            .iter()
            .map(|f| issue.dir.join(f))
            .find(|p| p.is_file());
        let Some(file) = file else {
            return status(Level::Warn, "No report or log for this issue");
        };
        let (prog, args) = if crate::pkg::command_exists("less") {
            (
                "less".to_string(),
                vec!["-R".to_string(), file.display().to_string()],
            )
        } else {
            (
                super::editor(),
                vec!["-R".to_string(), file.display().to_string()],
            )
        };
        let mut job = Job::new("View crash report", prog, args);
        job.pause = false;
        vec![Action::Run(job)]
    }

    pub fn handle_key(&mut self, key: Key) -> Vec<Action> {
        if self.picking || self.camp.is_none() {
            return self.picker_key(key);
        }
        match key {
            Key::Char('c') => {
                self.picking = true;
                self.picker.load();
                return Vec::new();
            }
            Key::Char('x') => return self.stop(),
            Key::Char('r') => {
                if let Some(c) = &mut self.camp {
                    c.refresh(true);
                }
                return status(Level::Info, "Refreshed");
            }
            Key::Char('f') => self.sub = Sub::Fuzzing,
            Key::Char('m') => self.sub = Sub::Metrics,
            Key::Char('i') => self.sub = Sub::Issues,
            Key::Tab => {
                self.sub = match self.sub {
                    Sub::Fuzzing => Sub::Metrics,
                    Sub::Metrics => Sub::Issues,
                    Sub::Issues => Sub::Fuzzing,
                }
            }
            Key::BackTab => {
                self.sub = match self.sub {
                    Sub::Fuzzing => Sub::Issues,
                    Sub::Metrics => Sub::Fuzzing,
                    Sub::Issues => Sub::Metrics,
                }
            }
            Key::Char('[') => self.window = self.window.saturating_sub(1),
            Key::Char(']') => self.window = (self.window + 1).min(WINDOWS.len() - 1),
            _ => {}
        }
        match self.sub {
            Sub::Fuzzing => match key {
                Key::Left | Key::Char('h') => self.window = self.window.saturating_sub(1),
                Key::Right | Key::Char('l') => {
                    self.window = (self.window + 1).min(WINDOWS.len() - 1)
                }
                _ => {}
            },
            Sub::Metrics => {
                let n = self.metric_names().len();
                let cols = self.metric_cols.max(1);
                match key {
                    Key::Left | Key::Char('h') if !self.zoom => {
                        self.metric_sel = self.metric_sel.saturating_sub(1)
                    }
                    Key::Right | Key::Char('l') if !self.zoom => {
                        self.metric_sel = (self.metric_sel + 1).min(n.saturating_sub(1))
                    }
                    Key::Up | Key::Char('k') if !self.zoom => {
                        self.metric_sel = self.metric_sel.saturating_sub(cols)
                    }
                    Key::Down | Key::Char('j') if !self.zoom => {
                        if self.metric_sel + cols < n {
                            self.metric_sel += cols;
                        }
                    }
                    Key::Left | Key::Char('h') | Key::Up | Key::Char('k') if self.zoom => {
                        self.metric_sel = self.metric_sel.saturating_sub(1);
                    }
                    Key::Right | Key::Char('l') | Key::Down | Key::Char('j') if self.zoom => {
                        self.metric_sel = (self.metric_sel + 1).min(n.saturating_sub(1));
                    }
                    Key::PageUp => self.metric_sel = self.metric_sel.saturating_sub(cols * 3),
                    Key::PageDown => {
                        self.metric_sel = (self.metric_sel + cols * 3).min(n.saturating_sub(1))
                    }
                    Key::Home | Key::Char('g') => self.metric_sel = 0,
                    Key::End | Key::Char('G') => self.metric_sel = n.saturating_sub(1),
                    Key::Enter => self.zoom = !self.zoom && n > 0,
                    Key::Esc => {
                        if self.zoom {
                            self.zoom = false;
                        } else if !self.filter.is_empty() {
                            self.filter.clear();
                        }
                    }
                    Key::Char('/') => {
                        return vec![Action::Input(
                            Field::MetricFilter,
                            Input::new(
                                "Filter metrics",
                                "Show metrics whose name contains (empty shows all):",
                                InputKind::Text,
                                &self.filter,
                                &self.root,
                            ),
                        )]
                    }
                    _ => {}
                }
            }
            Sub::Issues => {
                let n = self.camp.as_ref().map_or(0, |c| c.issues.len());
                let before = self.issue_sel;
                match key {
                    Key::Up | Key::Char('k') => self.issue_sel = self.issue_sel.saturating_sub(1),
                    Key::Down | Key::Char('j') => {
                        self.issue_sel = (self.issue_sel + 1).min(n.saturating_sub(1))
                    }
                    Key::Home | Key::Char('g') => self.issue_sel = 0,
                    Key::End | Key::Char('G') => self.issue_sel = n.saturating_sub(1),
                    Key::PageUp => self.report_scroll = self.report_scroll.saturating_sub(10),
                    Key::PageDown => self.report_scroll += 10,
                    Key::Enter => return self.open_issue(),
                    _ => {}
                }
                if before != self.issue_sel {
                    self.report_scroll = 0;
                }
            }
        }
        Vec::new()
    }

    fn picker_key(&mut self, key: Key) -> Vec<Action> {
        let p = &mut self.picker;
        match key {
            Key::Tab | Key::BackTab => {
                p.focus = 1 - p.focus;
                return Vec::new();
            }
            Key::Esc => {
                if self.camp.is_some() {
                    self.picking = false;
                }
                return Vec::new();
            }
            Key::Char('r') => {
                p.scan(&self.root);
                p.load();
                return status(Level::Info, format!("Found {} campaign(s)", p.found.len()));
            }
            Key::Char('/') | Key::Char('p') => {
                return vec![Action::Input(
                    Field::MonitorPath,
                    Input::new(
                        "Monitor workdir",
                        "syz-manager workdir to monitor (Tab completes):",
                        InputKind::Path,
                        &format!("{}/", p.cwd.display()),
                        &p.cwd,
                    ),
                )]
            }
            Key::Char('.') => {
                p.hidden = !p.hidden;
                p.load();
                return Vec::new();
            }
            _ => {}
        }
        if p.focus == 0 {
            let n = p.found.len();
            match key {
                Key::Up | Key::Char('k') => p.found_sel = p.found_sel.saturating_sub(1),
                Key::Down | Key::Char('j') => {
                    p.found_sel = (p.found_sel + 1).min(n.saturating_sub(1))
                }
                Key::Home | Key::Char('g') => p.found_sel = 0,
                Key::End | Key::Char('G') => p.found_sel = n.saturating_sub(1),
                Key::Enter | Key::Char(' ') | Key::Char('s') => {
                    if let Some(d) = p.found.get(p.found_sel) {
                        return vec![Action::Monitor(d.clone())];
                    }
                }
                Key::Right | Key::Char('l') => {
                    if let Some(d) = p.found.get(p.found_sel).cloned() {
                        if let Some(parent) = d.parent() {
                            p.cd(parent.to_path_buf());
                            if let Some(i) = p.entries.iter().position(|e| e.path == d) {
                                p.sel = i;
                            }
                            p.focus = 1;
                        }
                    }
                }
                _ => {}
            }
            return Vec::new();
        }
        let n = p.entries.len();
        match key {
            Key::Up | Key::Char('k') => p.sel = p.sel.saturating_sub(1),
            Key::Down | Key::Char('j') => p.sel = (p.sel + 1).min(n.saturating_sub(1)),
            Key::PageUp => p.sel = p.sel.saturating_sub(10),
            Key::PageDown => p.sel = (p.sel + 10).min(n.saturating_sub(1)),
            Key::Home | Key::Char('g') => p.sel = 0,
            Key::End | Key::Char('G') => p.sel = n.saturating_sub(1),
            Key::Backspace | Key::Left | Key::Char('h') => {
                if let Some(parent) = p.cwd.parent().map(|q| q.to_path_buf()) {
                    p.cd(parent);
                }
            }
            Key::Right | Key::Char('l') => {
                if let Some(e) = p.entries.get(p.sel) {
                    let path = e.path.clone();
                    p.cd(path);
                }
            }
            Key::Enter => {
                if let Some(e) = p.entries.get(p.sel) {
                    if e.campaign {
                        return vec![Action::Monitor(e.path.clone())];
                    }
                    let path = e.path.clone();
                    p.cd(path);
                }
            }
            Key::Char(' ') | Key::Char('s') => {
                if let Some(e) = p.entries.get(p.sel) {
                    if e.name == ".." {
                        return Vec::new();
                    }
                    return vec![Action::Monitor(e.path.clone())];
                }
            }
            _ => {}
        }
        Vec::new()
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) -> Vec<(&'static str, &'static str)> {
        if self.picking || self.camp.is_none() {
            return self.draw_picker(buf, area);
        }
        let (head, body) = area.split_top(2);
        self.draw_header(buf, head);
        let body = Rect::new(body.x, body.y, body.w, body.h);
        match self.sub {
            Sub::Fuzzing => {
                self.draw_fuzzing(buf, body);
                vec![
                    ("←→", "Window"),
                    ("Tab", "Sub-tab"),
                    ("f/m/i", "Fuzzing/Metrics/Issues"),
                    ("x", "Stop"),
                    ("c", "Change dir"),
                    ("r", "Refresh"),
                ]
            }
            Sub::Metrics => {
                self.draw_metrics(buf, body);
                if self.zoom {
                    vec![
                        ("←→", "Prev/next metric"),
                        ("[ ]", "Window"),
                        ("Enter", "Grid"),
                        ("Tab", "Sub-tab"),
                        ("x", "Stop"),
                        ("c", "Change dir"),
                    ]
                } else {
                    vec![
                        ("Arrows", "Select"),
                        ("Enter", "Zoom"),
                        ("[ ]", "Window"),
                        ("/", "Filter"),
                        ("Tab", "Sub-tab"),
                        ("x", "Stop"),
                        ("c", "Change dir"),
                    ]
                }
            }
            Sub::Issues => {
                self.draw_issues(buf, body);
                vec![
                    ("↑↓", "Select"),
                    ("PgUp/PgDn", "Scroll report"),
                    ("Enter", "Open report"),
                    ("Tab", "Sub-tab"),
                    ("x", "Stop"),
                    ("c", "Change dir"),
                ]
            }
        }
    }

    fn draw_picker(&mut self, buf: &mut Buffer, area: Rect) -> Vec<(&'static str, &'static str)> {
        let ic = icons();
        if !self.picker.scanned {
            self.picker.scan(&self.root);
        }
        let cols = area.columns(&[1, 1], 0);
        let p = &mut self.picker;

        // Discovered campaigns.
        let inner = ui::panel(buf, cols[0], ic.radar, "Discovered Campaigns", p.focus == 0);
        ui::panel_note(
            buf,
            cols[0],
            &[span(format!(" {} found ", p.found.len()), st(theme::MUTED))],
        );
        if p.found.is_empty() {
            let lines = [
                format!("{} No syz-manager workdirs found.", ic.info),
                String::new(),
                "Searched .workdir/ and /vol for directories containing bench.log, or crashes/ with corpus.db.".to_string(),
                String::new(),
                "Browse on the right or press / to type a path.".to_string(),
            ];
            let text = lines.join("\n");
            for (i, l) in super::buffer::wrap(&text, inner.w.saturating_sub(4))
                .iter()
                .enumerate()
                .take(inner.h.saturating_sub(1))
            {
                buf.text(
                    inner.x + 2,
                    inner.y + 1 + i,
                    l,
                    st(theme::MUTED),
                    inner.right(),
                );
            }
        } else {
            let rows = inner.h / 2;
            if p.found_sel < p.found_scroll {
                p.found_scroll = p.found_sel;
            }
            if p.found_sel >= p.found_scroll + rows {
                p.found_scroll = p.found_sel + 1 - rows;
            }
            for (row, (i, dir)) in p
                .found
                .iter()
                .enumerate()
                .skip(p.found_scroll)
                .take(rows)
                .enumerate()
            {
                let y = inner.y + row * 2;
                let sel = i == p.found_sel;
                let bg = if sel && p.focus == 0 {
                    theme::BG_SEL
                } else {
                    theme::BG
                };
                buf.fill(
                    Rect::new(inner.x, y, inner.w, 2),
                    ' ',
                    Style::new(theme::TEXT, bg),
                );
                if sel {
                    buf.set(inner.x, y, '▌', Style::new(theme::CYAN, bg));
                    buf.set(inner.x, y + 1, '▌', Style::new(theme::CYAN, bg));
                }
                let age = campaign::activity_age(dir);
                let live = age.is_some_and(|a| a < Duration::from_secs(150));
                let (dot, dc) = if live {
                    (ic.dot, theme::GREEN)
                } else {
                    (ic.dot, theme::SUBTLE)
                };
                let name = dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let parent = dir.parent().map(util::tilde).unwrap_or_default();
                buf.spans(
                    inner.x + 2,
                    y,
                    &[
                        span(format!("{} ", dot), Style::new(dc, bg)),
                        span(
                            name,
                            Style::new(if sel { theme::WHITE } else { theme::TEXT }, bg).bold(),
                        ),
                    ],
                    inner.right().saturating_sub(14),
                );
                let note = match age {
                    Some(a) if live => format!("live · {}", util::ago(a.as_secs_f64())),
                    Some(a) => util::ago(a.as_secs_f64()),
                    None => "no logs".to_string(),
                };
                buf.text_right(
                    inner.right().saturating_sub(1),
                    y,
                    &note,
                    Style::new(if live { theme::GREEN } else { theme::MUTED }, bg),
                );
                buf.text(
                    inner.x + 4,
                    y + 1,
                    &truncate_left(&parent, inner.w.saturating_sub(6)),
                    Style::new(theme::MUTED, bg),
                    inner.right(),
                );
            }
        }

        // Directory browser.
        let title = format!("Browse · {}", util::tilde(&p.cwd));
        let inner = ui::panel(buf, cols[1], ic.folder_open, &title, p.focus == 1);
        let rows = inner.h;
        if p.sel < p.scroll {
            p.scroll = p.sel;
        }
        if p.sel >= p.scroll + rows {
            p.scroll = p.sel + 1 - rows;
        }
        for (row, (i, e)) in p
            .entries
            .iter()
            .enumerate()
            .skip(p.scroll)
            .take(rows)
            .enumerate()
        {
            let y = inner.y + row;
            let sel = i == p.sel;
            let bg = if sel && p.focus == 1 {
                theme::BG_SEL
            } else {
                theme::BG
            };
            buf.fill(
                Rect::new(inner.x, y, inner.w, 1),
                ' ',
                Style::new(theme::TEXT, bg),
            );
            if sel {
                buf.set(inner.x, y, '▌', Style::new(theme::CYAN, bg));
            }
            let (icon, c) = if e.campaign {
                (ic.monitor, theme::GREEN)
            } else if e.name == ".." {
                (ic.folder_open, theme::MUTED)
            } else {
                (ic.folder, theme::BLUE)
            };
            buf.spans(
                inner.x + 2,
                y,
                &[
                    span(format!("{} ", icon), Style::new(c, bg)),
                    span(
                        e.name.clone(),
                        Style::new(
                            if e.campaign {
                                theme::GREEN
                            } else {
                                theme::TEXT
                            },
                            bg,
                        ),
                    ),
                ],
                inner.right().saturating_sub(12),
            );
            if e.campaign {
                buf.text_right(
                    inner.right().saturating_sub(1),
                    y,
                    "workdir",
                    Style::new(theme::GREEN, bg).italic(),
                );
            }
        }
        let mut h = vec![("↑↓", "Move"), ("Tab", "Switch panel")];
        if p.focus == 0 {
            h.push(("Enter", "Monitor"));
            h.push(("→", "Reveal"));
        } else {
            h.extend([
                ("Enter", "Open/Monitor"),
                ("Space", "Monitor dir"),
                ("←", "Parent"),
                (".", "Hidden"),
            ]);
        }
        h.extend([("/", "Type path"), ("r", "Rescan")]);
        if self.camp.is_some() {
            h.push(("Esc", "Back"));
        }
        h
    }

    fn draw_header(&self, buf: &mut Buffer, r: Rect) {
        let ic = icons();
        let Some(c) = &self.camp else { return };
        let y = r.y;
        buf.fill(
            Rect::new(r.x, y, r.w, 1),
            ' ',
            Style::new(theme::TEXT, theme::BG_ALT),
        );
        let bg = theme::BG_ALT;
        let narrow = r.w < 110;
        let (badge, bc, detail) = match &c.state {
            RunState::Local(pid) => (" ● RUNNING ", theme::GREEN, format!(" pid {} · local", pid)),
            RunState::Remote if narrow => (" ● RUNNING ", theme::ORANGE, " remote".to_string()),
            RunState::Remote => (
                " ● RUNNING ",
                theme::ORANGE,
                " remote · outside this container".to_string(),
            ),
            RunState::Stopped => (
                " ■ STOPPED ",
                theme::SUBTLE,
                match campaign::activity_age(&c.dir) {
                    Some(a) if !narrow => format!(" last activity {}", util::ago(a.as_secs_f64())),
                    Some(a) => format!(" {}", util::ago(a.as_secs_f64())),
                    None => " no activity".to_string(),
                },
            ),
        };
        let (btn, btn_st) = match &c.state {
            RunState::Local(_) => {
                let forced = self
                    .sigint_at
                    .is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(3));
                let label = match (forced, narrow) {
                    (true, _) => "FORCE KILL",
                    (false, true) => "STOP",
                    (false, false) => "STOP FUZZER",
                };
                (
                    format!(" {} {} [x] ", ic.stop, label),
                    Style::new(theme::WHITE, theme::RED).bold(),
                )
            }
            RunState::Remote if narrow => (
                format!(" {} CAN'T STOP [x] ", ic.stop),
                Style::new(theme::MUTED, theme::BG_SEL),
            ),
            RunState::Remote => (
                format!(" {} STOP UNAVAILABLE [x] ", ic.stop),
                Style::new(theme::MUTED, theme::BG_SEL),
            ),
            RunState::Stopped => (
                format!(" {} NOT RUNNING ", ic.stop),
                Style::new(theme::SUBTLE, theme::BG_SEL),
            ),
        };
        let bw = str_width(&btn);
        let bx = r.right().saturating_sub(bw + 1);
        // Mandatory parts first, optional ones only while they fit.
        let mut spans = vec![
            span(badge, Style::new(theme::BG, bc).bold()),
            span(detail, Style::new(bc, bg)),
            span(format!("   {} ", ic.clock), Style::new(theme::SUBTLE, bg)),
            span(
                c.uptime()
                    .map(util::dur_long)
                    .unwrap_or_else(|| "-".to_string()),
                Style::new(theme::WHITE, bg).bold(),
            ),
        ];
        let limit = bx.saturating_sub(r.x + 3);
        if let Some(m) = c.bench.mtime {
            let extra = [
                span("   last sample ", Style::new(theme::SUBTLE, bg)),
                span(
                    util::ago(campaign::age_secs(m)),
                    Style::new(theme::MUTED, bg),
                ),
            ];
            if Buffer::spans_width(&spans) + Buffer::spans_width(&extra) <= limit {
                spans.extend(extra);
            }
        }
        let x = buf.spans(r.x + 1, y, &spans, bx);
        let avail = bx.saturating_sub(x + 6);
        if avail >= 12 {
            let path = truncate_left(&util::tilde(&c.dir), avail);
            buf.spans(
                x,
                y,
                &[
                    span(
                        format!("   {} ", ic.folder_open),
                        Style::new(theme::SUBTLE, bg),
                    ),
                    span(path, Style::new(theme::MUTED, bg)),
                ],
                bx,
            );
        }
        buf.text(bx, y, &btn, btn_st, r.right());

        // Sub-tabs and window selector.
        let y = r.y + 1;
        let specs = [
            ui::TabSpec {
                key: "f",
                icon: ic.fuzzing,
                label: "Fuzzing",
            },
            ui::TabSpec {
                key: "m",
                icon: ic.metrics,
                label: "Metrics",
            },
            ui::TabSpec {
                key: "i",
                icon: ic.issues,
                label: "Issues",
            },
        ];
        let active = match self.sub {
            Sub::Fuzzing => 0,
            Sub::Metrics => 1,
            Sub::Issues => 2,
        };
        let mut x = ui::tabs(buf, r.x + 1, y, r.right(), &specs, active, true);
        if !c.issues.is_empty() {
            // Issue count badge next to the tab bar.
            let n = format!(" {} ", c.issues.len());
            x = buf.text(
                x,
                y,
                &n,
                Style::new(theme::BG, theme::RED).bold(),
                r.right(),
            );
        }
        if self.sub != Sub::Issues {
            let mut full = vec![
                span(format!("{} ", ic.clock), st(theme::SUBTLE)),
                span("◂ ", st(theme::SUBTLE)),
            ];
            for (i, (label, _)) in WINDOWS.iter().enumerate() {
                let stl = if i == self.window {
                    Style::new(theme::BG, theme::CYAN).bold()
                } else {
                    st(theme::MUTED)
                };
                full.push(span(format!(" {} ", label), stl));
            }
            full.push(span(" ▸", st(theme::SUBTLE)));
            let compact = vec![
                span(format!("{} window ", ic.clock), st(theme::SUBTLE)),
                span("◂ ", st(theme::SUBTLE)),
                span(
                    format!(" {} ", WINDOWS[self.window].0),
                    Style::new(theme::BG, theme::CYAN).bold(),
                ),
                span(" ▸", st(theme::SUBTLE)),
            ];
            let room = r.right().saturating_sub(x + 3);
            let spans = if Buffer::spans_width(&full) <= room {
                full
            } else {
                compact
            };
            let w = Buffer::spans_width(&spans);
            if w <= room {
                buf.spans(r.right().saturating_sub(w + 1), y, &spans, r.right());
            }
        }
    }

    fn tile(
        &self,
        buf: &mut Buffer,
        r: Rect,
        label: &str,
        value: String,
        sub: Option<Span>,
        color: super::buffer::Rgb,
    ) {
        buf.border(r, super::buffer::ROUNDED, st(theme::BORDER));
        buf.text(
            r.x + 2,
            r.y,
            &format!(" {} ", label),
            st(theme::MUTED),
            r.right().saturating_sub(1),
        );
        let inner = r.inset(2, 1);
        buf.text(
            inner.x,
            inner.y,
            &value,
            Style::new(color, theme::BG).bold(),
            inner.right(),
        );
        if let (Some((txt, stl)), true) = (sub, inner.h > 1) {
            buf.text(inner.x, inner.y + 1, &txt, stl, inner.right());
        }
    }

    /// Bordered chart; `head` is the panel icon, title and border note.
    fn chart_panel(
        &self,
        buf: &mut Buffer,
        r: Rect,
        head: (&str, &str, Vec<Span>),
        series: &[Series],
        (x0, x1): (f64, f64),
        y_zero: bool,
    ) {
        let (icon, title, note) = head;
        if r.h < 4 || r.w < 12 {
            return;
        }
        let inner = ui::panel(buf, r, icon, title, false);
        ui::panel_note(buf, r, &note);
        let opts = ChartOpts {
            x_range: (x0, x1),
            y_zero,
            axes: true,
            x_fmt: util::elapsed,
        };
        chart::draw(
            buf,
            Rect::new(inner.x + 1, inner.y, inner.w.saturating_sub(2), inner.h),
            series,
            &opts,
        );
    }

    fn draw_fuzzing(&self, buf: &mut Buffer, r: Rect) {
        let ic = icons();
        let Some(c) = &self.camp else { return };
        let b = &c.bench;
        if b.is_empty() {
            self.draw_no_bench(buf, r, c);
            return;
        }
        let range = self.range(b);
        let delta = |name: &str| -> Option<f64> {
            let s = b.get(name)?;
            let v1 = b.last(name)?;
            let v0 = chart::value_at(&b.t, s, range.0);
            v0.is_finite().then_some(v1 - v0)
        };
        let rate = b.rate("exec total");
        let last_rate = rate.iter().rev().find(|v| v.is_finite()).copied();
        let win = WINDOWS[self.window].0;
        let sub = |d: Option<f64>| -> Option<Span> {
            Some(match d? {
                d if d > 0.0 => span(format!("▲ +{} / {}", util::num(d), win), st(theme::GREEN)),
                d if d < 0.0 => span(format!("▼ {} / {}", util::num(d), win), st(theme::ORANGE)),
                _ => span(format!("· flat / {}", win), st(theme::SUBTLE)),
            })
        };
        let avg_rate = delta("exec total").map(|d| d / (range.1 - range.0).max(1.0));
        let vms = b.get("fuzzing VMs").map(|v| stats(&b.t, v, range));
        let tiles: Vec<(&str, String, Option<Span>, super::buffer::Rgb)> = vec![
            (
                "Coverage",
                b.last("coverage")
                    .map(util::grouped)
                    .unwrap_or_else(|| "-".into()),
                sub(delta("coverage")),
                theme::CYAN,
            ),
            (
                "Crashes",
                b.last("crashes")
                    .map(util::grouped)
                    .unwrap_or_else(|| "0".into()),
                sub(delta("crashes")),
                theme::RED,
            ),
            (
                "Crash types",
                b.last("crash types")
                    .map(util::grouped)
                    .unwrap_or_else(|| "0".into()),
                sub(delta("crash types")),
                theme::ORANGE,
            ),
            (
                "Corpus",
                b.last("corpus")
                    .map(util::grouped)
                    .unwrap_or_else(|| "-".into()),
                sub(delta("corpus")),
                theme::PURPLE,
            ),
            (
                "Signal",
                b.last("signal")
                    .map(util::grouped)
                    .unwrap_or_else(|| "-".into()),
                sub(delta("signal")),
                theme::BLUE,
            ),
            (
                "Exec / sec",
                last_rate
                    .map(|v| format!("{:.1}", v))
                    .unwrap_or_else(|| "-".into()),
                avg_rate.map(|a| span(format!("avg {:.1} / {}", a, win), st(theme::MUTED))),
                theme::GREEN,
            ),
            (
                "Exec total",
                b.last("exec total")
                    .map(util::num)
                    .unwrap_or_else(|| "-".into()),
                sub(delta("exec total")),
                theme::GREEN,
            ),
            (
                "Fuzzing VMs",
                b.last("fuzzing VMs")
                    .map(util::grouped)
                    .unwrap_or_else(|| "-".into()),
                vms.map(|(lo, hi, _)| {
                    span(
                        format!("min {} max {}", util::num(lo), util::num(hi)),
                        st(theme::MUTED),
                    )
                }),
                theme::YELLOW,
            ),
        ];
        let n = (r.w / TILE_W).clamp(3, tiles.len());
        let (trow, rest) = r.split_top(4);
        for (rect, (label, value, d, color)) in trow.columns(&vec![1; n], 0).into_iter().zip(tiles)
        {
            self.tile(buf, rect, label, value, d, color);
        }

        let empty: Vec<f64> = Vec::new();
        let get = |n: &str| b.get(n).unwrap_or(&empty);
        let cov = get("coverage");
        let window = WINDOWS[self.window].0;
        let has_second = rest.h >= 20;
        let main_h = if has_second {
            (rest.h * 11 / 20).max(10)
        } else {
            rest.h
        };
        let (main, rest2) = rest.split_top(main_h);
        let mc = main.columns(&[62, 38], 0);
        self.chart_panel(
            buf,
            mc[0],
            (
                ic.radar,
                &format!("Coverage · {}", window),
                vec![
                    span(
                        b.last("coverage").map(util::grouped).unwrap_or_default(),
                        st(theme::CYAN).bold(),
                    ),
                    span(" PCs ", st(theme::MUTED)),
                ],
            ),
            &[Series {
                xs: &b.t,
                ys: cov,
                color: theme::CYAN,
                fill: true,
            }],
            range,
            false,
        );
        self.chart_panel(
            buf,
            mc[1],
            (
                ic.bug,
                &format!("Issues · {}", window),
                vec![
                    span("━ ", st(theme::RED)),
                    span("crashes ", st(theme::MUTED)),
                    span("━ ", st(theme::ORANGE)),
                    span("types ", st(theme::MUTED)),
                ],
            ),
            &[
                Series {
                    xs: &b.t,
                    ys: get("crashes"),
                    color: theme::RED,
                    fill: false,
                },
                Series {
                    xs: &b.t,
                    ys: get("crash types"),
                    color: theme::ORANGE,
                    fill: false,
                },
            ],
            range,
            true,
        );
        if !has_second {
            return;
        }
        let log_h = if rest2.h >= 18 {
            (rest2.h / 3).clamp(5, 12)
        } else {
            0
        };
        let (row2, log_r) = rest2.split_top(rest2.h - log_h);
        let c2 = row2.columns(&[1, 1], 0);
        self.chart_panel(
            buf,
            c2[0],
            (
                ic.bolt,
                &format!("Exec / sec · {}", window),
                vec![
                    span(
                        last_rate.map(|v| format!("{:.1}", v)).unwrap_or_default(),
                        st(theme::GREEN).bold(),
                    ),
                    span(" /s ", st(theme::MUTED)),
                ],
            ),
            &[Series {
                xs: &b.t,
                ys: &rate,
                color: theme::GREEN,
                fill: false,
            }],
            range,
            false,
        );
        self.chart_panel(
            buf,
            c2[1],
            (
                ic.file,
                &format!("Corpus · {}", window),
                vec![
                    span(
                        b.last("corpus").map(util::grouped).unwrap_or_default(),
                        st(theme::PURPLE).bold(),
                    ),
                    span(" progs ", st(theme::MUTED)),
                ],
            ),
            &[Series {
                xs: &b.t,
                ys: get("corpus"),
                color: theme::PURPLE,
                fill: true,
            }],
            range,
            false,
        );
        if log_h > 0 {
            self.draw_log(buf, log_r, c);
        }
    }

    fn draw_log(&self, buf: &mut Buffer, r: Rect, c: &Campaign) {
        let ic = icons();
        let inner = ui::panel(buf, r, ic.term, "fuzz.log", false);
        ui::panel_note(
            buf,
            r,
            &[span(
                format!(" {} lines tail ", c.tail.len().min(inner.h)),
                st(theme::MUTED),
            )],
        );
        let start = c.tail.len().saturating_sub(inner.h);
        for (i, line) in c.tail[start..].iter().enumerate() {
            let lower = line.to_lowercase();
            let color =
                if lower.contains("panic") || lower.contains("crash") || lower.contains("fatal") {
                    theme::RED
                } else if lower.contains("warn") || lower.contains("fail") {
                    theme::YELLOW
                } else {
                    theme::MUTED
                };
            let (ts, rest) =
                if line.len() > 20 && line.as_bytes()[4] == b'/' && line.is_char_boundary(19) {
                    (&line[..19], &line[19..])
                } else {
                    ("", line.as_str())
                };
            buf.spans(
                inner.x + 1,
                inner.y + i,
                &[span(ts, st(theme::SUBTLE)), span(rest, st(color))],
                inner.right(),
            );
        }
    }

    fn draw_no_bench(&self, buf: &mut Buffer, r: Rect, c: &Campaign) {
        let ic = icons();
        let inner = ui::panel(buf, r, ic.metrics, "Waiting for statistics", false);
        let lines: Vec<Vec<Span>> = if c.bench.path.is_file() {
            vec![
                vec![span(
                    format!(
                        "{} bench.log exists but holds no complete sample yet.",
                        ic.clock
                    ),
                    st(theme::YELLOW),
                )],
                vec![span(
                    "  syz-manager writes one sample per minute.",
                    st(theme::MUTED),
                )],
            ]
        } else {
            vec![
                vec![span(
                    format!("{} No bench.log in this workdir.", ic.warn),
                    st(theme::YELLOW),
                )],
                vec![
                    span("  Start syz-manager with ", st(theme::MUTED)),
                    span("-bench <workdir>/bench.log", st(theme::GREEN)),
                    span(" to record statistics", st(theme::MUTED)),
                ],
                vec![span(
                    "  (fuzzing launched from the Fuzzer Config tab always records them).",
                    st(theme::MUTED),
                )],
                vec![],
                vec![span(
                    format!(
                        "  {} crash(es) are still listed in the Issues sub-tab.",
                        c.issues.len()
                    ),
                    st(theme::MUTED),
                )],
            ]
        };
        for (i, l) in lines.iter().enumerate() {
            buf.spans(inner.x + 2, inner.y + 1 + i, l, inner.right());
        }
        if inner.h > lines.len() + 4 {
            let log_r = Rect::new(
                r.x + 1,
                r.y + lines.len() + 3,
                r.w - 2,
                r.h.saturating_sub(lines.len() + 4),
            );
            if log_r.h >= 4 {
                self.draw_log(buf, log_r, c);
            }
        }
    }

    fn draw_metrics(&mut self, buf: &mut Buffer, r: Rect) {
        let ic = icons();
        let names = self.metric_names();
        let Some(c) = &self.camp else { return };
        let b = &c.bench;
        if b.is_empty() {
            self.draw_no_bench(buf, r, c);
            return;
        }
        if names.is_empty() {
            let inner = ui::panel(buf, r, ic.metrics, "Metrics", false);
            buf.text(
                inner.x + 2,
                inner.y + 1,
                &format!(
                    "{} No metric matches \"{}\" (Esc clears the filter)",
                    ic.search, self.filter
                ),
                st(theme::MUTED),
                inner.right(),
            );
            return;
        }
        self.metric_sel = self.metric_sel.min(names.len() - 1);
        let range = self.range(b);
        let window = WINDOWS[self.window].0;
        if self.zoom {
            let name = &names[self.metric_sel];
            let ys = b.get(name).unwrap_or(&[]);
            let color = theme::SERIES[self.metric_sel % theme::SERIES.len()];
            let (lo, hi, d) = stats(&b.t, ys, range);
            let note = vec![
                span(
                    format!(" {}/{} ", self.metric_sel + 1, names.len()),
                    st(theme::MUTED),
                ),
                span("min ", st(theme::SUBTLE)),
                span(util::num(lo), st(theme::TEXT)),
                span("  max ", st(theme::SUBTLE)),
                span(util::num(hi), st(theme::TEXT)),
                span("  Δ ", st(theme::SUBTLE)),
                span(
                    signed(d),
                    st(if d > 0.0 { theme::GREEN } else { theme::TEXT }),
                ),
                span("  now ", st(theme::SUBTLE)),
                span(
                    b.last(name).map(util::grouped).unwrap_or_default(),
                    Style::new(color, theme::BG).bold(),
                ),
                span(" ", st(theme::MUTED)),
            ];
            let title = format!("{} · {}", name, window);
            self.chart_panel(
                buf,
                r,
                (ic.metrics, &title, note),
                &[Series {
                    xs: &b.t,
                    ys,
                    color,
                    fill: true,
                }],
                range,
                false,
            );
            return;
        }
        let cols = (r.w / MINI_W).clamp(1, 6);
        self.metric_cols = cols;
        // One row is reserved for the page indicator.
        let rows_vis = (r.h.saturating_sub(1) / MINI_H).max(1);
        let total_rows = names.len().div_ceil(cols);
        let sel_row = self.metric_sel / cols;
        if sel_row < self.metric_row {
            self.metric_row = sel_row;
        }
        if sel_row >= self.metric_row + rows_vis {
            self.metric_row = sel_row + 1 - rows_vis;
        }
        let cell_h =
            (r.h.saturating_sub(1) / rows_vis.min(total_rows).max(1)).clamp(MINI_H, MINI_H + 4);
        let col_rects = Rect::new(r.x, r.y, r.w, cell_h).columns(&vec![1; cols], 0);
        for (i, name) in names.iter().enumerate() {
            let row = i / cols;
            if row < self.metric_row || row >= self.metric_row + rows_vis {
                continue;
            }
            let cr = col_rects[i % cols];
            let y = r.y + (row - self.metric_row) * cell_h;
            if y + cell_h > r.bottom() {
                continue;
            }
            let cell = Rect::new(cr.x, y, cr.w, cell_h);
            let selected = i == self.metric_sel;
            let color = theme::SERIES[i % theme::SERIES.len()];
            let now = b
                .last(name)
                .map(util::num)
                .unwrap_or_else(|| "-".to_string());
            let title_w = cell.w.saturating_sub(str_width(&now) + 11);
            let inner = ui::panel(buf, cell, "", &truncate(name, title_w), selected);
            ui::panel_note(
                buf,
                cell,
                &[span(
                    format!(" {} ", now),
                    Style::new(color, theme::BG).bold(),
                )],
            );
            let ys = b.get(name).unwrap_or(&[]);
            let chart_r = Rect::new(inner.x, inner.y, inner.w, inner.h.saturating_sub(1));
            let opts = ChartOpts {
                x_range: range,
                y_zero: false,
                axes: false,
                x_fmt: util::elapsed,
            };
            chart::draw(
                buf,
                chart_r,
                &[Series {
                    xs: &b.t,
                    ys,
                    color,
                    fill: true,
                }],
                &opts,
            );
            let (lo, hi, d) = stats(&b.t, ys, range);
            let line = vec![
                span("min ", st(theme::SUBTLE)),
                span(util::num(lo), st(theme::MUTED)),
                span(" max ", st(theme::SUBTLE)),
                span(util::num(hi), st(theme::MUTED)),
                span(" Δ ", st(theme::SUBTLE)),
                span(
                    signed(d),
                    st(if d > 0.0 { theme::GREEN } else { theme::MUTED }),
                ),
            ];
            buf.spans(
                inner.x,
                inner.bottom().saturating_sub(1),
                &line,
                inner.right(),
            );
        }
        // Page indicator in the bottom-right corner.
        let info = format!(
            " {} metrics · rows {}-{} of {}{} ",
            names.len(),
            self.metric_row + 1,
            (self.metric_row + rows_vis).min(total_rows),
            total_rows,
            if self.filter.is_empty() {
                String::new()
            } else {
                format!(" · filter \"{}\"", self.filter)
            }
        );
        buf.text_right(
            r.right().saturating_sub(1),
            r.bottom().saturating_sub(1),
            &info,
            st(theme::MUTED),
        );
    }

    fn draw_issues(&mut self, buf: &mut Buffer, r: Rect) {
        let ic = icons();
        let Some(c) = &mut self.camp else { return };
        if c.issues.is_empty() {
            let inner = ui::panel(buf, r, ic.issues, "Issues", false);
            let msg = format!(
                "{} No crashes recorded in {} yet.",
                ic.ok,
                util::tilde(&c.dir.join("crashes"))
            );
            let mr = inner.centered(str_width(&msg).min(inner.w), 1);
            buf.text(mr.x, mr.y, &msg, st(theme::GREEN), inner.right());
            return;
        }
        self.issue_sel = self.issue_sel.min(c.issues.len() - 1);
        let table_h = ((r.h * 2) / 5)
            .max(6)
            .min(c.issues.len() + 4)
            .min(r.h.saturating_sub(8).max(6));
        let (top, bottom) = r.split_top(table_h);
        let inner = ui::panel(buf, top, ic.issues, "Crashes", true);
        let total_hits: usize = c.issues.iter().map(|i| i.logs).sum();
        ui::panel_note(
            buf,
            top,
            &[span(
                format!(" {} types · {} logs ", c.issues.len(), total_hits),
                st(theme::MUTED),
            )],
        );

        // Columns: # | title | logs | repro | first seen | last seen
        let (wn, wl, wr, wf, wa) = (4, 6, 8, 17, 11);
        let wt = inner.w.saturating_sub(wn + wl + wr + wf + wa + 6);
        let header = [
            pad_left("#", wn),
            pad("Title", wt),
            pad_left("Logs", wl),
            pad("Repro", wr),
            pad("First seen", wf),
            pad("Last seen", wa),
        ]
        .join(" ");
        buf.text(
            inner.x + 1,
            inner.y,
            &header,
            st(theme::MUTED).bold(),
            inner.right(),
        );
        buf.hline(inner.x, inner.y + 1, inner.w, '─', st(theme::GRID));
        let rows = inner.h.saturating_sub(2);
        if self.issue_sel < self.issue_scroll {
            self.issue_scroll = self.issue_sel;
        }
        if self.issue_sel >= self.issue_scroll + rows {
            self.issue_scroll = self.issue_sel + 1 - rows;
        }
        for (row, (i, is)) in c
            .issues
            .iter()
            .enumerate()
            .skip(self.issue_scroll)
            .take(rows)
            .enumerate()
        {
            let y = inner.y + 2 + row;
            let sel = i == self.issue_sel;
            let bg = if sel { theme::BG_SEL } else { theme::BG };
            buf.fill(
                Rect::new(inner.x, y, inner.w, 1),
                ' ',
                Style::new(theme::TEXT, bg),
            );
            if sel {
                buf.set(inner.x, y, '▌', Style::new(theme::CYAN, bg));
            }
            let (repro, rc) = if is.has_crepro {
                ("C", theme::GREEN)
            } else if is.has_repro {
                ("syz", theme::CYAN)
            } else if is.repro_attempts > 0 {
                ("failed", theme::YELLOW)
            } else {
                ("-", theme::SUBTLE)
            };
            let first = is
                .first
                .map(|t| util::datetime(campaign::unix_secs(t)))
                .unwrap_or_else(|| "-".into());
            let last = is
                .last
                .map(|t| util::ago(campaign::age_secs(t)))
                .unwrap_or_else(|| "-".into());
            let tc = title_color(&is.title);
            let spans = vec![
                span(
                    format!("{} ", pad_left(&(i + 1).to_string(), wn)),
                    Style::new(theme::SUBTLE, bg),
                ),
                span(
                    format!("{} ", pad(&is.title, wt)),
                    Style::new(if sel { theme::WHITE } else { tc }, bg),
                ),
                span(
                    format!("{} ", pad_left(&is.logs.to_string(), wl)),
                    Style::new(theme::YELLOW, bg),
                ),
                span(format!("{} ", pad(repro, wr)), Style::new(rc, bg).bold()),
                span(
                    format!("{} ", pad(&first, wf)),
                    Style::new(theme::MUTED, bg),
                ),
                span(pad(&last, wa), Style::new(theme::MUTED, bg)),
            ];
            buf.spans(inner.x + 1, y, &spans, inner.right());
        }

        // Details and report preview.
        let is = c.issues[self.issue_sel].clone();
        let inner = ui::panel(buf, bottom, ic.bug, &is.title, false);
        let lw = 11;
        let mx = inner.right();
        let mut y = inner.y;
        let dir = truncate_left(&util::tilde(&is.dir), inner.w.saturating_sub(lw + 3));
        ui::kv(
            buf,
            inner.x + 1,
            y,
            mx,
            "Directory",
            lw,
            &[span(dir, st(theme::CYAN))],
        );
        y += 1;
        let repro_txt = match (is.has_crepro, is.has_repro, is.repro_attempts) {
            (true, _, _) => vec![
                span("C reproducer", st(theme::GREEN).bold()),
                span(" repro.cprog + repro.prog", st(theme::MUTED)),
            ],
            (false, true, _) => vec![
                span("syz reproducer", st(theme::CYAN).bold()),
                span(" repro.prog", st(theme::MUTED)),
            ],
            (false, false, n) if n > 0 => {
                vec![span(format!("{} failed attempt(s)", n), st(theme::YELLOW))]
            }
            _ => vec![span("none", st(theme::MUTED))],
        };
        ui::kv(buf, inner.x + 1, y, mx, "Reproducer", lw, &repro_txt);
        y += 1;
        let seen = vec![
            span(format!("{} logs", is.logs), st(theme::YELLOW)),
            span("  first ", st(theme::SUBTLE)),
            span(
                is.first
                    .map(|t| util::datetime(campaign::unix_secs(t)))
                    .unwrap_or_else(|| "-".into()),
                theme::BASE,
            ),
            span("  last ", st(theme::SUBTLE)),
            span(
                is.last
                    .map(|t| util::datetime(campaign::unix_secs(t)))
                    .unwrap_or_else(|| "-".into()),
                theme::BASE,
            ),
        ];
        ui::kv(buf, inner.x + 1, y, mx, "Seen", lw, &seen);
        y += 1;
        ui::kv(
            buf,
            inner.x + 1,
            y,
            mx,
            "Files",
            lw,
            &[span(
                truncate(&is.files.join(" "), inner.w.saturating_sub(lw + 3)),
                st(theme::MUTED),
            )],
        );
        y += 1;
        buf.hline(inner.x, y, inner.w, '┄', st(theme::GRID));
        let report_file = ["report0", "log0"]
            .iter()
            .map(|f| is.dir.join(f))
            .find(|p| p.is_file());
        let label = report_file
            .as_ref()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_default();
        buf.text(
            inner.x + 2,
            y,
            &format!(" {} ", label),
            st(theme::MUTED),
            mx,
        );
        y += 1;
        if c.report.as_ref().is_none_or(|(id, _)| id != &is.id) {
            let text = report_file
                .and_then(|p| fs::read(p).ok())
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            let lines = text.lines().map(|l| l.replace('\t', "    ")).collect();
            c.report = Some((is.id.clone(), lines));
        }
        let lines = &c.report.as_ref().unwrap().1;
        let rows = inner.bottom().saturating_sub(y);
        self.report_scroll = self.report_scroll.min(lines.len().saturating_sub(rows));
        for (i, line) in lines.iter().skip(self.report_scroll).take(rows).enumerate() {
            let color =
                if line.contains("panic") || line.contains("BUG") || line.contains("WARNING") {
                    theme::RED
                } else if line.trim_start().starts_with("at ") || line.contains(".rs:") {
                    theme::BLUE
                } else {
                    theme::TEXT
                };
            buf.text(inner.x + 1, y + i, line, st(color), mx);
        }
    }
}

/// Min, max and change of a series within the x range.
fn stats(xs: &[f64], ys: &[f64], (x0, x1): (f64, f64)) -> (f64, f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (&x, &y) in xs.iter().zip(ys) {
        if x >= x0 && x <= x1 && y.is_finite() {
            lo = lo.min(y);
            hi = hi.max(y);
        }
    }
    let d = chart::value_at(xs, ys, x1) - chart::value_at(xs, ys, x0);
    (lo, hi, if d.is_finite() { d } else { 0.0 })
}

fn signed(v: f64) -> String {
    if v > 0.0 {
        format!("+{}", util::num(v))
    } else {
        util::num(v)
    }
}

fn title_color(title: &str) -> super::buffer::Rgb {
    let t = title.to_lowercase();
    if t.contains("panic") || t.contains("deadlock") || t.contains("overflow") {
        theme::RED
    } else if t.contains("lost connection")
        || t.contains("no output")
        || t.contains("timed out")
        || t.contains("suppressed")
    {
        theme::MUTED
    } else {
        theme::ORANGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_stats() {
        let xs = [0.0, 60.0, 120.0, 180.0];
        let ys = [10.0, 20.0, 40.0, 40.0];
        let (lo, hi, d) = stats(&xs, &ys, (60.0, 180.0));
        assert_eq!((lo, hi, d), (20.0, 40.0, 20.0));
    }

    #[test]
    fn signed_formatting() {
        assert_eq!(signed(1500.0), "+1.5K");
        assert_eq!(signed(0.0), "0");
    }
}
