//! Rust rewrite of `tools/check-asterinas-kcov_test.py`.
//!
//! Validates `check-asterinas-kcov` orchestration, exit code 77 handling,
//! PASS marker checking, and port validation.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use syzaster::pkg::TempDir;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_check-asterinas-kcov"))
}

fn run_check(status: &str, output: &str, port: &str) -> Output {
    let tmp = TempDir::new("kcov-check-test-").unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(root.join("tools/kcov-smoke.c"), "int main(void) { return 0; }\n").unwrap();
    fs::write(root.join("key"), "dummy-key\n").unwrap();
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let scripts = [
        ("cc", "touch \"${@: -1}\"\nexit 0"),
        ("scp", "exit 0"),
        (
            "ssh",
            "case \"${!#}\" in\n  'mktemp '*) echo /tmp/kcov-check.ABC123 ;;\n  'chmod '*) printf '%s\\n' \"$TEST_OUTPUT\"; exit \"$TEST_STATUS\" ;;\nesac",
        ),
    ];

    for (name, body) in scripts {
        let p = bin_dir.join(name);
        fs::write(&p, format!("#!/bin/bash\n{}\n", body)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let orig_path = env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), orig_path);

    Command::new(bin_path())
        .args([
            root.to_str().unwrap(),
            root.join("key").to_str().unwrap(),
            port,
        ])
        .env("PATH", new_path)
        .env("CC", "cc")
        .env("TEST_STATUS", status)
        .env("TEST_OUTPUT", output)
        .output()
        .expect("failed to execute check-asterinas-kcov")
}

#[test]
fn test_pass() {
    let res = run_check("0", "PASS: real KCOV PCs, test", "10022");
    assert_eq!(
        res.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&res.stderr)
    );
}

#[test]
fn test_skip_is_failure() {
    let res = run_check("77", "PASS: real KCOV PCs, test", "10022");
    assert_ne!(res.status.code(), Some(0));
}

#[test]
fn test_missing_marker() {
    let res = run_check("0", "no coverage", "10022");
    assert_ne!(res.status.code(), Some(0));
}

#[test]
fn test_failure_with_marker() {
    let res = run_check("1", "PASS: real KCOV PCs, test", "10022");
    assert_ne!(res.status.code(), Some(0));
}

#[test]
fn test_invalid_port() {
    let res = run_check("0", "PASS: real KCOV PCs, test", "65536");
    assert_eq!(res.status.code(), Some(2));
}
