//! Fuzzing campaign data sources: `bench.log`, `crashes/`, `fuzz.log` and
//! the `syz-manager` processes visible in this PID namespace.

use crate::pkg::json::JsonValue;
use crate::pkg::tui::cfgedit::JsonDoc;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// syz-manager writes one bench sample per minute.
pub const BENCH_PERIOD: f64 = 60.0;

/// Time series parsed from the JSON objects syz-manager appends to `-bench`.
#[derive(Debug, Default, Clone)]
pub struct BenchLog {
    pub path: PathBuf,
    offset: u64,
    pending: String,
    /// X axis: campaign uptime in seconds.
    pub t: Vec<f64>,
    /// Metric name -> values aligned with `t` (NaN where absent).
    pub series: BTreeMap<String, Vec<f64>>,
    pub mtime: Option<SystemTime>,
}

/// Splits concatenated JSON objects; returns complete objects and the number
/// of bytes consumed.
pub fn split_objects(s: &str) -> (Vec<&str>, usize) {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    let mut start = None;
    let mut consumed = 0;
    for (i, &c) in b.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    if let Some(st) = start.take() {
                        out.push(&s[st..=i]);
                        consumed = i + 1;
                    }
                }
            }
            _ => {}
        }
    }
    if depth == 0 && !in_str {
        consumed = s.len();
    }
    (out, consumed)
}

impl BenchLog {
    pub fn new(path: PathBuf) -> Self {
        BenchLog {
            path,
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.t.len()
    }

    pub fn is_empty(&self) -> bool {
        self.t.is_empty()
    }

    /// Reads newly appended samples. Returns true if anything changed.
    pub fn refresh(&mut self) -> bool {
        let Ok(meta) = fs::metadata(&self.path) else {
            let had = !self.t.is_empty();
            if had {
                *self = BenchLog::new(self.path.clone());
            }
            return had;
        };
        if meta.len() < self.offset {
            *self = BenchLog::new(self.path.clone());
        }
        self.mtime = meta.modified().ok();
        if meta.len() == self.offset {
            return false;
        }
        let Ok(mut f) = fs::File::open(&self.path) else {
            return false;
        };
        if f.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }
        let mut bytes = Vec::new();
        if f.read_to_end(&mut bytes).is_err() {
            return false;
        }
        self.offset += bytes.len() as u64;
        self.pending.push_str(&String::from_utf8_lossy(&bytes));
        let text = std::mem::take(&mut self.pending);
        let (objs, used) = split_objects(&text);
        for obj in objs {
            self.push_object(obj);
        }
        self.pending = text[used..].to_string();
        true
    }

    /// Parses a full bench file from memory (used by tests and tools).
    pub fn from_text(text: &str) -> Self {
        let mut b = BenchLog::default();
        for obj in split_objects(text).0 {
            b.push_object(obj);
        }
        b
    }

    fn push_object(&mut self, obj: &str) {
        let Ok(JsonValue::Object(map)) = JsonValue::parse(obj) else {
            return;
        };
        let idx = self.t.len();
        let uptime = map
            .get("uptime")
            .and_then(|v| match v {
                JsonValue::Number(n) => Some(*n),
                _ => None,
            })
            .unwrap_or((idx + 1) as f64 * BENCH_PERIOD);
        // A restarted manager appending to the same file resets uptime;
        // keep the axis monotonic by continuing from the last sample.
        let t = match self.t.last() {
            Some(&last) if uptime <= last => last + BENCH_PERIOD,
            _ => uptime,
        };
        self.t.push(t);
        for (k, v) in &map {
            if let JsonValue::Number(n) = v {
                let col = self
                    .series
                    .entry(k.clone())
                    .or_insert_with(|| vec![f64::NAN; idx]);
                col.push(*n);
            }
        }
        for col in self.series.values_mut() {
            if col.len() < idx + 1 {
                col.resize(idx + 1, f64::NAN);
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&[f64]> {
        self.series.get(name).map(|v| v.as_slice())
    }

    pub fn last(&self, name: &str) -> Option<f64> {
        self.get(name)?
            .iter()
            .rev()
            .find(|v| v.is_finite())
            .copied()
    }

    pub fn last_t(&self) -> f64 {
        self.t.last().copied().unwrap_or(0.0)
    }

    pub fn first_t(&self) -> f64 {
        self.t.first().copied().unwrap_or(0.0)
    }

    /// Per-second rate derived from a cumulative counter.
    pub fn rate(&self, name: &str) -> Vec<f64> {
        let Some(v) = self.get(name) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(v.len());
        for i in 0..v.len() {
            if i == 0 {
                out.push(f64::NAN);
                continue;
            }
            let dt = self.t[i] - self.t[i - 1];
            let dv = v[i] - v[i - 1];
            out.push(if dt > 0.0 && dv >= 0.0 {
                dv / dt
            } else {
                f64::NAN
            });
        }
        if out.len() > 1 {
            out[0] = out[1];
        }
        out
    }
}

/// One crash type stored by syz-manager under `crashes/<hash>/`.
#[derive(Debug, Clone)]
pub struct Issue {
    pub id: String,
    pub dir: PathBuf,
    pub title: String,
    pub logs: usize,
    pub has_repro: bool,
    pub has_crepro: bool,
    pub repro_attempts: usize,
    pub first: Option<SystemTime>,
    pub last: Option<SystemTime>,
    pub files: Vec<String>,
}

/// Reads all crash directories, newest first.
pub fn scan_issues(workdir: &Path) -> Vec<Issue> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(workdir.join("crashes")) else {
        return out;
    };
    for e in entries.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let desc_path = dir.join("description");
        let Ok(desc) = fs::read_to_string(&desc_path) else {
            continue;
        };
        let mut issue = Issue {
            id: e.file_name().to_string_lossy().to_string(),
            title: desc.trim().to_string(),
            logs: 0,
            has_repro: false,
            has_crepro: false,
            repro_attempts: 0,
            first: None,
            last: None,
            files: Vec::new(),
            dir: dir.clone(),
        };
        if let Ok(meta) = fs::metadata(&desc_path) {
            issue.last = meta.modified().ok();
            issue.first = meta.created().ok();
        }
        if let Ok(files) = fs::read_dir(&dir) {
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if let Some(idx) = name.strip_prefix("log") {
                    if idx.parse::<u32>().is_ok() {
                        issue.logs += 1;
                    }
                }
                if name == "repro.prog" {
                    issue.has_repro = true;
                } else if name == "repro.cprog" {
                    issue.has_crepro = true;
                } else if name
                    .strip_prefix("repro")
                    .is_some_and(|r| r.parse::<u32>().is_ok())
                {
                    issue.repro_attempts += 1;
                }
                if let Ok(m) = f.metadata().and_then(|m| m.modified()) {
                    if issue.first.is_none_or(|f0| m < f0) {
                        issue.first = Some(m);
                    }
                }
                issue.files.push(name);
            }
        }
        issue.files.sort();
        out.push(issue);
    }
    out.sort_by(|a, b| b.last.cmp(&a.last).then(a.title.cmp(&b.title)));
    out
}

/// Reads the last `max_bytes` of a file and returns its trailing lines.
pub fn tail_lines(path: &Path, max_bytes: u64, max_lines: usize) -> Vec<String> {
    let Ok(mut f) = fs::File::open(path) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = text
        .lines()
        .skip(if start > 0 { 1 } else { 0 })
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect();
    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
    }
    lines
}

/// A `syz-manager` process found in `/proc`.
#[derive(Debug, Clone)]
pub struct ManagerProc {
    pub pid: i32,
    pub config: Option<PathBuf>,
    pub bench: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        let a = a.trim_start_matches('-');
        if a == name {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            return Some(v.to_string());
        }
    }
    None
}

/// Parses a `/proc/<pid>/cmdline` blob; returns `None` if it is not syz-manager.
pub fn parse_manager_cmdline(
    cmdline: &[u8],
    cwd: &Path,
) -> Option<(Option<PathBuf>, Option<PathBuf>)> {
    let args: Vec<String> = cmdline
        .split(|&b| b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).to_string())
        .collect();
    let exe = args.first()?;
    if Path::new(exe).file_name()?.to_str()? != "syz-manager" {
        return None;
    }
    let resolve = |p: String| {
        let p = PathBuf::from(p);
        if p.is_absolute() {
            p
        } else {
            cwd.join(p)
        }
    };
    Some((
        flag_value(&args[1..], "config").map(resolve),
        flag_value(&args[1..], "bench").map(resolve),
    ))
}

/// Reads the `workdir` field from a syz-manager config.
pub fn config_workdir(cfg: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(cfg).ok()?;
    let doc = JsonDoc::parse(&text).ok()?;
    doc.get_str(&["workdir"]).map(PathBuf::from)
}

/// Lists syz-manager processes visible in this PID namespace.
pub fn scan_managers() -> Vec<ManagerProc> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return out;
    };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
            continue;
        };
        let Ok(cmdline) = fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let cwd = fs::read_link(e.path().join("cwd")).unwrap_or_else(|_| PathBuf::from("/"));
        let Some((config, bench)) = parse_manager_cmdline(&cmdline, &cwd) else {
            continue;
        };
        let workdir = config.as_deref().and_then(config_workdir);
        out.push(ManagerProc {
            pid,
            config,
            bench,
            workdir,
        });
    }
    out
}

fn same_path(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}

/// Finds the local syz-manager working on `workdir`.
pub fn find_manager<'a>(workdir: &Path, procs: &'a [ManagerProc]) -> Option<&'a ManagerProc> {
    procs.iter().find(|p| {
        p.workdir.as_deref().is_some_and(|w| same_path(w, workdir))
            || p.bench
                .as_deref()
                .and_then(|b| b.parent())
                .is_some_and(|d| same_path(d, workdir))
            || p.config
                .as_deref()
                .and_then(|c| c.parent())
                .is_some_and(|d| same_path(d, workdir))
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    /// syz-manager is visible in this container and can be signalled.
    Local(i32),
    /// Files are still being updated but no local process owns them:
    /// the campaign runs in another container or PID namespace.
    Remote,
    /// No process and no recent activity.
    Stopped,
}

/// Time of the most recent write to the campaign's live files.
pub fn last_activity(workdir: &Path) -> Option<SystemTime> {
    ["bench.log", "fuzz.log"]
        .iter()
        .filter_map(|f| {
            fs::metadata(workdir.join(f))
                .and_then(|m| m.modified())
                .ok()
        })
        .max()
}

/// Age of the most recent write to the campaign's live files.
pub fn activity_age(workdir: &Path) -> Option<Duration> {
    let m = last_activity(workdir)?;
    Some(
        SystemTime::now()
            .duration_since(m)
            .unwrap_or(Duration::ZERO),
    )
}

/// Classifies a workdir. `gone_at` is when a local manager for it was last
/// seen exiting: writes up to then come from that process, not a remote one.
pub fn run_state(workdir: &Path, procs: &[ManagerProc], gone_at: Option<SystemTime>) -> RunState {
    if let Some(p) = find_manager(workdir, procs) {
        return RunState::Local(p.pid);
    }
    let Some(last) = last_activity(workdir) else {
        return RunState::Stopped;
    };
    if gone_at.is_some_and(|g| last <= g + Duration::from_secs(5)) {
        return RunState::Stopped;
    }
    // fuzz.log is written every 10 s and bench.log every 60 s.
    let age = SystemTime::now()
        .duration_since(last)
        .unwrap_or(Duration::ZERO);
    if age < Duration::from_secs(150) {
        RunState::Remote
    } else {
        RunState::Stopped
    }
}

/// Whether a directory looks like a syz-manager workdir.
pub fn is_campaign_dir(dir: &Path) -> bool {
    dir.join("bench.log").is_file()
        || (dir.join("crashes").is_dir() && dir.join("corpus.db").is_file())
        || (dir.join("fuzz.log").is_file() && dir.join("fuzz.cfg").is_file())
}

/// Breadth-limited search for campaign directories below `roots`.
pub fn discover(roots: &[PathBuf], max_depth: usize, budget: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut visited = 0;
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = roots
        .iter()
        .filter(|r| r.is_dir())
        .map(|r| (r.clone(), 0))
        .collect();
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > budget {
            break;
        }
        if is_campaign_dir(&dir) {
            if !out.iter().any(|p: &PathBuf| same_path(p, &dir)) {
                out.push(dir);
            }
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut subs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                !matches!(
                    n.as_ref(),
                    ".git" | "target" | "node_modules" | "syzkaller" | "kernel" | "image"
                ) && !n.starts_with("asterinas")
            })
            .map(|e| e.path())
            .collect();
        subs.sort();
        queue.extend(subs.into_iter().map(|p| (p, depth + 1)));
    }
    out
}

pub fn unix_secs(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

pub fn age_secs(t: SystemTime) -> f64 {
    SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkg::TempDir;

    const BENCH: &str = "{\n  \"coverage\": 10,\n  \"exec total\": 100,\n  \"uptime\": 60\n}\n{\n  \"coverage\": 15,\n  \"crashes\": 1,\n  \"exec total\": 220,\n  \"uptime\": 120\n}\n{\n  \"cov";

    #[test]
    fn splits_concatenated_objects() {
        let (objs, used) = split_objects(BENCH);
        assert_eq!(objs.len(), 2);
        assert_eq!(&BENCH[used..], "\n{\n  \"cov");
    }

    #[test]
    fn parses_bench_series() {
        let b = BenchLog::from_text(BENCH);
        assert_eq!(b.t, vec![60.0, 120.0]);
        assert_eq!(b.last("coverage"), Some(15.0));
        let crashes = b.get("crashes").unwrap();
        assert!(crashes[0].is_nan());
        assert_eq!(crashes[1], 1.0);
        assert_eq!(b.rate("exec total"), vec![2.0, 2.0]);
    }

    #[test]
    fn incremental_refresh_reads_appended_samples() {
        let tmp = TempDir::new("syzaster-tui-bench-").unwrap();
        let p = tmp.path().join("bench.log");
        let (_, used) = split_objects(BENCH);
        fs::write(&p, &BENCH[..used]).unwrap();
        let mut b = BenchLog::new(p.clone());
        assert!(b.refresh());
        assert_eq!(b.len(), 2);
        let mut f = fs::OpenOptions::new().append(true).open(&p).unwrap();
        std::io::Write::write_all(&mut f, b"{\n  \"coverage\": 30,\n  \"uptime\": 180\n}\n")
            .unwrap();
        assert!(b.refresh());
        assert_eq!(b.len(), 3);
        assert_eq!(b.last("coverage"), Some(30.0));
        assert!(!b.refresh());
    }

    #[test]
    fn recognizes_manager_cmdline() {
        let cmd = b"/syzkaller/bin/syz-manager\0-config\0cfg/fuzz.cfg\0-bench=/w/bench.log\0";
        let (cfg, bench) = parse_manager_cmdline(cmd, Path::new("/root")).unwrap();
        assert_eq!(cfg, Some(PathBuf::from("/root/cfg/fuzz.cfg")));
        assert_eq!(bench, Some(PathBuf::from("/w/bench.log")));
        assert!(
            parse_manager_cmdline(b"timeout\0-s\0KILL\0syz-manager\0", Path::new("/")).is_none()
        );
    }

    #[test]
    fn classifies_run_state() {
        let tmp = TempDir::new("syzaster-tui-state-").unwrap();
        assert_eq!(run_state(tmp.path(), &[], None), RunState::Stopped);
        fs::write(tmp.path().join("fuzz.log"), "x").unwrap();
        assert_eq!(run_state(tmp.path(), &[], None), RunState::Remote);
        // The final writes of a manager we saw exit do not mean "remote".
        let gone = SystemTime::now();
        assert_eq!(run_state(tmp.path(), &[], Some(gone)), RunState::Stopped);
        let procs = [ManagerProc {
            pid: 42,
            config: Some(tmp.path().join("fuzz.cfg")),
            bench: None,
            workdir: None,
        }];
        assert_eq!(run_state(tmp.path(), &procs, None), RunState::Local(42));
    }

    #[test]
    fn scans_crash_directories() {
        let tmp = TempDir::new("syzaster-tui-crash-").unwrap();
        let c = tmp.path().join("crashes").join("abc");
        fs::create_dir_all(&c).unwrap();
        fs::write(c.join("description"), "Uncaught panic: boom\n").unwrap();
        fs::write(c.join("log0"), "x").unwrap();
        fs::write(c.join("log1"), "x").unwrap();
        fs::write(c.join("repro.prog"), "x").unwrap();
        fs::write(c.join("repro0"), "x").unwrap();
        let issues = scan_issues(tmp.path());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "Uncaught panic: boom");
        assert_eq!(issues[0].logs, 2);
        assert!(issues[0].has_repro && !issues[0].has_crepro);
        assert_eq!(issues[0].repro_attempts, 1);
    }
}
