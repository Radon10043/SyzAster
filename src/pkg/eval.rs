//! Fuzzing campaign evaluation and statistical analysis tool (`eval-fuzz`).
//!
//! Rust rewrite of `SyzPilot/experiment/viz/fuzz/main.go`:
//! - Parses coverage (`fuzz.log` / `bench.log`) and unique crashes (`crashes/`) across repeated runs.
//! - Computes Mann-Whitney U test two-sided p-value (with tie correction) and Vargha-Delaney A12 effect size.
//! - Renders formatted tables in `text`, `markdown`, `csv`, `tsv`, and `html`.

use crate::pkg::json::JsonValue;
use crate::pkg::realpath_m;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kernel {
    pub name: String,
    pub version: String,
    pub subsystem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fuzzer {
    pub name: String,
    pub master: bool,
}

#[derive(Debug, Clone)]
pub struct EvalResult {
    pub coverage: Vec<u64>,
    pub avg_cov: u64,
    pub cov_a12: f64,
    pub cov_pv: f64,
    pub crashes: Vec<u64>,
    pub avg_cra: f64,
    pub cra_a12: f64,
    pub cra_pv: f64,
}

#[derive(Debug, Clone)]
pub struct ExpInfo {
    pub kernel: Kernel,
    pub fuzzer: Fuzzer,
    pub result: EvalResult,
}

pub fn run_eval_fuzz(prog: &str, args: &[String]) -> Result<(), i32> {
    let mut expdir = String::new();
    let mut master = String::new();
    let mut format = "text".to_string();
    let mut strict = false;

    let print_help = || {
        println!("Usage: {} [OPTIONS]", prog);
        println!("Options:");
        println!("  -expdir, --expdir <DIR>       path to the experiment directory");
        println!("  -master, --master <FUZZER>    master fuzzer name for statistical comparison");
        println!("  -format, --format <FMT>       output format: text, csv, tsv, markdown, html (default: text)");
        println!("  -strict, --strict             enforce identical repeat counts across all targets");
        println!("  -h, --help                    show this help message");
    };

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if let Some(val) = a.strip_prefix("-expdir=").or_else(|| a.strip_prefix("--expdir=")) {
            expdir = val.to_string();
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("-master=").or_else(|| a.strip_prefix("--master=")) {
            master = val.to_string();
            i += 1;
            continue;
        }
        if let Some(val) = a.strip_prefix("-format=").or_else(|| a.strip_prefix("--format=")) {
            format = val.to_string();
            i += 1;
            continue;
        }
        match a {
            "-expdir" | "--expdir" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("missing argument for {}", a);
                    return Err(1);
                };
                expdir = val.clone();
                i += 2;
            }
            "-master" | "--master" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("missing argument for {}", a);
                    return Err(1);
                };
                master = val.clone();
                i += 2;
            }
            "-format" | "--format" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("missing argument for {}", a);
                    return Err(1);
                };
                format = val.clone();
                i += 2;
            }
            "-strict" | "--strict" => {
                strict = true;
                i += 1;
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            other => {
                eprintln!("unknown option: {}", other);
                return Err(1);
            }
        }
    }

    match evaluate_experiment_dir(&expdir, &master, strict, &format) {
        Ok(output) => {
            print!("{}", output);
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            Err(1)
        }
    }
}

pub fn evaluate_experiment_dir(
    expdir: &str,
    master_name: &str,
    strict: bool,
    format: &str,
) -> Result<String, String> {
    check_expdir(expdir, strict)?;
    let abs_expdir = realpath_m(Path::new(expdir));

    let mut entries: Vec<PathBuf> = fs::read_dir(&abs_expdir)
        .map_err(|e| format!("failed to read {}: {}", abs_expdir.display(), e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();

    let mut ei_slice = Vec::new();
    for entry in entries {
        let mut parsed = parse_fuzzing(&abs_expdir, &entry, master_name)?;
        ei_slice.append(&mut parsed);
    }

    stat_test(&mut ei_slice);
    render_table(&mut ei_slice, format)
}

fn check_expdir(expdir: &str, strict: bool) -> Result<(), String> {
    if expdir.is_empty() {
        return Err("experiment directory is empty".to_string());
    }
    let path = Path::new(expdir);
    if !path.exists() {
        return Err(format!("experiment directory does not exist: {}", expdir));
    }
    if !strict {
        return Ok(());
    }

    let mut repeat: Option<usize> = None;
    let entries = fs::read_dir(path)
        .map_err(|e| format!("failed to read experiment dir: {}", e))?;
    for entry in entries.flatten() {
        let fuzzer_path = entry.path();
        if !fuzzer_path.is_dir() {
            continue;
        }
        let subentries = fs::read_dir(&fuzzer_path)
            .map_err(|e| format!("failed to read fuzzer dir: {}", e))?;
        for sub in subentries.flatten() {
            let target_path = sub.path();
            if !target_path.is_dir() {
                continue;
            }
            let logs = find_run_logs(&target_path);
            match repeat {
                None => repeat = Some(logs.len()),
                Some(prev) if prev != logs.len() => {
                    return Err(format!(
                        "inconsistent repeat times: previous is {} vs {} is {}",
                        prev,
                        target_path.display(),
                        logs.len()
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn find_run_logs(target_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(runs) = fs::read_dir(target_dir) else {
        return out;
    };
    for run in runs.flatten() {
        let p = run.path();
        if p.is_dir() {
            let fuzz_log = p.join("fuzz.log");
            let bench_log = p.join("bench.log");
            if fuzz_log.is_file() {
                out.push(fuzz_log);
            } else if bench_log.is_file() {
                out.push(bench_log);
            }
        }
    }
    out.sort();
    out
}

fn parse_fuzzing(
    expdir: &Path,
    fuzzer_dir: &Path,
    master_name: &str,
) -> Result<Vec<ExpInfo>, String> {
    let fuzzer_name = fuzzer_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let fuzzer = Fuzzer {
        master: !master_name.is_empty() && fuzzer_name == master_name,
        name: fuzzer_name,
    };

    let mut out = Vec::new();
    let entries = fs::read_dir(fuzzer_dir)
        .map_err(|e| format!("failed to read fuzzing dir: {}", e))?;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let dir_name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let parts: Vec<&str> = dir_name.split('-').collect();
        if parts.len() != 3 {
            return Err(format!("invalid dir name: {}", dir_name));
        }
        let kernel = Kernel {
            name: parts[0].to_string(),
            version: parts[1].to_string(),
            subsystem: parts[2].to_string(),
        };
        let result = parse_fuzzing_result(expdir, &fuzzer, &kernel)?;
        out.push(ExpInfo {
            kernel,
            fuzzer: fuzzer.clone(),
            result,
        });
    }
    Ok(out)
}

fn parse_fuzzing_result(
    expdir: &Path,
    fuzzer: &Fuzzer,
    kernel: &Kernel,
) -> Result<EvalResult, String> {
    let out_root = expdir
        .join(&fuzzer.name)
        .join(format!("{}-{}-{}", kernel.name, kernel.version, kernel.subsystem));
    let log_files = find_run_logs(&out_root);
    if log_files.is_empty() {
        return Err(format!("failed to find fuzz.log in {}", out_root.display()));
    }

    let repeat = log_files.len() as u64;
    let mut coverage = Vec::with_capacity(log_files.len());
    let mut total_cov = 0u64;
    for log_path in &log_files {
        let cov = parse_coverage(log_path)?;
        coverage.push(cov);
        total_cov += cov;
    }

    let mut crashes = Vec::new();
    let mut total_cra = 0u64;
    if let Ok(runs) = fs::read_dir(&out_root) {
        let mut run_dirs: Vec<PathBuf> = runs.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        run_dirs.sort();
        for run_dir in run_dirs {
            let crash_dir = run_dir.join("crashes");
            if crash_dir.is_dir() {
                let cra = parse_crashes(&crash_dir)?;
                crashes.push(cra);
                total_cra += cra;
            } else {
                crashes.push(0);
            }
        }
    }

    Ok(EvalResult {
        coverage,
        avg_cov: total_cov / repeat,
        cov_a12: -1.0,
        cov_pv: -1.0,
        crashes,
        avg_cra: (total_cra as f64) / (repeat as f64),
        cra_a12: -1.0,
        cra_pv: -1.0,
    })
}

pub fn parse_crashes(crash_dir: &Path) -> Result<u64, String> {
    let entries = fs::read_dir(crash_dir)
        .map_err(|e| format!("failed to read crashes dir {}: {}", crash_dir.display(), e))?;
    let count = entries.flatten().count() as u64;
    Ok(count)
}

pub fn parse_coverage(fpath: &Path) -> Result<u64, String> {
    let content = fs::read_to_string(fpath)
        .map_err(|e| format!("failed to read {}: {}", fpath.display(), e))?;
    for line in content.lines().rev() {
        if let Some((_, after)) = line.split_once("coverage=") {
            let num_str = after
                .split(|c: char| c.is_whitespace() || c == ',')
                .next()
                .unwrap_or_default();
            if let Ok(cov) = num_str.parse::<u64>() {
                return Ok(cov);
            }
        }
        // Also support syz-manager JSON bench.log format (`{"corpus":..., "covering": 1234}`)
        if line.trim_start().starts_with('{') {
            if let Ok(val) = JsonValue::parse(line.trim()) {
                if let Some(cov) = val.get("covering").and_then(|v| v.as_u64()) {
                    return Ok(cov);
                }
            }
        }
    }
    Ok(0)
}

fn stat_test(eis: &mut [ExpInfo]) {
    let master_fuzzer = eis.iter().find(|e| e.fuzzer.master).map(|e| e.fuzzer.clone());
    let Some(master) = master_fuzzer else {
        eprintln!("no master fuzzer found, skip statistical test");
        return;
    };

    let mut master_map: BTreeMap<Kernel, EvalResult> = BTreeMap::new();
    for ei in eis.iter() {
        if ei.fuzzer == master {
            master_map.insert(ei.kernel.clone(), ei.result.clone());
        }
    }

    for ei in eis.iter_mut() {
        if ei.fuzzer.master {
            continue;
        }
        if let Some(master_res) = master_map.get(&ei.kernel) {
            let m_cov: Vec<f64> = master_res.coverage.iter().map(|&v| v as f64).collect();
            let s_cov: Vec<f64> = ei.result.coverage.iter().map(|&v| v as f64).collect();
            let (cov_u, cov_p) = mann_whitney_u_test(&m_cov, &s_cov);
            if !m_cov.is_empty() && !s_cov.is_empty() {
                ei.result.cov_a12 = cov_u / ((m_cov.len() * s_cov.len()) as f64);
                ei.result.cov_pv = cov_p;
            }

            let m_cra: Vec<f64> = master_res.crashes.iter().map(|&v| v as f64).collect();
            let s_cra: Vec<f64> = ei.result.crashes.iter().map(|&v| v as f64).collect();
            let (cra_u, cra_p) = mann_whitney_u_test(&m_cra, &s_cra);
            if !m_cra.is_empty() && !s_cra.is_empty() {
                ei.result.cra_a12 = cra_u / ((m_cra.len() * s_cra.len()) as f64);
                ei.result.cra_pv = cra_p;
            }
        }
    }
}

/// Computes Mann-Whitney U statistic (`U1`) and two-sided p-value with tie correction.
pub fn mann_whitney_u_test(v1: &[f64], v2: &[f64]) -> (f64, f64) {
    let n1 = v1.len();
    let n2 = v2.len();
    if n1 == 0 || n2 == 0 {
        return (-1.0, -1.0);
    }

    let mut u1 = 0.0;
    for &x in v1 {
        for &y in v2 {
            if x > y {
                u1 += 1.0;
            } else if (x - y).abs() < 1e-12 {
                u1 += 0.5;
            }
        }
    }

    // Group values for tie correction
    let mut combined: Vec<f64> = Vec::with_capacity(n1 + n2);
    combined.extend_from_slice(v1);
    combined.extend_from_slice(v2);
    combined.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let n = (n1 + n2) as f64;
    let mut tie_sum = 0.0;
    let mut idx = 0;
    while idx < combined.len() {
        let mut end = idx + 1;
        while end < combined.len() && (combined[end] - combined[idx]).abs() < 1e-12 {
            end += 1;
        }
        let t = (end - idx) as f64;
        if t > 1.0 {
            tie_sum += t * t * t - t;
        }
        idx = end;
    }

    let mean_u = (n1 * n2) as f64 / 2.0;
    let tie_term = if n > 1.0 {
        tie_sum / (n * (n - 1.0))
    } else {
        0.0
    };
    let var_u = ((n1 * n2) as f64 / 12.0) * ((n + 1.0) - tie_term);
    if var_u <= 0.0 {
        return (u1, 1.0);
    }

    let diff = (u1 - mean_u).abs();
    let z = diff / var_u.sqrt();
    let p = (erfc_approx(z / std::f64::consts::SQRT_2)).clamp(0.0, 1.0);
    (u1, p)
}

/// High-precision complementary error function approximation (Abramowitz and Stegun 7.1.26).
fn erfc_approx(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let poly = (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t;
    let res = poly * (-z * z).exp();
    if x >= 0.0 {
        res
    } else {
        2.0 - res
    }
}

fn render_table(ei_slice: &mut [ExpInfo], format: &str) -> Result<String, String> {
    ei_slice.sort_by(|a, b| {
        let a_is_kernel = a.kernel.subsystem == "kernel";
        let b_is_kernel = b.kernel.subsystem == "kernel";
        if a_is_kernel && !b_is_kernel {
            return Ordering::Less;
        }
        if !a_is_kernel && b_is_kernel {
            return Ordering::Greater;
        }
        let a_key = format!(
            "{}-{}-{}-{}",
            a.kernel.name, a.kernel.subsystem, a.kernel.version, a.fuzzer.name
        );
        let b_key = format!(
            "{}-{}-{}-{}",
            b.kernel.name, b.kernel.subsystem, b.kernel.version, b.fuzzer.name
        );
        a_key.cmp(&b_key)
    });

    let headers = [
        "Kernel",
        "Version",
        "Subsystem",
        "Fuzzer",
        "Avg. Coverage",
        "CovA12",
        "CovPV",
        "Avg. Crashes",
        "CraA12",
        "CraPV",
    ];

    let rows: Vec<[String; 10]> = ei_slice
        .iter()
        .map(|ei| {
            [
                ei.kernel.name.clone(),
                ei.kernel.version.clone(),
                ei.kernel.subsystem.clone(),
                ei.fuzzer.name.clone(),
                ei.result.avg_cov.to_string(),
                format!("{:.4}", ei.result.cov_a12),
                format!("{:.4}", ei.result.cov_pv),
                format!("{:.2}", ei.result.avg_cra),
                format!("{:.4}", ei.result.cra_a12),
                format!("{:.4}", ei.result.cra_pv),
            ]
        })
        .collect();

    let mut out = String::new();
    match format {
        "csv" => {
            out.push_str(&headers.join(","));
            out.push('\n');
            for r in rows {
                out.push_str(&r.join(","));
                out.push('\n');
            }
        }
        "tsv" => {
            out.push_str(&headers.join("\t"));
            out.push('\n');
            for r in rows {
                out.push_str(&r.join("\t"));
                out.push('\n');
            }
        }
        "markdown" => {
            out.push_str("| ");
            out.push_str(&headers.join(" | "));
            out.push_str(" |\n|");
            for _ in &headers {
                out.push_str(" --- |");
            }
            out.push('\n');
            for r in rows {
                out.push_str("| ");
                out.push_str(&r.join(" | "));
                out.push_str(" |\n");
            }
        }
        "html" => {
            out.push_str("<table>\n  <thead>\n    <tr>");
            for h in &headers {
                out.push_str(&format!("<th>{}</th>", h));
            }
            out.push_str("</tr>\n  </thead>\n  <tbody>\n");
            for r in rows {
                out.push_str("    <tr>");
                for c in &r {
                    out.push_str(&format!("<td>{}</td>", c));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("  </tbody>\n</table>\n");
        }
        "text" => {
            let mut widths = [0usize; 10];
            for (i, h) in headers.iter().enumerate() {
                widths[i] = h.len();
            }
            for r in &rows {
                for (i, c) in r.iter().enumerate() {
                    widths[i] = widths[i].max(c.len());
                }
            }
            let sep = {
                let mut s = String::from("+");
                for w in &widths {
                    s.push_str(&"-".repeat(*w + 2));
                    s.push('+');
                }
                s.push('\n');
                s
            };
            out.push_str(&sep);
            out.push('|');
            for (i, h) in headers.iter().enumerate() {
                out.push_str(&format!(" {:width$} |", h, width = widths[i]));
            }
            out.push('\n');
            out.push_str(&sep);
            for r in &rows {
                out.push('|');
                for (i, c) in r.iter().enumerate() {
                    out.push_str(&format!(" {:width$} |", c, width = widths[i]));
                }
                out.push('\n');
            }
            out.push_str(&sep);
        }
        other => return Err(format!("Unsupported format: {}", other)),
    }

    Ok(out)
}
