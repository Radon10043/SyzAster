//! Fuzzer Config tab: syz-manager configuration, options and launch.

use super::buffer::{span, Buffer, Rect, Span, Style};
use super::cfgedit::{self, JsonDoc};
use super::kernel::KernelTab;
use super::theme::{self, icons, st};
use super::ui::{self, Dialog, Input, InputKind, Item, ItemKind, Level, ListState};
use super::util::{self, Settings};
use super::{rel, status, Action, After, Confirmed, Field, Job};
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::SystemTime;

const DECISION_MODES: [&str; 2] = ["bandit", "heuristic"];
const CRASH_RECOVERY: [&str; 2] = ["clean", "lineage"];
/// Executor processes per VM when the config does not set `procs`.
pub const DEFAULT_PROCS: u64 = 8;
/// syz-manager accepts procs in [1, prog.MaxPids].
const MAX_PROCS: u64 = 32;

pub struct FuzzerTab {
    root: PathBuf,
    syzkaller: PathBuf,
    cfg_path: PathBuf,
    templates: Vec<PathBuf>,
    profiles: Vec<PathBuf>,
    template: usize,
    profile: usize,
    procs: u64,
    doc: Option<Result<JsonDoc, String>>,
    lines: Vec<String>,
    mtime: Option<SystemTime>,
    list: ListState,
    focus: usize,
    scroll: usize,
}

/// One health check line: `None` = warning.
type Check = (Option<bool>, String);

impl FuzzerTab {
    pub fn new(root: &Path, s: &Settings) -> Self {
        let dir = root.join("configs").join("fuzz").join("asterinas");
        let mut templates = Vec::new();
        let mut profiles = Vec::new();
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "cfg"))
                    .collect()
            })
            .unwrap_or_default();
        // Full configs first, with the whole-kernel one as the default.
        files.sort_by_key(|p| {
            let n = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            (n != "kernel", n)
        });
        for f in files {
            let full = fs::read_to_string(&f)
                .map(|t| t.contains("\"target\""))
                .unwrap_or(false);
            if full {
                templates.push(f);
            } else {
                profiles.push(f);
            }
        }
        let path = |k: &str| s.get(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let idx = |k: &str, n: usize| {
            s.get(k)
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&i| i < n)
                .unwrap_or(0)
        };
        let mut tab = FuzzerTab {
            root: root.to_path_buf(),
            syzkaller: path("fuzzer.syzkaller").unwrap_or_else(|| root.join("syzkaller")),
            cfg_path: path("fuzzer.cfg").unwrap_or_else(|| root.join(".workdir").join("fuzz.cfg")),
            template: idx("fuzzer.template", templates.len().max(1)),
            profile: idx("fuzzer.profile", profiles.len() + 1),
            templates,
            profiles,
            procs: s
                .get("fuzzer.procs")
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| (1..=MAX_PROCS).contains(n))
                .unwrap_or(DEFAULT_PROCS),
            doc: None,
            lines: Vec::new(),
            mtime: None,
            list: ListState::default(),
            focus: 0,
            scroll: 0,
        };
        tab.reload();
        tab
    }

    /// Re-reads the manager config from disk.
    pub fn reload(&mut self) {
        self.mtime = fs::metadata(&self.cfg_path).and_then(|m| m.modified()).ok();
        match fs::read_to_string(&self.cfg_path) {
            Ok(text) => {
                self.lines = text.lines().map(|l| l.to_string()).collect();
                self.doc = Some(JsonDoc::parse(&text));
            }
            Err(_) => {
                self.lines.clear();
                self.doc = None;
            }
        }
    }

    /// Picks up edits made outside the TUI.
    pub fn tick(&mut self) {
        let m = fs::metadata(&self.cfg_path).and_then(|m| m.modified()).ok();
        if m != self.mtime {
            self.reload();
        }
    }

    fn doc(&self) -> Option<&JsonDoc> {
        self.doc.as_ref().and_then(|d| d.as_ref().ok())
    }

    fn get_bool(&self, path: &[&str]) -> Option<bool> {
        self.doc().and_then(|d| d.get_bool(path))
    }

    fn get_num(&self, path: &[&str]) -> Option<f64> {
        self.doc().and_then(|d| d.get_num(path))
    }

    fn get_str(&self, path: &[&str]) -> Option<String> {
        self.doc()
            .and_then(|d| d.get_str(path))
            .map(|s| s.to_string())
    }

    /// procs shown in the TUI: the config's value, else the TUI setting.
    fn procs_value(&self) -> u64 {
        self.get_num(&["procs"])
            .filter(|n| *n >= 1.0)
            .map(|n| n as u64)
            .unwrap_or(self.procs)
    }

    fn is_template(&self) -> bool {
        let cfgs = self.root.join("configs");
        let p = fs::canonicalize(&self.cfg_path).unwrap_or_else(|_| self.cfg_path.clone());
        p.starts_with(fs::canonicalize(&cfgs).unwrap_or(cfgs))
    }

    fn manager_bin(&self) -> PathBuf {
        self.syzkaller.join("bin").join("syz-manager")
    }

    fn profile_name(&self) -> String {
        match self.profile {
            0 => "none".to_string(),
            i => self.profiles[i - 1]
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        }
    }

    fn template_name(&self) -> String {
        self.templates
            .get(self.template)
            .map(|p| {
                p.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "-".to_string())
    }

    fn workdir(&self) -> Option<PathBuf> {
        self.get_str(&["workdir"])
            .filter(|w| !w.contains("__"))
            .map(PathBuf::from)
    }

    fn items(&self) -> Vec<Item> {
        let ic = icons();
        let has_doc = self.doc().is_some();
        let writable = has_doc && !self.is_template();
        let adaptive = self
            .get_bool(&["experimental", "adaptive_restart", "enabled"])
            .unwrap_or(false);
        let num = |path: &[&str]| {
            self.get_num(path)
                .map(util::grouped)
                .unwrap_or_else(|| "-".to_string())
        };
        let cfg_badge = if self.cfg_path.is_file() {
            match &self.doc {
                Some(Ok(_)) if self.is_template() => Some(span("template", st(theme::YELLOW))),
                Some(Ok(_)) => Some(span(format!("{} ", ic.ok), st(theme::GREEN))),
                _ => Some(span(format!("{} invalid", ic.fail), st(theme::RED))),
            }
        } else {
            Some(span(format!("{} missing", ic.fail), st(theme::RED)))
        };
        let built = self.manager_bin().is_file();
        let opt = "Writes the option into the manager config immediately (formatting and comments are preserved).";
        vec![
            Item::section("Paths"),
            Item::new("syzkaller", ItemKind::Path, "Syzkaller")
                .icon(ic.folder)
                .value(rel(&self.root, &self.syzkaller))
                .badge(Some(if built { span(format!("{} ", ic.ok), st(theme::GREEN)) } else { span("not built", st(theme::RED)) }))
                .help("Patched syzkaller checkout; bin/syz-manager is used to smoke test and launch fuzzing. Build it with `make syzkaller`."),
            Item::new("cfg", ItemKind::Path, "Manager config")
                .icon(ic.file)
                .value(rel(&self.root, &self.cfg_path))
                .badge(cfg_badge)
                .help("syz-manager configuration used by this tab. Templates under configs/ are read-only; generate a working copy under .workdir/ instead."),
            Item::new("edit", ItemKind::Action, "Edit config in vim")
                .icon(ic.edit)
                .value("e")
                .help("Open the manager config in vim; it is re-validated when you return."),
            Item::section("Template"),
            Item::new("template", ItemKind::Choice, "Base template")
                .icon(ic.file)
                .value(self.template_name())
                .help("Template from configs/fuzz/asterinas/: kernel is the instrumented whole-kernel setup, nocover the cover=false baseline."),
            Item::new("profile", ItemKind::Choice, "Subsystem profile")
                .icon(ic.radar)
                .value(self.profile_name())
                .help("Optional syscall profile merged on top of the template (enable_syscalls), like `jq -s add base.cfg fs.cfg`."),
            Item::new("generate", ItemKind::Action, "Generate config from template")
                .icon(ic.cog)
                .value("g")
                .help("Fill the template placeholders from the Kernel Config tab (__KERNEL__, __IMAGE__, __KERNEL_VERSION__) and this tab (__SYZKALLER__, __WORKDIR__ = .workdir) and write the manager config."),
            Item::section("Options"),
            Item::new("cover", ItemKind::Toggle, "Coverage tracing")
                .icon(ic.radar)
                .on(self.get_bool(&["cover"]).unwrap_or(false))
                .value("cover")
                .enabled(writable)
                .help(format!("Collect KCOV coverage from /dev/kcov to guide fuzzing (\"cover\"). Needs a KCOV=1 image. {}", opt)),
            Item::new("crash_snapshot", ItemKind::Toggle, "QEMU crash snapshot")
                .icon(ic.chip)
                .on(self.get_bool(&["experimental", "crash_snapshot"]).unwrap_or(false))
                .value("experimental.crash_snapshot")
                .enabled(writable)
                .help(format!("Snapshot the VM before running the executor and revert to it after crashes instead of rebooting (qemu/amd64 only, incompatible with \"snapshot\"). {}", opt)),
            Item::new("adaptive", ItemKind::Toggle, "Adaptive snapshot")
                .icon(ic.bolt)
                .on(adaptive)
                .value("adaptive_restart.enabled")
                .enabled(writable)
                .help(format!("Let the adaptive controller decide when to continue, checkpoint, restore a snapshot (soft reboot) or reboot the VM. Enabling it also enables the crash snapshot. {}", opt)),
            Item::new("decision", ItemKind::Choice, "Decision mode")
                .icon(ic.cog)
                .value(self.get_str(&["experimental", "adaptive_restart", "decision_mode"]).unwrap_or_else(|| "bandit".to_string()))
                .enabled(writable && adaptive)
                .help("bandit: cost-aware linear UCB learner (default). heuristic: fixed threshold baseline."),
            Item::new("recovery", ItemKind::Choice, "Crash recovery")
                .icon(ic.cog)
                .value(self.get_str(&["experimental", "adaptive_restart", "crash_recovery"]).unwrap_or_else(|| "clean".to_string()))
                .enabled(writable && adaptive)
                .help("Snapshot restored after a kernel crash. clean: always the initial snapshot. lineage: may restore an intermediate snapshot the crashed state does not descend from."),
            Item::new("reproduce", ItemKind::Toggle, "Crash reproduction")
                .icon(ic.bug)
                .on(self.get_bool(&["reproduce"]).unwrap_or(false))
                .value("reproduce")
                .enabled(writable)
                .help(format!("Let syz-manager try to extract reproducers (repro.prog / repro.cprog) for new crashes. {}", opt)),
            Item::new("vm_count", ItemKind::Number, "VM count").icon(ic.chip).value(num(&["vm", "count"])).enabled(writable).help("Number of QEMU VMs (vm.count). Press Enter to edit."),
            Item::new("procs", ItemKind::Number, "Procs per VM").icon(ic.cog).value(self.procs_value().to_string()).help(format!("Parallel executor processes inside each VM (procs, 1-{}, default {}). Press Enter to edit; the value is written to the manager config and used when generating one.", MAX_PROCS, DEFAULT_PROCS)),
            Item::new("vm_cpu", ItemKind::Number, "VM CPUs").icon(ic.chip).value(num(&["vm", "cpu"])).enabled(writable).help("Virtual CPUs per VM (vm.cpu)."),
            Item::new("vm_mem", ItemKind::Number, "VM memory (MB)").icon(ic.chip).value(num(&["vm", "mem"])).enabled(writable).help("Memory per VM in megabytes (vm.mem)."),
            Item::section("Run"),
            Item::new("validate", ItemKind::Action, "Validate config").icon(ic.search).value("v").help("Check JSON syntax, unfilled __XXX__ placeholders, referenced paths and option conflicts."),
            Item::new("smoke", ItemKind::Action, "Smoke test").icon(ic.term).value("syz-manager -mode=smoke-test").help("Boot the VM and check executor communication in the foreground before a long campaign."),
            Item::new("launch", ItemKind::Action, "Launch fuzzing").icon(ic.play).value("background").help("Start syz-manager in the background (output in <workdir>/fuzz.log) and switch to the Monitor tab."),
        ]
    }

    /// Validates the config; returns check lines.
    fn checks(&self) -> Vec<Check> {
        let mut out: Vec<Check> = Vec::new();
        match &self.doc {
            None => {
                out.push((
                    Some(false),
                    format!("{} does not exist", rel(&self.root, &self.cfg_path)),
                ));
                return out;
            }
            Some(Err(e)) => {
                out.push((Some(false), format!("JSON error at {}", e)));
                return out;
            }
            Some(Ok(_)) => out.push((Some(true), "valid JSON".to_string())),
        }
        let doc = self.doc().unwrap();
        let ph = cfgedit::placeholders(doc.text());
        if ph.is_empty() {
            out.push((Some(true), "no unfilled placeholders".to_string()));
        } else {
            out.push((Some(false), format!("placeholders: {}", ph.join(" "))));
        }
        if self.is_template() {
            out.push((None, "template under configs/ (read-only)".to_string()));
        }
        let path_check = |key: &[&str], label: &str, required: bool, out: &mut Vec<Check>| match doc
            .get_str(key)
        {
            Some(p) if p.contains("__") => {}
            Some(p) => {
                let ok = Path::new(p).exists();
                out.push((
                    if ok {
                        Some(true)
                    } else if required {
                        Some(false)
                    } else {
                        None
                    },
                    format!("{} {}", label, if ok { "found" } else { "missing" }),
                ));
            }
            None if required => out.push((Some(false), format!("{} not set", label))),
            None => {}
        };
        path_check(&["image"], "image", true, &mut out);
        path_check(&["sshkey"], "sshkey", true, &mut out);
        if doc.get_bool(&["cover"]).unwrap_or(false) {
            path_check(&["kernel_obj"], "kernel_obj", false, &mut out);
        }
        match doc.get_str(&["syzkaller"]) {
            Some(p) if !p.contains("__") => {
                let ok = Path::new(p).join("bin").join("syz-manager").is_file();
                out.push((
                    if ok { Some(true) } else { Some(false) },
                    format!("syzkaller {}", if ok { "built" } else { "not built" }),
                ));
            }
            _ => {}
        }
        if let Some(w) = self.workdir() {
            out.push((Some(true), format!("workdir {}", util::tilde(&w))));
        }
        let crash_snap = doc
            .get_bool(&["experimental", "crash_snapshot"])
            .unwrap_or(false);
        let adaptive = doc
            .get_bool(&["experimental", "adaptive_restart", "enabled"])
            .unwrap_or(false);
        if crash_snap && doc.get_bool(&["snapshot"]).unwrap_or(false) {
            out.push((
                Some(false),
                "crash_snapshot conflicts with snapshot".to_string(),
            ));
        }
        if adaptive && !crash_snap {
            out.push((
                Some(false),
                "adaptive_restart needs crash_snapshot".to_string(),
            ));
        }
        if crash_snap && doc.get_str(&["type"]) != Some("qemu") {
            out.push((Some(false), "crash_snapshot needs type qemu".to_string()));
        }
        out
    }

    fn write(&mut self, edits: &[(&[&str], String)]) -> Result<(), String> {
        if self.is_template() {
            return Err(
                "templates under configs/ are read-only: generate a working config first"
                    .to_string(),
            );
        }
        let Some(Ok(doc)) = &self.doc else {
            return Err("manager config is missing or invalid".to_string());
        };
        let mut doc = doc.clone();
        for (path, lit) in edits {
            doc.set(path, lit)?;
        }
        fs::write(&self.cfg_path, doc.text())
            .map_err(|e| format!("cannot write {}: {}", self.cfg_path.display(), e))?;
        self.reload();
        Ok(())
    }

    fn toggle(&mut self, id: &str) -> Vec<Action> {
        let flip = |cur: Option<bool>| (!cur.unwrap_or(false)).to_string();
        let crash = self
            .get_bool(&["experimental", "crash_snapshot"])
            .unwrap_or(false);
        let adaptive = self
            .get_bool(&["experimental", "adaptive_restart", "enabled"])
            .unwrap_or(false);
        let snapshot = self.get_bool(&["snapshot"]).unwrap_or(false);
        let mut edits: Vec<(&[&str], String)> = Vec::new();
        let mut note = String::new();
        match id {
            "cover" => edits.push((&["cover"], flip(self.get_bool(&["cover"])))),
            "reproduce" => edits.push((&["reproduce"], flip(self.get_bool(&["reproduce"])))),
            "crash_snapshot" => {
                edits.push((&["experimental", "crash_snapshot"], (!crash).to_string()));
                if !crash && snapshot {
                    edits.push((&["snapshot"], "false".to_string()));
                    note = " (disabled conflicting \"snapshot\")".to_string();
                }
                if crash && adaptive {
                    edits.push((
                        &["experimental", "adaptive_restart", "enabled"],
                        "false".to_string(),
                    ));
                    note = " (adaptive snapshot disabled too)".to_string();
                }
            }
            "adaptive" => {
                edits.push((
                    &["experimental", "adaptive_restart", "enabled"],
                    (!adaptive).to_string(),
                ));
                if !adaptive && !crash {
                    edits.push((&["experimental", "crash_snapshot"], "true".to_string()));
                    note = " (crash snapshot enabled too)".to_string();
                }
                if !adaptive && snapshot {
                    edits.push((&["snapshot"], "false".to_string()));
                }
            }
            _ => return Vec::new(),
        }
        let label = edits[0].0.join(".");
        let val = edits[0].1.clone();
        match self.write(&edits) {
            Ok(()) => status(Level::Ok, format!("{} = {}{}", label, val, note)),
            Err(e) => status(Level::Error, e),
        }
    }

    fn cycle(&mut self, id: &str, delta: i32, s: &mut Settings) -> Vec<Action> {
        let step = |i: usize, n: usize| ((i as i32 + delta).rem_euclid(n.max(1) as i32)) as usize;
        match id {
            "template" => {
                self.template = step(self.template, self.templates.len());
                s.set("fuzzer.template", self.template.to_string());
                Vec::new()
            }
            "profile" => {
                self.profile = step(self.profile, self.profiles.len() + 1);
                s.set("fuzzer.profile", self.profile.to_string());
                Vec::new()
            }
            "decision" | "recovery" => {
                if !self
                    .get_bool(&["experimental", "adaptive_restart", "enabled"])
                    .unwrap_or(false)
                {
                    return status(Level::Warn, "Enable the adaptive snapshot first");
                }
                let (key, opts): (&str, &[&str]) = if id == "decision" {
                    ("decision_mode", &DECISION_MODES)
                } else {
                    ("crash_recovery", &CRASH_RECOVERY)
                };
                let path = ["experimental", "adaptive_restart", key];
                let cur = self.get_str(&path).unwrap_or_else(|| opts[0].to_string());
                let i = opts.iter().position(|o| *o == cur).unwrap_or(0);
                let next = opts[step(i, opts.len())];
                match self.write(&[(&path, cfgedit::quote(next))]) {
                    Ok(()) => status(Level::Ok, format!("adaptive_restart.{} = {}", key, next)),
                    Err(e) => status(Level::Error, e),
                }
            }
            _ => Vec::new(),
        }
    }

    pub fn set_field(&mut self, field: Field, value: &str, s: &mut Settings) -> Vec<Action> {
        let (path, label): (&[&str], &str) = match field {
            Field::SyzkallerDir => {
                self.syzkaller = util::expand(value, &self.root);
                s.set("fuzzer.syzkaller", self.syzkaller.display().to_string());
                return status(
                    Level::Ok,
                    format!("Syzkaller set to {}", util::tilde(&self.syzkaller)),
                );
            }
            Field::ManagerCfg => {
                self.cfg_path = util::expand(value, &self.root);
                s.set("fuzzer.cfg", self.cfg_path.display().to_string());
                self.reload();
                return status(
                    Level::Ok,
                    format!("Manager config set to {}", util::tilde(&self.cfg_path)),
                );
            }
            Field::VmCount => (&["vm", "count"], "vm.count"),
            Field::Procs => {
                let n = match value.trim().parse::<u64>() {
                    Ok(n) if (1..=MAX_PROCS).contains(&n) => n,
                    _ => {
                        return status(Level::Error, format!("procs must be in [1, {}]", MAX_PROCS))
                    }
                };
                self.procs = n;
                s.set("fuzzer.procs", n.to_string());
                if self.doc().is_none() || self.is_template() {
                    return status(
                        Level::Ok,
                        format!("procs = {} (applied when a config is generated)", n),
                    );
                }
                return match self.write(&[(&["procs"], n.to_string())]) {
                    Ok(()) => status(Level::Ok, format!("procs = {}", n)),
                    Err(e) => status(Level::Error, e),
                };
            }
            Field::VmCpu => (&["vm", "cpu"], "vm.cpu"),
            Field::VmMem => (&["vm", "mem"], "vm.mem"),
            _ => return Vec::new(),
        };
        let Ok(n) = value.trim().parse::<u64>() else {
            return status(Level::Error, "expected a number");
        };
        if n == 0 {
            return status(Level::Error, format!("{} must be positive", label));
        }
        match self.write(&[(path, n.to_string())]) {
            Ok(()) => status(Level::Ok, format!("{} = {}", label, n)),
            Err(e) => status(Level::Error, e),
        }
    }

    /// Fills the selected template and writes the manager config.
    pub fn generate(&mut self, k: &KernelTab) -> Vec<Action> {
        let Some(tpl) = self.templates.get(self.template) else {
            return status(
                Level::Error,
                "no templates found under configs/fuzz/asterinas",
            );
        };
        let text = match fs::read_to_string(tpl) {
            Ok(t) => t,
            Err(e) => return status(Level::Error, format!("cannot read template: {}", e)),
        };
        let workdir = self.root.join(".workdir");
        let image = k.image_dir();
        let values = [
            ("KERNEL_VERSION", k.version().to_string()),
            ("WORKDIR", workdir.display().to_string()),
            ("IMAGE", image.display().to_string()),
            ("SYZKALLER", self.syzkaller.display().to_string()),
            ("KERNEL", k.source_tree().display().to_string()),
            ("ASTERINAS", k.source_tree().display().to_string()),
            ("SYZASTER", self.root.display().to_string()),
            ("VMCOUNT", "1".to_string()),
        ];
        let filled = cfgedit::fill_placeholders(&text, &values);
        let mut doc = match JsonDoc::parse(&filled) {
            Ok(d) => d,
            Err(e) => return status(Level::Error, format!("template is not valid JSON: {}", e)),
        };
        if let Err(e) = doc.set(&["procs"], &self.procs.to_string()) {
            return status(Level::Error, format!("cannot set procs: {}", e));
        }
        if self.profile > 0 {
            let prof = &self.profiles[self.profile - 1];
            let ptext = fs::read_to_string(prof).unwrap_or_default();
            match JsonDoc::parse(&ptext) {
                Ok(pdoc) => {
                    if let cfgedit::Kind::Object(members) = &pdoc.root().kind {
                        for m in members {
                            let raw = pdoc.raw(&m.value).to_string();
                            if let Err(e) = doc.set(&[m.key.as_str()], &raw) {
                                return status(
                                    Level::Error,
                                    format!("cannot merge profile: {}", e),
                                );
                            }
                        }
                    }
                }
                Err(e) => return status(Level::Error, format!("profile is not valid JSON: {}", e)),
            }
            // Keep the instance name unique per subsystem.
            if let Some(name) = doc.get_str(&["name"]).map(|s| s.to_string()) {
                let name = format!("{}-{}", name, self.profile_name());
                let _ = doc.set(&["name"], &cfgedit::quote(&name));
            }
        }
        if let Some(dir) = self.cfg_path.parent() {
            if let Err(e) = fs::create_dir_all(dir) {
                return status(
                    Level::Error,
                    format!("cannot create {}: {}", dir.display(), e),
                );
            }
        }
        if let Err(e) = fs::write(&self.cfg_path, doc.text()) {
            return status(
                Level::Error,
                format!("cannot write {}: {}", self.cfg_path.display(), e),
            );
        }
        self.reload();
        status(
            Level::Ok,
            format!(
                "Generated {} from {}{}",
                rel(&self.root, &self.cfg_path),
                self.template_name(),
                if self.profile > 0 {
                    format!(" + {}", self.profile_name())
                } else {
                    String::new()
                }
            ),
        )
    }

    fn launch_plan(&self) -> Result<(PathBuf, Vec<String>), String> {
        if self.doc().is_none() {
            return Err("manager config is missing or invalid".to_string());
        }
        let ph = cfgedit::placeholders(self.doc().unwrap().text());
        if !ph.is_empty() {
            return Err(format!("config still has placeholders: {}", ph.join(" ")));
        }
        let Some(workdir) = self.workdir() else {
            return Err("config has no workdir".to_string());
        };
        if !self.manager_bin().is_file() {
            return Err(format!(
                "{} not found: run `make syzkaller`",
                util::tilde(&self.manager_bin())
            ));
        }
        let mut args = vec!["-config".to_string(), self.cfg_path.display().to_string()];
        // Bench statistics are always recorded: the Monitor tab charts them.
        args.push("-bench".to_string());
        args.push(workdir.join("bench.log").display().to_string());
        Ok((workdir, args))
    }

    /// Spawns syz-manager detached from the TUI's process group.
    pub fn launch(&self) -> Result<(Child, PathBuf), String> {
        let (workdir, args) = self.launch_plan()?;
        fs::create_dir_all(&workdir)
            .map_err(|e| format!("cannot create {}: {}", workdir.display(), e))?;
        let (y, mo, d, h, mi, s) = super::term::local_time(util::now_unix());
        let stamp = format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, mo, d, h, mi, s);
        for name in ["bench", "fuzz"] {
            let p = workdir.join(format!("{}.log", name));
            if p.exists() {
                let to = workdir.join(format!("{}-{}.log", name, stamp));
                fs::rename(&p, &to)
                    .map_err(|e| format!("cannot archive {}: {}", p.display(), e))?;
            }
        }
        let log = fs::File::create(workdir.join("fuzz.log"))
            .map_err(|e| format!("cannot create fuzz.log: {}", e))?;
        let log2 = log.try_clone().map_err(|e| e.to_string())?;
        let child = Command::new(self.manager_bin())
            .args(&args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log2)
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start syz-manager: {}", e))?;
        Ok((child, workdir))
    }

    fn edit(&self) -> Vec<Action> {
        let mut job = Job::new(
            "Edit manager config",
            super::editor(),
            vec![self.cfg_path.display().to_string()],
        );
        job.pause = false;
        job.after = After::Fuzzer;
        job.mkdir = self.cfg_path.parent().map(|p| p.to_path_buf());
        vec![Action::Run(job)]
    }

    fn activate(&mut self, id: &str, k: &KernelTab, s: &mut Settings) -> Vec<Action> {
        let ic = icons();
        let root = self.root.clone();
        let num_input = |field, title: &str, label: &str, cur: Option<f64>| {
            let v = cur.map(|n| (n as u64).to_string()).unwrap_or_default();
            vec![Action::Input(
                field,
                Input::new(title, label, InputKind::Number, &v, &root),
            )]
        };
        match id {
            "syzkaller" => vec![Action::Input(
                Field::SyzkallerDir,
                Input::new(
                    "Syzkaller directory",
                    "Patched syzkaller checkout (contains bin/syz-manager):",
                    InputKind::Path,
                    &self.syzkaller.display().to_string(),
                    &root,
                ),
            )],
            "cfg" => vec![Action::Input(
                Field::ManagerCfg,
                Input::new(
                    "Manager config",
                    "Path of the syz-manager configuration file:",
                    InputKind::Path,
                    &self.cfg_path.display().to_string(),
                    &root,
                ),
            )],
            "edit" => self.edit(),
            "generate" => {
                let lines = vec![
                    vec![
                        span("Template  ", st(theme::MUTED)),
                        span(self.template_name(), st(theme::YELLOW).bold()),
                        span("  profile  ", st(theme::MUTED)),
                        span(self.profile_name(), st(theme::YELLOW).bold()),
                    ],
                    vec![
                        span("Kernel    ", st(theme::MUTED)),
                        span(util::tilde(&k.source_tree()), st(theme::CYAN)),
                    ],
                    vec![
                        span("Image     ", st(theme::MUTED)),
                        span(util::tilde(&k.image_dir()), st(theme::CYAN)),
                    ],
                    vec![
                        span("Output    ", st(theme::MUTED)),
                        span(util::tilde(&self.cfg_path), st(theme::CYAN)),
                    ],
                    vec![],
                    if self.cfg_path.exists() {
                        vec![span(
                            format!("{} The existing config will be overwritten.", ic.warn),
                            st(theme::YELLOW),
                        )]
                    } else {
                        vec![span(
                            format!("{} A new config file will be created.", ic.info),
                            st(theme::BLUE),
                        )]
                    },
                ];
                vec![Action::Dialog(Dialog::confirm(
                    "Generate manager config",
                    lines,
                    "Generate",
                    Confirmed::Generate,
                ))]
            }
            "cover" | "crash_snapshot" | "adaptive" | "reproduce" => self.toggle(id),
            "template" | "profile" | "decision" | "recovery" => self.cycle(id, 1, s),
            "vm_count" => num_input(
                Field::VmCount,
                "VM count",
                "Number of QEMU VMs (vm.count):",
                self.get_num(&["vm", "count"]),
            ),
            "procs" => num_input(
                Field::Procs,
                "Procs per VM",
                "Parallel executor processes per VM (procs):",
                Some(self.procs_value() as f64),
            ),
            "vm_cpu" => num_input(
                Field::VmCpu,
                "VM CPUs",
                "Virtual CPUs per VM (vm.cpu):",
                self.get_num(&["vm", "cpu"]),
            ),
            "vm_mem" => num_input(
                Field::VmMem,
                "VM memory",
                "Memory per VM in MB (vm.mem):",
                self.get_num(&["vm", "mem"]),
            ),
            "validate" => {
                let checks = self.checks();
                let bad = checks.iter().filter(|c| c.0 == Some(false)).count();
                let lines: Vec<Vec<Span>> = checks
                    .iter()
                    .map(|(ok, msg)| vec![ui::mark(*ok), span(msg.clone(), theme::BASE)])
                    .collect();
                let lvl = if bad > 0 { Level::Error } else { Level::Ok };
                let title = if bad > 0 {
                    format!("{} problem(s) found", bad)
                } else {
                    "Config looks good".to_string()
                };
                vec![Action::Dialog(Dialog::message(title, lvl, lines))]
            }
            "smoke" => match self.launch_plan() {
                Ok(_) => {
                    let mut job = Job::new(
                        "syz-manager smoke test",
                        self.manager_bin().display().to_string(),
                        vec![
                            "-config".to_string(),
                            self.cfg_path.display().to_string(),
                            "-mode=smoke-test".to_string(),
                        ],
                    );
                    job.cwd = Some(root);
                    vec![Action::Run(job)]
                }
                Err(e) => status(Level::Error, e),
            },
            "launch" => match self.launch_plan() {
                Ok((workdir, args)) => {
                    let cmd = format!(
                        "$ {} {}",
                        rel(&self.root, &self.manager_bin()),
                        args.join(" ")
                    );
                    let mut lines = vec![
                        vec![span("Start a background fuzzing campaign:", theme::BASE)],
                        vec![span(cmd, st(theme::GREEN))],
                        vec![span(
                            format!("  > {}", util::tilde(&workdir.join("fuzz.log"))),
                            st(theme::MUTED),
                        )],
                    ];
                    if workdir.join("bench.log").exists() || workdir.join("fuzz.log").exists() {
                        lines.push(vec![]);
                        lines.push(vec![span(
                            format!("{} Existing bench.log / fuzz.log will be archived with a timestamp suffix.", ic.info),
                            st(theme::BLUE),
                        )]);
                    }
                    vec![Action::Dialog(
                        Dialog::confirm("Launch fuzzing", lines, "Launch", Confirmed::Launch)
                            .level(Level::Info),
                    )]
                }
                Err(e) => status(Level::Error, e),
            },
            _ => Vec::new(),
        }
    }

    pub fn handle_key(
        &mut self,
        key: super::term::Key,
        k: &KernelTab,
        s: &mut Settings,
    ) -> Vec<Action> {
        use super::term::Key;
        match key {
            Key::Tab | Key::BackTab => {
                self.focus = 1 - self.focus;
                return Vec::new();
            }
            Key::Char('e') => return self.edit(),
            Key::Char('v') => return self.activate("validate", k, s),
            Key::Char('g') if self.focus == 0 => return self.activate("generate", k, s),
            Key::Char('r') => {
                self.reload();
                return status(Level::Info, "Manager config reloaded");
            }
            _ => {}
        }
        if self.focus == 1 {
            match key {
                Key::Up | Key::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                Key::Down | Key::Char('j') => self.scroll += 1,
                Key::PageUp => self.scroll = self.scroll.saturating_sub(10),
                Key::PageDown => self.scroll += 10,
                Key::Home | Key::Char('g') => self.scroll = 0,
                Key::End | Key::Char('G') => self.scroll = usize::MAX / 2,
                Key::Enter => return self.edit(),
                _ => {}
            }
            return Vec::new();
        }
        let items = self.items();
        self.list.fix(&items);
        if self.list.nav(&items, key) {
            return Vec::new();
        }
        let item = items[self.list.sel].clone();
        if !item.enabled && matches!(key, Key::Enter | Key::Char(' ') | Key::Left | Key::Right) {
            let why = if self.doc().is_none() {
                "Generate or open a valid manager config first"
            } else if self.is_template() {
                "Templates under configs/ are read-only: generate a working config first"
            } else {
                "Enable the adaptive snapshot first"
            };
            return status(Level::Warn, why);
        }
        match (item.kind, key) {
            (ItemKind::Choice, Key::Left | Key::Char('h')) => self.cycle(item.id, -1, s),
            (ItemKind::Choice, Key::Right | Key::Char('l') | Key::Enter | Key::Char(' ')) => {
                self.cycle(item.id, 1, s)
            }
            (ItemKind::Toggle, Key::Char(' ') | Key::Enter) => self.activate(item.id, k, s),
            (ItemKind::Path | ItemKind::Number | ItemKind::Action, Key::Enter) => {
                self.activate(item.id, k, s)
            }
            (ItemKind::Action, Key::Char(' ')) => self.activate(item.id, k, s),
            _ => Vec::new(),
        }
    }

    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        _k: &KernelTab,
    ) -> Vec<(&'static str, &'static str)> {
        let ic = icons();
        let cols = if area.w < 110 {
            area.columns(&[3, 2], 0)
        } else {
            area.columns(&[11, 9], 0)
        };
        let (left, right) = (cols[0], cols[1]);
        let items = self.items();
        self.list.fix(&items);
        let inner = ui::panel(buf, left, ic.fuzzer, "syz-manager", self.focus == 0);
        let help_lines =
            super::buffer::wrap(&items[self.list.sel].help, inner.w.saturating_sub(4)).len();
        let help_h = (help_lines + 1).clamp(3, 6).min(inner.h / 3);
        let (list_r, help_r) = inner.split_top(inner.h - help_h);
        let label_w = 21.min(list_r.w * 2 / 5);
        ui::item_list(
            buf,
            list_r,
            &items,
            &mut self.list,
            self.focus == 0,
            label_w,
        );
        ui::help_box(buf, help_r, &items[self.list.sel].help);

        let checks = self.checks();
        let ch = (checks.len() + 2).min(right.h / 2).max(3);
        let (top, bottom) = right.split_top(ch);
        let hinner = ui::panel(buf, top, ic.search, "Config Health", false);
        let bad = checks.iter().filter(|c| c.0 == Some(false)).count();
        ui::panel_note(
            buf,
            top,
            &[span(
                if bad == 0 {
                    " ready ".to_string()
                } else {
                    format!(" {} issue(s) ", bad)
                },
                Style::new(theme::BG, if bad == 0 { theme::GREEN } else { theme::RED }).bold(),
            )],
        );
        for (i, (ok, msg)) in checks.iter().enumerate().take(hinner.h) {
            let msg = super::buffer::truncate(msg, hinner.w.saturating_sub(4));
            buf.spans(
                hinner.x + 1,
                hinner.y + i,
                &[ui::mark(*ok), span(msg, theme::BASE)],
                hinner.right(),
            );
        }

        let title = format!(
            "{} · {}",
            if self.is_template() {
                "Template"
            } else {
                "Config"
            },
            rel(&self.root, &self.cfg_path)
        );
        let pinner = ui::panel(buf, bottom, ic.file, &title, self.focus == 1);
        if self.lines.is_empty() {
            let msg = [
                format!("{} No manager config at this path yet.", ic.info),
                String::new(),
                "Select a template and press g to generate one,".to_string(),
                "or press e to write it from scratch in vim.".to_string(),
            ];
            for (i, l) in msg.iter().enumerate() {
                buf.text(
                    pinner.x + 2,
                    pinner.y + 1 + i,
                    l,
                    st(theme::MUTED),
                    pinner.right(),
                );
            }
        } else {
            let lines = self.lines.clone();
            ui::source_view(buf, pinner, &lines, &mut self.scroll, false);
        }

        if self.focus == 1 {
            return vec![
                ("↑↓", "Scroll"),
                ("Enter", "Edit in vim"),
                ("Tab", "Focus list"),
                ("v", "Validate"),
            ];
        }
        let mut h = vec![("↑↓", "Move")];
        match items[self.list.sel].kind {
            ItemKind::Toggle => h.push(("Space", "Toggle")),
            ItemKind::Choice => h.push(("←→", "Cycle")),
            ItemKind::Path | ItemKind::Number => h.push(("Enter", "Edit")),
            ItemKind::Action => h.push(("Enter", "Run")),
            _ => {}
        }
        h.extend([
            ("e", "Edit in vim"),
            ("g", "Generate"),
            ("v", "Validate"),
            ("Tab", "Focus preview"),
        ]);
        h
    }
}
