//! KCOV verification and compiler instrumentation tools in Rust:
//! - `check-asterinas-kcov`: Builds `kcov-smoke.c`, uploads over SSH, and validates the KCOV ABI in a live VM.
//! - `check-kcov-inst`: Verifies that `kcov-rustc` instruments `aster_core` with `__sanitizer_cov_trace_pc` while leaving `ostd` uninstrumented.
//! - `kcov-rustc`: Rust compiler wrapper that injects `-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3 -C llvm-args=-sanitizer-coverage-trace-pc` exclusively for `aster_core` on `x86_64-unknown-none`.

use crate::pkg::elf::inspect_elf;
use crate::pkg::{find_syzaster_root, TempDir};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn print_kcov_usage(prog: &str) {
    eprintln!("Usage: {} ASTERINAS_SOURCE SSH_PRIVATE_KEY SSH_PORT [SSH_HOST]", prog);
    eprintln!("Build and run the KCOV ABI test in an already booted, trusted test VM.");
}

fn is_valid_remote_temp_dir(s: &str) -> bool {
    let Some(suffix) = s.strip_prefix("/tmp/kcov-check.") else {
        return false;
    };
    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn is_readable_file(path: &Path) -> bool {
    fs::File::open(path).is_ok()
}

/// Executes `check-asterinas-kcov` with full behavioral parity with `check-asterinas-kcov.sh`.
pub fn run_check_asterinas_kcov(prog: &str, args: &[String]) -> Result<(), i32> {
    if args.len() == 1 && (args[0] == "-h" || args[0] == "--help") {
        print_kcov_usage(prog);
        return Ok(());
    }
    if args.len() < 3 || args.len() > 4 {
        print_kcov_usage(prog);
        return Err(2);
    }

    let source_dir = Path::new(&args[0]);
    let key = Path::new(&args[1]);
    let port_str = &args[2];
    let host = args.get(3).map(|s| s.as_str()).unwrap_or("127.0.0.1");

    if port_str.is_empty() || !port_str.bytes().all(|b| b.is_ascii_digit()) {
        print_kcov_usage(prog);
        return Err(2);
    }
    let Ok(port) = port_str.parse::<u32>() else {
        print_kcov_usage(prog);
        return Err(2);
    };
    if !(1..=65535).contains(&port) {
        print_kcov_usage(prog);
        return Err(2);
    }

    if host.is_empty() || host.starts_with('-') || host.chars().any(|c| c.is_whitespace()) {
        print_kcov_usage(prog);
        return Err(2);
    }

    let smoke_src = source_dir.join("tools/kcov-smoke.c");
    if !is_readable_file(&smoke_src) {
        eprintln!("Missing tools/kcov-smoke.c: apply the KCOV patch first.");
        return Err(1);
    }
    if !is_readable_file(key) {
        eprintln!("Cannot read SSH key: {}", key.display());
        return Err(1);
    }

    let work = match TempDir::new("kcov-host-") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to create temporary directory: {}", e);
            return Err(1);
        }
    };
    let known_hosts = work.path().join("known_hosts");
    let smoke_bin = work.path().join("kcov-smoke");

    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let cc_status = Command::new(&cc)
        .args([
            "-static",
            "-pthread",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            &smoke_src.to_string_lossy(),
            "-o",
            &smoke_bin.to_string_lossy(),
        ])
        .status();
    match cc_status {
        Ok(s) if s.success() => {}
        _ => {
            eprintln!("Failed to compile {}", smoke_src.display());
            return Err(1);
        }
    }

    let ssh_opts = vec![
        "-i".to_string(),
        key.to_string_lossy().into_owned(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "IdentitiesOnly=yes".to_string(),
        "-o".to_string(),
        "ConnectTimeout=10".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        format!("UserKnownHostsFile={}", known_hosts.display()),
    ];

    let target = format!("root@{}", host);
    let mut mktemp_cmd = Command::new("timeout");
    mktemp_cmd.args(["30", "ssh"]).args(&ssh_opts).args([
        "-p",
        port_str,
        &target,
        "mktemp -d /tmp/kcov-check.XXXXXXXX",
    ]);
    let mktemp_out = match mktemp_cmd.output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            eprintln!("Failed to create remote temporary directory over SSH");
            return Err(1);
        }
    };

    if !is_valid_remote_temp_dir(&mktemp_out) {
        eprintln!("Unexpected guest temporary path");
        return Err(1);
    }

    struct RemoteGuard {
        ssh_opts: Vec<String>,
        port_str: String,
        target: String,
        remote: String,
    }
    impl Drop for RemoteGuard {
        fn drop(&mut self) {
            let _ = Command::new("timeout")
                .args(["15", "ssh"])
                .args(&self.ssh_opts)
                .args([
                    "-p",
                    &self.port_str,
                    &self.target,
                    &format!("rm -rf '{}'", self.remote),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _remote_guard = RemoteGuard {
        ssh_opts: ssh_opts.clone(),
        port_str: port_str.clone(),
        target: target.clone(),
        remote: mktemp_out.clone(),
    };

    let scp_status = Command::new("timeout")
        .args(["60", "scp", "-O"])
        .args(&ssh_opts)
        .args([
            "-P",
            port_str,
            &smoke_bin.to_string_lossy(),
            &format!("{}:{}/kcov-smoke", target, mktemp_out),
        ])
        .status();
    match scp_status {
        Ok(s) if s.success() => {}
        _ => {
            eprintln!("Failed to copy kcov-smoke to guest");
            return Err(1);
        }
    }

    let remote_cmd = format!(
        "chmod 700 '{r}/kcov-smoke' && '{r}/kcov-smoke'",
        r = mktemp_out
    );
    let mut run_cmd = Command::new("timeout");
    run_cmd
        .args(["120", "ssh"])
        .args(&ssh_opts)
        .args(["-p", port_str, &target, &remote_cmd])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match run_cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to execute remote KCOV test: {}", e);
            return Err(1);
        }
    };

    let mut combined = String::new();
    if let Some(mut out) = child.stdout.take() {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        print!("{}", s);
        combined.push_str(&s);
    }
    if let Some(mut err) = child.stderr.take() {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        eprint!("{}", s);
        combined.push_str(&s);
    }

    let status = match child.wait() {
        Ok(s) => s,
        Err(_) => return Err(1),
    };
    let code = status.code().unwrap_or(1);
    if code != 0 {
        eprintln!(
            "KCOV validation failed (status {}; 77 means KCOV is absent).",
            code
        );
        return Err(1);
    }

    let has_pass_marker = combined
        .lines()
        .any(|line| line.starts_with("PASS: real KCOV PCs,"));
    if !has_pass_marker {
        eprintln!("Missing KCOV PASS marker");
        return Err(1);
    }

    println!("KCOV ABI validated. Next run syz-manager smoke-test and verify coverage growth during fuzzing.");
    Ok(())
}

/// Rust compiler wrapper (`kcov-rustc`) that instruments only `aster_core` for `x86_64-unknown-none`.
pub fn run_kcov_rustc(args: &[String]) -> Result<(), i32> {
    if args.is_empty() {
        eprintln!("Usage: kcov-rustc <RUSTC> [ARGS...]");
        return Err(1);
    }
    let rustc = &args[0];
    let rest = &args[1..];

    let mut crate_name = None;
    let mut target = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--crate-name" => {
                if let Some(v) = rest.get(i + 1) {
                    crate_name = Some(v.as_str());
                    i += 2;
                    continue;
                }
            }
            "--target" => {
                if let Some(v) = rest.get(i + 1) {
                    target = Some(v.as_str());
                    i += 2;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut cmd = Command::new(rustc);
    cmd.args(rest);
    if crate_name == Some("aster_core") && target == Some("x86_64-unknown-none") {
        cmd.args([
            "-C",
            "passes=sancov-module",
            "-C",
            "llvm-args=-sanitizer-coverage-level=3",
            "-C",
            "llvm-args=-sanitizer-coverage-trace-pc",
        ]);
    }

    match cmd.status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("failed to execute {}: {}", rustc, e);
            Err(1)
        }
    }
}

/// Verifies that the pinned Rust compiler and `kcov-rustc` wrapper emit `__sanitizer_cov_trace_pc`
/// callbacks for `aster_core` while keeping `ostd` uninstrumented.
pub fn run_check_kcov_instrumentation(prog: &str, args: &[String]) -> Result<(), i32> {
    let mut asterinas_dir: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--asterinas" => {
                let Some(val) = args.get(i + 1) else {
                    eprintln!("{} requires an argument", args[i]);
                    return Err(1);
                };
                asterinas_dir = Some(PathBuf::from(val));
                i += 2;
            }
            "-h" | "--help" => {
                println!("Usage: {} [-d|--asterinas ASTERINAS_DIR]", prog);
                println!("Check that the Rust compiler and KCOV wrapper instrument aster_core and keep ostd uninstrumented.");
                return Ok(());
            }
            other if !other.starts_with('-') && asterinas_dir.is_none() => {
                asterinas_dir = Some(PathBuf::from(other));
                i += 1;
            }
            other => {
                eprintln!("unknown option: {}", other);
                return Err(1);
            }
        }
    }

    let work = match TempDir::new("kcov-inst-") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("failed to create temp dir: {}", e);
            return Err(1);
        }
    };

    let probe_rs = work.path().join("probe.rs");
    let probe_code = "#![no_std]\n\
#[unsafe(no_mangle)]\n\
pub extern \"C\" fn branch(x: usize) -> usize {\n\
    if x > 7 { x / 3 } else { x + 9 }\n\
}\n";
    if let Err(e) = fs::write(&probe_rs, probe_code) {
        eprintln!("failed to write probe.rs: {}", e);
        return Err(1);
    }

    let rustc = env::var("RUSTC").unwrap_or_else(|_| {
        let home = env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let cargo_rustc = Path::new(&home).join(".cargo/bin/rustc");
        if cargo_rustc.is_file() {
            cargo_rustc.to_string_lossy().into_owned()
        } else {
            "rustc".to_string()
        }
    });

    // Prefer wrapper in asterinas_dir/tools/kcov-rustc.sh if specified & exists, else use our built-in wrapper logic
    let wrapper_script = asterinas_dir
        .as_ref()
        .map(|d| d.join("tools/kcov-rustc.sh"))
        .filter(|p| p.is_file())
        .or_else(|| {
            let root = find_syzaster_root();
            let bin_wrapper = root.join("bin/kcov-rustc");
            if bin_wrapper.is_file() {
                Some(bin_wrapper)
            } else {
                None
            }
        });

    for krate in ["aster_core", "ostd"] {
        let out_obj = work.path().join(format!("{}.o", krate));
        let compile_args = vec![
            rustc.clone(),
            "--crate-name".to_string(),
            krate.to_string(),
            "--crate-type".to_string(),
            "lib".to_string(),
            "--target".to_string(),
            "x86_64-unknown-none".to_string(),
            "--emit=obj".to_string(),
            "-C".to_string(),
            "opt-level=1".to_string(),
            probe_rs.to_string_lossy().into_owned(),
            "-o".to_string(),
            out_obj.to_string_lossy().into_owned(),
        ];

        let status = if let Some(ref wrapper) = wrapper_script {
            Command::new(wrapper)
                .args(&compile_args)
                .current_dir(asterinas_dir.as_deref().unwrap_or_else(|| Path::new(".")))
                .status()
                .map(|s| if s.success() { Ok(()) } else { Err(1) })
                .unwrap_or(Err(1))
        } else {
            run_kcov_rustc(&compile_args)
        };

        if status.is_err() {
            eprintln!("FAIL: compilation failed for crate {}", krate);
            return Err(1);
        }
    }

    let aster_info = match inspect_elf(&work.path().join("aster_core.o")) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("FAIL: could not inspect aster_core.o: {}", e);
            return Err(1);
        }
    };
    let ostd_info = match inspect_elf(&work.path().join("ostd.o")) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("FAIL: could not inspect ostd.o: {}", e);
            return Err(1);
        }
    };

    if !aster_info.references_undefined_symbol("__sanitizer_cov_trace_pc") {
        eprintln!("FAIL: aster_core.o does not contain __sanitizer_cov_trace_pc callbacks");
        return Err(1);
    }
    if ostd_info.references_undefined_symbol("__sanitizer_cov_trace_pc") {
        eprintln!("FAIL: the coverage runtime would be instrumented");
        return Err(1);
    }

    println!("PASS: safe kernel gets trace-pc callbacks; OSTD remains uninstrumented");
    Ok(())
}
