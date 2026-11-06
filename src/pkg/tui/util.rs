//! Formatting helpers and persisted TUI settings.

use super::term;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Compact number: 999, 1.2K, 34.5K, 1.20M.
pub fn num(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a < 1000.0 {
        if a.fract().abs() > 1e-6 && a < 100.0 {
            format!("{}{:.1}", sign, a)
        } else {
            format!("{}{}", sign, a.round() as i64)
        }
    } else if a < 1e6 {
        format!("{}{:.1}K", sign, a / 1e3)
    } else if a < 1e9 {
        format!("{}{:.2}M", sign, a / 1e6)
    } else {
        format!("{}{:.2}G", sign, a / 1e9)
    }
}

/// Integer with thousands separators: 1,234,567.
pub fn grouped(v: f64) -> String {
    if !v.is_finite() {
        return "-".to_string();
    }
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{}", out)
    } else {
        out
    }
}

/// Short duration: 45s, 12m, 3h05m, 2d04h.
pub fn dur(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "-".to_string();
    }
    let s = secs.round() as u64;
    if s < 60 {
        format!("{}s", s)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d{:02}h", s / 86400, (s % 86400) / 3600)
    }
}

/// Long duration: 1h 02m 03s.
pub fn dur_long(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "-".to_string();
    }
    let s = secs.round() as u64;
    let (d, h, m, sec) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60, s % 60);
    if d > 0 {
        format!("{}d {:02}h {:02}m", d, h, m)
    } else if h > 0 {
        format!("{}h {:02}m {:02}s", h, m, sec)
    } else {
        format!("{}m {:02}s", m, sec)
    }
}

/// Elapsed campaign time for chart axes, with a resolution that suits the
/// visible span: `45m`, `25h05m`, `3d04h`.
pub fn elapsed(secs: f64, span: f64) -> String {
    if !secs.is_finite() {
        return "-".to_string();
    }
    let s = secs.max(0.0).round() as u64;
    if span >= 2.0 * 86400.0 {
        return dur(s as f64);
    }
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h == 0 {
        if span < 600.0 {
            format!("{}m{:02}s", m, s % 60)
        } else {
            format!("{}m", m)
        }
    } else {
        format!("{}h{:02}m", h, m)
    }
}

pub fn ago(secs: f64) -> String {
    format!("{} ago", dur(secs))
}

/// `YYYY-MM-DD HH:MM` in local time.
pub fn datetime(unix: i64) -> String {
    let (y, mo, d, h, mi, _) = term::local_time(unix);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", y, mo, d, h, mi)
}

/// `HH:MM:SS` in local time.
pub fn clock(unix: i64) -> String {
    let (_, _, _, h, mi, s) = term::local_time(unix);
    format!("{:02}:{:02}:{:02}", h, mi, s)
}

pub fn now_unix() -> i64 {
    super::campaign::unix_secs(std::time::SystemTime::now())
}

/// Replaces a leading `$HOME` with `~` for display.
pub fn tilde(p: &Path) -> String {
    let s = p.display().to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() && home != "/" {
            if let Some(rest) = s.strip_prefix(&home) {
                if rest.is_empty() || rest.starts_with('/') {
                    return format!("~{}", rest);
                }
            }
        }
    }
    s
}

/// Expands `~` and makes the path absolute (relative to `base`).
pub fn expand(input: &str, base: &Path) -> PathBuf {
    let s = input.trim();
    let p = if let Some(rest) = s.strip_prefix('~') {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(format!("{}{}", home, rest))
    } else {
        PathBuf::from(s)
    };
    if p.is_absolute() {
        p
    } else {
        base.join(p)
    }
}

/// Completes the last path component; returns the new text and candidates.
pub fn complete_path(input: &str, base: &Path) -> (String, Vec<String>) {
    let (dir_part, prefix) = match input.rfind('/') {
        Some(i) => (&input[..=i], &input[i + 1..]),
        None => ("", input),
    };
    let dir = if dir_part.is_empty() {
        base.to_path_buf()
    } else {
        expand(dir_part, base)
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return (input.to_string(), Vec::new());
    };
    let mut matches: Vec<(String, bool)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let is_dir = e.path().is_dir();
            Some((name, is_dir))
        })
        .collect();
    matches.sort();
    if matches.is_empty() {
        return (input.to_string(), Vec::new());
    }
    let first = &matches[0].0;
    let mut common = first.len();
    for (m, _) in &matches[1..] {
        common = common.min(
            first
                .char_indices()
                .zip(m.chars())
                .take_while(|((_, a), b)| a == b)
                .count(),
        );
    }
    let common_str: String = first.chars().take(common).collect();
    let mut out = format!("{}{}", dir_part, common_str);
    if matches.len() == 1 && matches[0].1 {
        out.push('/');
    }
    let names = matches
        .iter()
        .map(|(n, d)| if *d { format!("{}/", n) } else { n.clone() })
        .collect();
    (out, names)
}

/// Simple `key=value` settings file stored under `.workdir/`.
#[derive(Debug, Default, Clone)]
pub struct Settings {
    path: Option<PathBuf>,
    values: BTreeMap<String, String>,
    dirty: bool,
}

impl Settings {
    pub fn load(path: PathBuf) -> Self {
        let mut values = BTreeMap::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        Settings {
            path: Some(path),
            values,
            dirty: false,
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: &str, val: impl Into<String>) {
        let val = val.into();
        if self.values.get(key) != Some(&val) {
            self.values.insert(key.to_string(), val);
            self.dirty = true;
        }
    }

    /// Writes the file if any value changed since the last save.
    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        self.dirty = false;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut out = String::from("# SyzAster TUI settings (written by `syzaster tui`)\n");
        for (k, v) in &self.values {
            out.push_str(&format!("{}={}\n", k, v));
        }
        fs::write(path, out).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkg::TempDir;

    #[test]
    fn formats_numbers_and_durations() {
        assert_eq!(num(999.0), "999");
        assert_eq!(num(12345.0), "12.3K");
        assert_eq!(num(2_500_000.0), "2.50M");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(grouped(1234567.0), "1,234,567");
        assert_eq!(dur(45.0), "45s");
        assert_eq!(dur(3600.0 + 300.0), "1h05m");
        assert_eq!(dur(90000.0), "1d01h");
        assert_eq!(dur_long(3723.0), "1h 02m 03s");
        assert_eq!(elapsed(90300.0, 3600.0), "25h05m");
        assert_eq!(elapsed(2700.0, 3600.0), "45m");
        assert_eq!(elapsed(200000.0, 300000.0), "2d07h");
    }

    #[test]
    fn completes_paths() {
        let tmp = TempDir::new("syzaster-tui-complete-").unwrap();
        fs::create_dir_all(tmp.path().join("alpha")).unwrap();
        fs::create_dir_all(tmp.path().join("alpine")).unwrap();
        fs::create_dir_all(tmp.path().join("beta")).unwrap();
        let base = tmp.path();
        let (out, cands) = complete_path("al", base);
        assert_eq!(out, "alp");
        assert_eq!(cands.len(), 2);
        let (out, _) = complete_path("be", base);
        assert_eq!(out, "beta/");
    }

    #[test]
    fn settings_round_trip() {
        let tmp = TempDir::new("syzaster-tui-settings-").unwrap();
        let p = tmp.path().join("sub").join("tui.conf");
        let mut s = Settings::load(p.clone());
        s.save().unwrap();
        assert!(!p.exists(), "unchanged settings must not be written");
        s.set("kernel_version", "v0.19.0");
        s.save().unwrap();
        let s2 = Settings::load(p);
        assert_eq!(s2.get("kernel_version"), Some("v0.19.0"));
    }
}
