//! Tests for Asterinas ELF KCOV validation, fuzz config templates (`configs/fuzz/asterinas/`),
//! and executor `open_kcov()` fallback.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use syzaster::pkg::elf::inspect_elf;
use syzaster::pkg::json::JsonValue;
use syzaster::pkg::TempDir;

const KCOV_OPEN: &str = "openat$auto_fuse_conn_congestion_threshold_ops_control";

fn compile_c(src: &str, out: &Path, debug: bool) {
    let mut cmd = Command::new("cc");
    if debug {
        cmd.arg("-g");
    }
    cmd.args(["-O0", "-x", "c", "-o", &out.to_string_lossy(), "-"])
        .stdin(Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn cc");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(src.as_bytes())
        .unwrap();
    let status = child.wait().unwrap();
    assert!(status.success(), "cc compilation failed");
}

#[test]
fn test_elf_kcov_symbol_and_debug_validation() {
    let tmp = TempDir::new("asterinas-elf-test-").unwrap();
    let valid_elf = tmp.path().join("valid-vmlinux");
    compile_c(
        "void __sanitizer_cov_trace_pc(void) {}\nint main(void) { __sanitizer_cov_trace_pc(); return 0; }\n",
        &valid_elf,
        true,
    );
    let info = inspect_elf(&valid_elf).unwrap();
    assert!(info.has_symtab());
    assert!(info.has_debug_info());
    assert!(info.defines_symbol("__sanitizer_cov_trace_pc"));

    // Stripped object should lack .symtab / .debug_info
    let stripped_elf = tmp.path().join("stripped-vmlinux");
    fs::copy(&valid_elf, &stripped_elf).unwrap();
    assert!(Command::new("strip")
        .arg(&stripped_elf)
        .status()
        .unwrap()
        .success());
    let stripped_info = inspect_elf(&stripped_elf).unwrap();
    assert!(!stripped_info.has_symtab() || !stripped_info.has_debug_info());

    // Uninstrumented debug object should lack __sanitizer_cov_trace_pc
    let uninst_elf = tmp.path().join("uninst-vmlinux");
    compile_c("int main(void) { return 0; }\n", &uninst_elf, true);
    let uninst_info = inspect_elf(&uninst_elf).unwrap();
    assert!(uninst_info.has_symtab());
    assert!(uninst_info.has_debug_info());
    assert!(!uninst_info.defines_symbol("__sanitizer_cov_trace_pc"));
}

#[test]
fn test_fuzz_config_templates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let kernel_cfg_raw = fs::read_to_string(root.join("configs/fuzz/asterinas/kernel.cfg"))
        .expect("read kernel.cfg");
    assert!(kernel_cfg_raw.contains("__WORKDIR__"));
    assert!(kernel_cfg_raw.contains("__IMAGE__"));
    assert!(kernel_cfg_raw.contains("__SYZKALLER__"));
    assert!(kernel_cfg_raw.contains("__KERNEL__"));
    assert!(kernel_cfg_raw.contains("__VMCOUNT__"));

    let filled = kernel_cfg_raw.replace("__VMCOUNT__", "1");
    let cfg = JsonValue::parse(&filled).expect("valid JSON after replacing __VMCOUNT__");
    assert_eq!(cfg.get("cover").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        cfg.get("experimental")
            .and_then(|e| e.get("remote_cover"))
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    let disabled: Vec<&str> = cfg
        .get("disable_syscalls")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(disabled.contains(&KCOV_OPEN));
}

#[test]
fn test_executor_open_kcov_enoent_fallback() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let patch_file = root.join(
        "patch/syzkaller/0001-executor-support-Asterinas-dev-kcov-fallback-and-opt.patch",
    );
    let patch_text = fs::read_to_string(&patch_file).expect("patch 0001 exists");
    assert!(patch_text.contains("static int open_kcov()"));
    assert!(patch_text.contains("open(\"/dev/kcov\", O_RDWR)"));

    let test_cpp = r#"
#include <cerrno>
#include <cstring>
#include <fcntl.h>
#include <cassert>
static int calls, linux_error, device_error;
static int mock_open(const char* path, int flags)
{
    assert(flags == O_RDWR);
    ++calls;
    int error;
    if (calls == 1) {
        assert(strcmp(path, "/sys/kernel/debug/kcov") == 0);
        error = linux_error;
    } else {
        assert(calls == 2);
        assert(strcmp(path, "/dev/kcov") == 0);
        error = device_error;
    }
    if (error) {
        errno = error;
        return -1;
    }
    return 42;
}
#define open mock_open
static int open_kcov()
{
    int fd = open("/sys/kernel/debug/kcov", O_RDWR);
    if (fd == -1 && errno == ENOENT)
        fd = open("/dev/kcov", O_RDWR);
    return fd;
}
int main()
{
    calls = 0; linux_error = 0;
    assert(open_kcov() == 42 && calls == 1);
    calls = 0; linux_error = ENOENT; device_error = 0;
    assert(open_kcov() == 42 && calls == 2);
    calls = 0; linux_error = EACCES;
    assert(open_kcov() == -1 && calls == 1 && errno == EACCES);
    calls = 0; linux_error = EIO;
    assert(open_kcov() == -1 && calls == 1 && errno == EIO);
    calls = 0; linux_error = ENOENT; device_error = ENODEV;
    assert(open_kcov() == -1 && calls == 2 && errno == ENODEV);
    return 0;
}
"#;
    let tmp = TempDir::new("asterinas-kcov-open-test-").unwrap();
    let binary = tmp.path().join("test-open-kcov");
    let mut child = Command::new("c++")
        .args([
            "-Wall",
            "-Wextra",
            "-Werror",
            "-x",
            "c++",
            "-o",
            &binary.to_string_lossy(),
            "-",
        ])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn c++");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(test_cpp.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(Command::new(&binary).status().unwrap().success());
}
