pub mod elf;
pub mod eval;
pub mod json;
pub mod kcov;

use std::env;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII temporary directory that cleans itself up on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> io::Result<Self> {
        let base = env::temp_dir();
        Self::new_in(&base, prefix)
    }

    pub fn new_in(base: &Path, prefix: &str) -> io::Result<Self> {
        let pid = std::process::id();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        for attempt in 0..100u64 {
            let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("{}{}-{}-{}-{}", prefix, pid, nanos, seq, attempt);
            let candidate = base.join(name);
            match fs::create_dir(&candidate) {
                Ok(()) => return Ok(Self { path: candidate }),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "failed to create unique temporary directory",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Checks whether an executable exists in `PATH` or as an explicit path.
pub fn command_exists(cmd: &str) -> bool {
    if cmd.contains('/') {
        let p = Path::new(cmd);
        return is_executable(p);
    }
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let full = dir.join(cmd);
            if is_executable(&full) {
                return true;
            }
        }
    }
    false
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    } else {
        false
    }
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Requires that `cmd` exists in `PATH`, returning an error message matching the shell helper.
pub fn require_cmd(cmd: &str) -> Result<(), String> {
    if command_exists(cmd) {
        Ok(())
    } else {
        Err(format!("missing required command: {}", cmd))
    }
}

/// Equivalent to `realpath -m`: resolves existing ancestor links and normalizes `.` and `..`
/// even when the final path components do not exist yet.
pub fn realpath_m(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };

    if let Ok(canon) = fs::canonicalize(&abs) {
        return canon;
    }

    // Walk up until we find an existing ancestor that can be canonicalized
    let mut existing = abs.as_path();
    let mut tail = Vec::new();
    while !existing.exists() {
        if let Some(name) = existing.file_name() {
            tail.push(name.to_os_string());
        }
        match existing.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => existing = parent,
            _ => break,
        }
    }

    let mut result = fs::canonicalize(existing).unwrap_or_else(|_| normalize_lexical(existing));
    for part in tail.into_iter().rev() {
        if part == ".." {
            result.pop();
        } else if part != "." {
            result.push(part);
        }
    }
    normalize_lexical(&result)
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

/// Escapes a string for embedding inside a JSON string literal.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Locates the SyzAster repository root directory.
pub fn find_syzaster_root() -> PathBuf {
    if let Ok(dir) = env::var("SYZASTER_DIR") {
        let p = PathBuf::from(dir);
        if p.exists() {
            return realpath_m(&p);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            if let Some(root) = bin_dir.parent() {
                if root.join("Cargo.toml").exists() || root.join("syzkaller").exists() {
                    return realpath_m(root);
                }
                // Handle target/release or target/debug
                if let Some(workspace) = root.parent() {
                    if workspace.join("Cargo.toml").exists() {
                        return realpath_m(workspace);
                    }
                }
            }
        }
    }
    realpath_m(Path::new("."))
}

/// Runs a command and captures trimmed stdout if successful.
pub fn run_capture(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {}", program, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{} exited with {}: {}",
            program,
            output.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
