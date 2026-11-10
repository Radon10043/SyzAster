//! Kernel Config tab: fetch, patch and build Asterinas.

use super::buffer::{span, Buffer, Rect, Span};
use super::theme::{self, icons, st};
use super::ui::{self, Dialog, Input, InputKind, Item, ItemKind, Level, ListState};
use super::util::{self, Settings};
use super::{rel, status, Action, After, Confirmed, Field, Job};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Default)]
struct Status {
    src_exists: bool,
    is_git: bool,
    head: Option<String>,
    expected: Option<String>,
    patches: Vec<(String, Option<bool>)>,
    artifacts: Vec<(&'static str, Option<u64>)>,
    checker: bool,
}

pub struct KernelTab {
    root: PathBuf,
    versions: Vec<String>,
    version: usize,
    dir: PathBuf,
    patch: bool,
    kcov: bool,
    image_override: Option<PathBuf>,
    cfg_override: Option<PathBuf>,
    list: ListState,
    focus: usize,
    scroll: usize,
    status: Option<Status>,
}

fn default_dir(root: &Path) -> PathBuf {
    if let Ok(a) = std::env::var("ASTERINAS") {
        // $ASTERINAS follows the <dir>/asterinas/<version> layout.
        let a = Path::new(&a);
        if let Some(group) = a.parent().filter(|p| p.ends_with("asterinas")) {
            if let Some(dir) = group.parent() {
                return dir.to_path_buf();
            }
        }
    }
    for cand in ["/vol/kernel", "/vol"] {
        if Path::new(cand).is_dir() {
            return PathBuf::from(cand);
        }
    }
    root.parent().unwrap_or(root).to_path_buf()
}

/// Reads `key = "value"` from a flat TOML file.
pub fn toml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != key {
            return None;
        }
        let v = v.split('#').next()?.trim().trim_matches('"');
        Some(v.to_string())
    })
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

impl KernelTab {
    pub fn new(root: &Path, s: &Settings) -> Self {
        let mut versions: Vec<String> = fs::read_dir(root.join("patch").join("asterinas"))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        versions.sort();
        if versions.is_empty() {
            versions.push("v0.19.0".to_string());
        }
        let version = s
            .get("kernel.version")
            .and_then(|v| versions.iter().position(|x| x == v))
            .unwrap_or(versions.len() - 1);
        let path = |k: &str| s.get(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        KernelTab {
            root: root.to_path_buf(),
            versions,
            version,
            dir: path("kernel.dir").unwrap_or_else(|| default_dir(root)),
            patch: s.get("kernel.patch") != Some("false"),
            kcov: s.get("kernel.kcov") != Some("false"),
            image_override: path("kernel.image"),
            cfg_override: path("kernel.cfg"),
            list: ListState::default(),
            focus: 0,
            scroll: 0,
            status: None,
        }
    }

    pub fn version(&self) -> &str {
        &self.versions[self.version]
    }

    /// Kernel source tree: `<dir>/asterinas/<version>`.
    pub fn source_tree(&self) -> PathBuf {
        self.dir.join("asterinas").join(self.version())
    }

    pub fn image_dir(&self) -> PathBuf {
        if let Some(p) = &self.image_override {
            return p.clone();
        }
        let name = format!("asterinas-{}", self.version());
        if Path::new("/vol/image").is_dir() {
            PathBuf::from("/vol/image").join(name)
        } else {
            self.root.join(".workdir").join("image").join(name)
        }
    }

    pub fn build_cfg(&self) -> PathBuf {
        self.cfg_override.clone().unwrap_or_else(|| {
            self.root
                .join("configs")
                .join("kernel")
                .join(format!("asterinas-{}.toml", self.version()))
        })
    }

    pub fn refresh(&mut self) {
        self.status = None;
    }

    fn compute_status(&self) -> Status {
        let src = self.source_tree();
        let mut s = Status {
            src_exists: src.is_dir(),
            ..Default::default()
        };
        if let Ok(text) = fs::read_to_string(self.build_cfg()) {
            s.expected = toml_value(&text, "commit");
        }
        if s.src_exists {
            s.head = git(&src, &["rev-parse", "HEAD"]);
            s.is_git = s.head.is_some();
        }
        let pdir = self
            .root
            .join("patch")
            .join("asterinas")
            .join(self.version());
        let mut patches: Vec<PathBuf> = fs::read_dir(&pdir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "patch"))
                    .collect()
            })
            .unwrap_or_default();
        patches.sort();
        for p in patches {
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let applied = if s.is_git {
                let p = p.to_string_lossy().to_string();
                Some(git(&src, &["apply", "--reverse", "--check", &p]).is_some())
            } else {
                None
            };
            s.patches.push((name, applied));
        }
        let img = self.image_dir();
        for f in ["asterinas.img", "asterinas.id_rsa", "kernel/vmlinux"] {
            s.artifacts
                .push((f, fs::metadata(img.join(f)).ok().map(|m| m.len())));
        }
        s.checker = self.root.join("bin").join("check-kcov-inst").is_file();
        s
    }

    fn items(&self) -> Vec<Item> {
        let ic = icons();
        let src = self.source_tree();
        let exists = |p: &Path| -> Option<Span> {
            Some(if p.exists() {
                span(format!("{} ", ic.ok), st(theme::GREEN))
            } else {
                span(format!("{} missing", ic.fail), st(theme::RED))
            })
        };
        let ver = self.version().to_string();
        vec![
            Item::section("Source"),
            Item::new("version", ItemKind::Choice, "Kernel version")
                .icon(ic.branch)
                .value(&ver)
                .help(format!("Asterinas release to fetch. SyzAster ships KCOV patches for {} (patch/asterinas/<version>). Use ←/→ or Enter to cycle.", self.versions.join(", "))),
            Item::new("dir", ItemKind::Path, "Download directory")
                .icon(ic.folder)
                .value(util::tilde(&self.dir))
                .badge(exists(&self.dir).filter(|_| !self.dir.exists()))
                .help("Parent directory for kernel sources. The kernel is cloned into <dir>/asterinas/<version> (scripts/update-kernel.sh --dest). Press Enter to change it."),
            Item::new("src", ItemKind::Info, "Source tree")
                .icon(ic.folder_open)
                .value(util::tilde(&src))
                .badge(exists(&src))
                .help("Derived from the download directory and version; this is the $ASTERINAS tree used by every build step."),
            Item::new("patch", ItemKind::Toggle, "Apply SyzAster patches")
                .icon(ic.edit)
                .on(self.patch)
                .help(format!("Apply patch/asterinas/{}/*.patch (task-scoped /dev/kcov, /proc/kallsyms, NixOS fixes) after checkout. OFF passes --no-patch.", ver)),
            Item::section("Build"),
            Item::new("cfg", ItemKind::Path, "Build config")
                .icon(ic.file)
                .value(rel(&self.root, &self.build_cfg()))
                .badge(exists(&self.build_cfg()))
                .help("Kernel build configuration (TOML): pinned commit, Rust toolchain and KCOV build flags such as cargo_profile_*_lto = false. Enter changes the path, e opens it in vim."),
            Item::new("edit", ItemKind::Action, "Edit build config in vim")
                .icon(ic.edit)
                .value("e")
                .help("Open the kernel build configuration in vim. The preview on the right reloads when you return."),
            Item::new("image", ItemKind::Path, "Image output")
                .icon(ic.folder)
                .value(util::tilde(&self.image_dir()))
                .help("Output directory of create-image.sh: asterinas.img, asterinas.id_rsa and the unstripped kernel/vmlinux used for symbolization."),
            Item::new("kcov", ItemKind::Toggle, "KCOV instrumentation")
                .icon(ic.radar)
                .on(self.kcov)
                .help("ON builds with KCOV=1: only aster_core gets sancov trace-pc callbacks, OSTD stays uninstrumented. OFF passes --no-cover for an uninstrumented baseline image."),
            Item::section("Actions"),
            Item::new("download", ItemKind::Action, "Download & patch kernel")
                .icon(ic.play)
                .value("scripts/update-kernel.sh")
                .help("Clone Asterinas (if needed), check out the pinned release commit and apply the SyzAster patch series."),
            Item::new("verify", ItemKind::Action, "Verify KCOV instrumentation boundary")
                .icon(ic.search)
                .value("bin/check-kcov-inst")
                .help("Compile a probe crate through kcov-rustc and assert that aster_core references __sanitizer_cov_trace_pc while ostd does not."),
            Item::new("build", ItemKind::Action, "Build NixOS image")
                .icon(ic.cog)
                .value("scripts/asterinas/create-image.sh")
                .help("Build the kernel and the bootable NixOS disk image with SSH access, then stage kernel/vmlinux. This takes a while."),
            Item::new("refresh", ItemKind::Action, "Refresh status")
                .icon(ic.clock)
                .value("r")
                .help("Re-read git HEAD, patch state and image artifacts."),
        ]
    }

    pub fn set_field(&mut self, field: Field, value: &str, s: &mut Settings) -> Vec<Action> {
        let p = util::expand(value, &self.root);
        match field {
            Field::KernelDir => {
                self.dir = p.clone();
                s.set("kernel.dir", p.display().to_string());
            }
            Field::ImageDir => {
                s.set("kernel.image", p.display().to_string());
                self.image_override = Some(p.clone());
            }
            Field::KernelCfg => {
                s.set("kernel.cfg", p.display().to_string());
                self.cfg_override = Some(p.clone());
            }
            _ => return Vec::new(),
        }
        self.refresh();
        status(Level::Ok, format!("Set to {}", util::tilde(&p)))
    }

    fn cycle_version(&mut self, delta: i32, s: &mut Settings) {
        let n = self.versions.len() as i32;
        self.version = ((self.version as i32 + delta).rem_euclid(n)) as usize;
        s.set("kernel.version", self.version().to_string());
        self.cfg_override = None;
        self.image_override = None;
        s.set("kernel.cfg", "");
        s.set("kernel.image", "");
        self.refresh();
    }

    fn edit_cfg(&self) -> Vec<Action> {
        let cfg = self.build_cfg();
        let mut job = Job::new(
            "Edit kernel build config",
            super::editor(),
            vec![cfg.display().to_string()],
        );
        job.pause = false;
        job.after = After::Kernel;
        job.mkdir = cfg.parent().map(|p| p.to_path_buf());
        vec![Action::Run(job)]
    }

    fn activate(&mut self, id: &str, key: super::term::Key, s: &mut Settings) -> Vec<Action> {
        use super::term::Key;
        let root = self.root.clone();
        match id {
            "version" => {
                let d = if key == Key::Left { -1 } else { 1 };
                self.cycle_version(d, s);
                status(Level::Info, format!("Kernel version {}", self.version()))
            }
            "dir" => vec![Action::Input(
                Field::KernelDir,
                Input::new(
                    "Download directory",
                    "Parent directory for the Asterinas source tree:",
                    InputKind::Path,
                    &self.dir.display().to_string(),
                    &root,
                ),
            )],
            "image" => vec![Action::Input(
                Field::ImageDir,
                Input::new(
                    "Image output",
                    "Directory for asterinas.img, asterinas.id_rsa and kernel/vmlinux:",
                    InputKind::Path,
                    &self.image_dir().display().to_string(),
                    &root,
                ),
            )],
            "cfg" => vec![Action::Input(
                Field::KernelCfg,
                Input::new(
                    "Kernel build config",
                    "Path of the kernel build configuration (TOML):",
                    InputKind::Path,
                    &self.build_cfg().display().to_string(),
                    &root,
                ),
            )],
            "patch" => {
                self.patch = !self.patch;
                s.set("kernel.patch", self.patch.to_string());
                Vec::new()
            }
            "kcov" => {
                self.kcov = !self.kcov;
                s.set("kernel.kcov", self.kcov.to_string());
                Vec::new()
            }
            "edit" => self.edit_cfg(),
            "refresh" => {
                self.refresh();
                status(Level::Info, "Kernel status refreshed")
            }
            "download" => {
                let src = self.source_tree();
                let mut args = vec![
                    "--dest".to_string(),
                    src.display().to_string(),
                    "-v".to_string(),
                    self.version().to_string(),
                ];
                if !self.patch {
                    args.push("--no-patch".to_string());
                }
                let script = root.join("scripts").join("update-kernel.sh");
                let mut job = Job::new(
                    "Download & patch Asterinas",
                    script.display().to_string(),
                    args,
                );
                job.after = After::Kernel;
                job.mkdir = src.parent().map(|p| p.to_path_buf());
                job.inline = true;
                let mut lines = vec![
                    vec![
                        span("Fetch Asterinas ", theme::BASE),
                        span(self.version(), st(theme::YELLOW).bold()),
                        span(" into:", theme::BASE),
                    ],
                    vec![span(
                        format!("  {}", util::tilde(&src)),
                        st(theme::CYAN),
                    )],
                    vec![],
                    vec![span(format!("$ {}", job.display()), st(theme::GREEN))],
                ];
                if !self.dir.exists() {
                    lines.push(vec![]);
                    lines.push(vec![span(
                        format!("{} The download directory will be created.", icons().info),
                        st(theme::BLUE),
                    )]);
                }
                vec![Action::Dialog(Dialog::confirm(
                    "Download kernel",
                    lines,
                    "Run",
                    Confirmed::Run(job),
                ))]
            }
            "verify" => {
                let bin = root.join("bin").join("check-kcov-inst");
                if !bin.is_file() {
                    return status(
                        Level::Error,
                        "bin/check-kcov-inst not found: run `make` first",
                    );
                }
                let mut args = Vec::new();
                let src = self.source_tree();
                if src.join("tools").is_dir() {
                    args.push("--asterinas".to_string());
                    args.push(src.display().to_string());
                }
                let mut job = Job::new(
                    "Verify KCOV instrumentation",
                    bin.display().to_string(),
                    args,
                );
                job.cwd = Some(root);
                job.inline = true;
                vec![Action::Run(job)]
            }
            "build" => {
                let src = self.source_tree();
                if !src.is_dir() {
                    return status(
                        Level::Error,
                        format!(
                            "Source tree {} does not exist: download the kernel first",
                            util::tilde(&src)
                        ),
                    );
                }
                let mut args = vec![
                    "--asterinas".to_string(),
                    src.display().to_string(),
                    "--out".to_string(),
                    self.image_dir().display().to_string(),
                    "--version".to_string(),
                    self.version().trim_start_matches('v').to_string(),
                ];
                if !self.kcov {
                    args.push("--no-cover".to_string());
                }
                let script = root
                    .join("scripts")
                    .join("asterinas")
                    .join("create-image.sh");
                let mut job = Job::new(
                    "Build Asterinas NixOS image",
                    script.display().to_string(),
                    args,
                );
                job.after = After::Kernel;
                job.inline = true;
                let lines = vec![
                    vec![
                        span("Build the ", theme::BASE),
                        span(
                            if self.kcov { "KCOV=1" } else { "KCOV=0" },
                            st(theme::YELLOW).bold(),
                        ),
                        span(" kernel and NixOS image into:", theme::BASE),
                    ],
                    vec![span(
                        format!("  {}", util::tilde(&self.image_dir())),
                        st(theme::CYAN),
                    )],
                    vec![],
                    vec![span(format!("$ {}", job.display()), st(theme::GREEN))],
                ];
                vec![Action::Dialog(Dialog::confirm(
                    "Build image",
                    lines,
                    "Build",
                    Confirmed::Run(job),
                ))]
            }
            _ => Vec::new(),
        }
    }

    pub fn handle_key(&mut self, key: super::term::Key, s: &mut Settings) -> Vec<Action> {
        use super::term::Key;
        match key {
            Key::Tab | Key::BackTab => {
                self.focus = 1 - self.focus;
                return Vec::new();
            }
            Key::Char('r') => return self.activate("refresh", key, s),
            Key::Char('e') => return self.edit_cfg(),
            _ => {}
        }
        if self.focus == 1 {
            match key {
                Key::Up | Key::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                Key::Down | Key::Char('j') => self.scroll += 1,
                Key::PageUp => self.scroll = self.scroll.saturating_sub(10),
                Key::PageDown => self.scroll += 10,
                Key::Home | Key::Char('g') => self.scroll = 0,
                Key::Enter => return self.edit_cfg(),
                _ => {}
            }
            return Vec::new();
        }
        let items = self.items();
        self.list.fix(&items);
        if self.list.nav(&items, key) {
            return Vec::new();
        }
        let item = &items[self.list.sel];
        let id = item.id;
        match (item.kind, key) {
            (
                ItemKind::Choice,
                Key::Left
                | Key::Right
                | Key::Enter
                | Key::Char(' ')
                | Key::Char('h')
                | Key::Char('l'),
            ) => {
                let k = if key == Key::Char('h') {
                    Key::Left
                } else {
                    key
                };
                self.activate(id, k, s)
            }
            (ItemKind::Toggle, Key::Char(' ') | Key::Enter) => self.activate(id, key, s),
            (ItemKind::Path | ItemKind::Action, Key::Enter) => self.activate(id, key, s),
            (ItemKind::Action, Key::Char(' ')) => self.activate(id, key, s),
            _ => Vec::new(),
        }
    }

    pub fn draw(&mut self, buf: &mut Buffer, area: Rect) -> Vec<(&'static str, &'static str)> {
        let ic = icons();
        if self.status.is_none() {
            self.status = Some(self.compute_status());
        }
        let cols = if area.w < 110 {
            area.columns(&[3, 2], 0)
        } else {
            area.columns(&[11, 9], 0)
        };
        let (left, right) = (cols[0], cols[1]);
        let items = self.items();
        self.list.fix(&items);
        let inner = ui::panel(buf, left, ic.kernel, "Asterinas Kernel", self.focus == 0);
        let help_lines =
            super::buffer::wrap(&items[self.list.sel].help, inner.w.saturating_sub(4)).len();
        let help_h = (help_lines + 1).clamp(3, 6).min(inner.h / 3);
        let (list_r, help_r) = inner.split_top(inner.h - help_h);
        let label_w = 23.min(list_r.w * 2 / 5);
        ui::item_list(
            buf,
            list_r,
            &items,
            &mut self.list,
            self.focus == 0,
            label_w,
        );
        ui::help_box(buf, help_r, &items[self.list.sel].help);

        let st_h = (8 + self.status.as_ref().map_or(0, |s| s.patches.len())).min(right.h / 2);
        let (top, bottom) = right.split_top(st_h);
        let sinner = ui::panel(buf, top, ic.search, "Tree Status", false);
        self.draw_status(buf, sinner);

        let cfg = self.build_cfg();
        let title = format!("Build Config · {}", rel(&self.root, &cfg));
        let pinner = ui::panel(buf, bottom, ic.file, &title, self.focus == 1);
        match fs::read_to_string(&cfg) {
            Ok(text) => {
                let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                ui::source_view(buf, pinner, &lines, &mut self.scroll, true);
            }
            Err(e) => {
                buf.text(
                    pinner.x + 1,
                    pinner.y,
                    &format!("{} cannot read {}: {}", ic.fail, util::tilde(&cfg), e),
                    st(theme::RED),
                    pinner.right(),
                );
            }
        }
        if self.focus == 1 {
            vec![
                ("↑↓", "Scroll"),
                ("Enter", "Edit in vim"),
                ("Tab", "Focus list"),
                ("r", "Refresh"),
            ]
        } else {
            let kind = items[self.list.sel].kind;
            let mut h = vec![("↑↓", "Move")];
            match kind {
                ItemKind::Path => h.push(("Enter", "Edit path")),
                ItemKind::Toggle => h.push(("Space", "Toggle")),
                ItemKind::Choice => h.push(("←→", "Cycle")),
                ItemKind::Action => h.push(("Enter", "Run")),
                _ => {}
            }
            h.extend([
                ("e", "Edit config"),
                ("Tab", "Focus preview"),
                ("r", "Refresh"),
            ]);
            h
        }
    }

    fn draw_status(&self, buf: &mut Buffer, r: Rect) {
        let Some(s) = &self.status else { return };
        let max = r.right();
        let lw = 13;
        let x = r.x + 1;
        let mut y = r.y;
        let mut put = |buf: &mut Buffer, label: &str, val: Vec<Span>| {
            if y < r.bottom() {
                ui::kv(buf, x, y, max, label, lw, &val);
            }
            y += 1;
        };
        put(
            buf,
            "Source tree",
            vec![
                ui::mark(Some(s.src_exists)),
                span(
                    if s.src_exists {
                        "present"
                    } else {
                        "not downloaded"
                    },
                    theme::BASE,
                ),
            ],
        );
        let head = match (&s.head, &s.expected) {
            (Some(h), Some(e)) if h == e => vec![
                ui::mark(Some(true)),
                span(format!("{} ", &h[..h.len().min(12)]), st(theme::YELLOW)),
                span(format!("= {}", self.version()), st(theme::MUTED)),
            ],
            (Some(h), Some(_)) => vec![
                ui::mark(None),
                span(format!("{} ", &h[..h.len().min(12)]), st(theme::YELLOW)),
                span("differs from pinned commit", st(theme::YELLOW)),
            ],
            (Some(h), None) => vec![
                ui::mark(None),
                span(h[..h.len().min(12)].to_string(), st(theme::YELLOW)),
            ],
            (None, _) if s.src_exists => vec![
                ui::mark(Some(false)),
                span("not a git checkout", st(theme::RED)),
            ],
            (None, Some(e)) => vec![
                span("  pinned ", st(theme::MUTED)),
                span(e[..e.len().min(12)].to_string(), st(theme::YELLOW)),
            ],
            (None, None) => vec![span("  -", st(theme::MUTED))],
        };
        put(buf, "HEAD", head);
        for (name, applied) in &s.patches {
            let short = super::buffer::truncate(name, r.w.saturating_sub(lw + 17));
            let label = match applied {
                Some(true) => "applied",
                Some(false) => "not applied",
                None => "pending",
            };
            let c = match applied {
                Some(true) => theme::GREEN,
                Some(false) => theme::RED,
                None => theme::MUTED,
            };
            put(
                buf,
                "Patch",
                vec![
                    ui::mark(*applied),
                    span(format!("{:<11} ", label), st(c)),
                    span(short, st(theme::MUTED)),
                ],
            );
        }
        for (name, size) in &s.artifacts {
            let val = match size {
                Some(sz) => vec![
                    ui::mark(Some(true)),
                    span(format!("{:<17}", name), theme::BASE),
                    span(util::num(*sz as f64) + "B", st(theme::MUTED)),
                ],
                None => vec![
                    ui::mark(Some(false)),
                    span(format!("{:<17}", name), st(theme::MUTED)),
                    span("missing", st(theme::RED)),
                ],
            };
            put(buf, "Image", val);
        }
        put(
            buf,
            "KCOV checker",
            vec![
                ui::mark(Some(s.checker)),
                span(
                    if s.checker {
                        "bin/check-kcov-inst built"
                    } else {
                        "run make"
                    },
                    theme::BASE,
                ),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flat_toml_values() {
        let t = "[kernel]\nname = \"asterinas\"\ncommit = \"abc123\" # pinned\nkcov = 1\n";
        assert_eq!(toml_value(t, "commit"), Some("abc123".to_string()));
        assert_eq!(toml_value(t, "kcov"), Some("1".to_string()));
        assert_eq!(toml_value(t, "missing"), None);
    }
}
