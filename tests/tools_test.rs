//! Unit and integration tests for SyzAster Rust tools (`eval-fuzz`, `check-kcov-inst`) and shell scripts.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use syzaster::pkg::eval::{evaluate_experiment_dir, mann_whitney_u_test};
use syzaster::pkg::{find_syzaster_root, TempDir};

#[test]
fn test_shell_scripts_csum_and_logs() {
    let root = find_syzaster_root();
    let tmp = TempDir::new("syzaster-csum-test-").unwrap();
    let crash_dir = tmp.path().join("crashes");
    let c1 = crash_dir.join("aaa111");
    let c2 = crash_dir.join("bbb222");
    fs::create_dir_all(&c1).unwrap();
    fs::create_dir_all(&c2).unwrap();

    fs::write(
        c1.join("description"),
        "asterinas panic: assertion failed: addr.is_unicast() at src/iface/route.rs:LINE\n",
    )
    .unwrap();
    fs::write(c1.join("log0"), "crash log 0\n").unwrap();

    fs::write(
        c2.join("description"),
        "asterinas panic: not implemented at src/syscall/timer_create.rs:LINE\n",
    )
    .unwrap();
    fs::write(c2.join("log0"), "crash log 0\n").unwrap();
    fs::write(c2.join("log1"), "crash log 1\n").unwrap();

    let csv_out = tmp.path().join("summary.csv");
    let csum_sh = root.join("scripts/csum.sh");
    let status = Command::new("bash")
        .args([
            csum_sh.to_str().unwrap(),
            "-d",
            crash_dir.to_str().unwrap(),
            "-o",
            csv_out.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let csv_text = fs::read_to_string(&csv_out).unwrap();
    assert!(csv_text.contains("status,title"));
    assert!(csv_text.contains(
        "unknown,\"asterinas panic: assertion failed: addr.is_unicast() at src/iface/route.rs:LINE\""
    ));

    let interest = tmp.path().join("interest.txt");
    fs::write(&interest, "timer_create.rs\n").unwrap();
    let logs_sh = root.join("scripts/logs.sh");
    let out = Command::new("bash")
        .args([
            logs_sh.to_str().unwrap(),
            "-d",
            crash_dir.to_str().unwrap(),
            "-i",
            interest.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let logs_stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(logs_stdout.lines().count(), 2);
}

#[test]
fn test_eval_fuzz_and_mann_whitney() {
    let (u, p) = mann_whitney_u_test(&[100.0, 110.0, 120.0], &[50.0, 60.0, 70.0]);
    assert_eq!(u, 9.0);
    assert!(p < 0.1);

    let tmp = TempDir::new("syzaster-eval-test-").unwrap();
    let exp = tmp.path();

    for (fuzzer, covs, n_crashes) in [
        ("SyzAster", [1200, 1300, 1250], 2usize),
        ("baseline", [600, 650, 700], 1usize),
    ] {
        for (idx, cov) in covs.iter().enumerate() {
            let run_dir = exp
                .join(fuzzer)
                .join("asterinas-v0.18.1-kernel")
                .join(format!("out-{}", idx));
            let crash_dir = run_dir.join("crashes");
            fs::create_dir_all(&crash_dir).unwrap();
            fs::write(
                run_dir.join("fuzz.log"),
                format!("2026/10/31 VMs=1, executed=5000, coverage={}, crashes={}\n", cov, n_crashes),
            )
            .unwrap();
            for c in 0..n_crashes {
                fs::create_dir_all(crash_dir.join(format!("crash_{}", c))).unwrap();
            }
        }
    }

    let md = evaluate_experiment_dir(exp.to_str().unwrap(), "SyzAster", true, "markdown").unwrap();
    assert!(md.contains("SyzAster"));
    assert!(md.contains("baseline"));
    assert!(md.contains("1250"));
    assert!(md.contains("650"));
}

#[test]
fn test_check_kcov_inst_live_rustc() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_check-kcov-inst"));
    let out = Command::new(bin)
        .output()
        .expect("run check-kcov-inst");
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("PASS: safe kernel gets trace-pc callbacks; OSTD remains uninstrumented"));
}
