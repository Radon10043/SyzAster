# Agents Guidelines for SyzAster

SyzAster is a coverage-guided kernel fuzzing prototype specifically designed for the Asterinas Rust framekernel (`v0.18.1`). It is built on top of `syzkaller` (as a Git submodule) combined with purpose-built Rust verification tools, a compiler-wrapper instrumenter, statistical evaluation engines, and lifecycle shell automation scripts. SyzAster was created for the Rust Prototype Tool Contest, adopting design patterns and organizational standards aligned with SyzPilot.

This document serves as the authoritative operational guide for AI agents and human contributors interacting with, maintaining, and extending the SyzAster codebase.

## Repository Overview

- **`bin/`**: Destination directory for compiled release binaries (`syzaster`, `check-asterinas-kcov`, `check-kcov-inst`, `kcov-rustc`, `eval-fuzz`).
- **`src/`**: Rust codebase providing core verification, compiler wrapping, and evaluation capabilities.
  - **`src/bin/`**: Cargo binary entry points (`syzaster.rs`, `check-asterinas-kcov.rs`, `check-kcov-inst.rs`, `kcov-rustc.rs`, `eval-fuzz.rs`).
  - **`src/pkg/`**: Zero-dependency library modules (`elf.rs`, `eval.rs`, `json.rs`, `kcov.rs`, `tools.rs`).
- **`syzkaller/`**: Upstream `syzkaller` repository tracked as a Git submodule.
- **`patch/`**: Curated patch sets for upstream components.
  - **`patch/syzkaller/`**: Patches adapting `syzkaller` for Asterinas (`/dev/kcov` fallback, symbol resolution, panic parsing, configuration templates, adaptive snapshot restoration).
  - **`patch/asterinas/v0.18.1/`**: Patches providing task-scoped KCOV runtime, `/dev/kcov` character device, and `/proc/kallsyms` for Asterinas `v0.18.1`.
- **`scripts/`**: Standalone, pure Shell scripts for kernel patching, image generation, VM launching, and crash reproduction.
- **`configs/`**: Reusable configuration templates for fuzzing campaigns, kernel targets, and repro runs.
- **`docker/`**: Self-contained Dockerfile providing the entire build, toolchain, NixOS, and QEMU execution environment.
- **`experiment/`**: Benchmark harness, Docker Compose orchestration files, and experiment runbooks.
- **`docs/`**: Deep-dive technical references (`asterinas.md`, `kcov.md`, `subsystem.md`).
- **`tests/`**: Integration and regression tests verifying ELF metadata, KCOV instrumentation boundaries, and shell script tools.

## Architectural Principles

### Framekernel-Aware KCOV Boundary

Asterinas enforces a strict framekernel architecture:
- **`ostd` (TCB)**: Minimal, unsafe OS framework containing low-level hardware abstractions, memory managers, and interrupt handlers.
- **`aster_core`**: Safe OS service layer implementing POSIX system calls, virtual file systems, and network abstractions.

Applying coverage instrumentation naively across the entire kernel causes reentrancy cycles and lock-ordering deadlocks. SyzAster enforces:
- **Selective Instrumentation**: Only `aster_core` is instrumented via `bin/kcov-rustc` using LLVM SanitizerCoverage (`-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3 -C llvm-args=-sanitizer-coverage-trace-pc`).
- **TCB Isolation**: `ostd` must never be instrumented.
- **LTO Prohibition**: Cross-crate Link-Time Optimization is explicitly disabled (`CARGO_PROFILE_DEV_LTO=false`, `CARGO_PROFILE_RELEASE_LTO=false`) so the compiler cannot inline instrumented code into uninstrumented TCB contexts.

### Task-Scoped `/dev/kcov` & Non-Reentrant Recording

- Trace collection via `ostd::kcov` is restricted to task context (`InterruptLevel::current().is_task_context()`).
- Reentrancy guards use `try_lock()` rather than blocking locks to prevent recursive spinlock deadlocks.
- The KCOV buffer is backed by shared anonymous memory (`Vmo`) exposed to userspace at `/dev/kcov`.

### Syzkaller Adaptations for Asterinas

- **Device Fallback**: Asterinas exposes `/dev/kcov` instead of `/sys/kernel/debug/kcov`. `syz-executor` falls back automatically when debugfs returns `ENOENT`.
- **Symbol Resolution**: When `_stext` is omitted, the symbol backend falls back to `__stext` from `/proc/kallsyms`.
- **Panic Parsing**: Syzkaller report parsing recognizes Asterinas panic formats (`Uncaught panic: ...`) and extracts file/line source coordinates for accurate bug deduplication.

## Strict Engineering Conventions

When modifying or adding code to SyzAster, agents must adhere to the following decisions established in prior development:

### Target Kernel Versioning

- **Target Version**: Asterinas `v0.18.1` (`d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`) is the sole targeted kernel release.
- **No Branch Bifurcation**: Do not split code, configs, or patches into `main` vs `v0.18.1`. Kernel patches reside exclusively in `patch/asterinas/v0.18.1/`.

### Working Directory Uniformity

- **Directory Name**: Always use `.workdir` as the standard working directory across all scripts, configurations, and documentation.
- **Forbidden Pattern**: Do not use `workdir-asterinas` or other ad-hoc names.

### Patch Management Rules

- **No Series Files**: Do not generate or maintain quilt/git-series `series` files. Patches rely on standard numerical prefixes (`0001-...patch`, `0002-...patch`) to define ordering.
- **Patch Application**: Upstream patches must be applicable via `git apply patch/.../*.patch`.
- **Commit Message Line Wrapping**: Commit messages within patch files must be wrapped at 80 characters for readability in terminal viewers.

### Scripting vs. Rust Division of Responsibility

- **Pure Shell for Automation**: Scripts under `scripts/` must be pure Shell scripts (Bash/POSIX). Do not create Rust wrapper binaries for trivial shell tasks, and avoid wrapping simple Rust calls in redundant shell scripts.
- **Rust for Core Logic**: Rust is strictly reserved for tools requiring type safety, ELF inspection, complex IPC/KCOV validation, compiler wrapping, and statistical computation (`src/pkg/` and `src/bin/`).
- **Executable Placement**: Compiled Rust tools belong in `bin/`, with their sources in `src/bin/`.

### Configuration Management Rules

- **No Auto-Generated Configs**: `scripts/asterinas/create-image.sh` must not automatically generate or overwrite `syz-manager` config files.
- **Placeholder Retention**: Configuration templates in `configs/` must retain clear `__XXX__` placeholders (such as `__SYZASTER__`, `__IMAGE__`, `__ASTERINAS__`) intended for manual configuration by the user or agent.
- **Repro Config**: Keep only `configs/repro/reprosh.cfg`; do not introduce redundant duplicate configuration files.

### Dockerfile Optimization

- **Cached Installation**: In `docker/Dockerfile`, basic package installations must be broken down into discrete `RUN apt-get install -y ...` layers rather than one monolithic command, ensuring network or cache recovery without rebuilding earlier layers.

### Git Version Control Conventions

- **Active Branch**: `main`.
- **Commit Author**: `Jiaming Zhang <772577952@qq.com>`.
- **Commit Message Format**: Follow SyzPilot modular semantic commit convention: `<module>: <concise description>`.
  - Examples: `scripts: add helper scripts for Asterinas`, `patch/syzkaller: add patches for Asterinas coverage`, `configs: add fuzzing config files`.
- **Commit Message Line & Length Limits**: Neither the subject line nor any line in the commit body may exceed 80 characters; the total commit message must not exceed 80 lines.
- **No Push Without Explicit Consent**: Never push commits or tags to the remote repository unless explicitly instructed by the user.
- **Modular Commits**: Separate unrelated changes into distinct, topic-focused commits.

### Documentation Style Guidelines

- **No Directory Structure Section**: Do not include raw ASCII directory trees in `README.md`.
- **No Numbered Steps or Lists**: Do not use numbered lists (`1.`, `2.`) or numbered step headings (`Step 1`, `Step 2`, `## 1.`) anywhere in documentation, as they are fragile and difficult to maintain. Use unordered bullet points (`-`) and descriptive headings instead.
- **Self-Contained README**: Ensure the root `README.md` provides all critical onboarding instructions directly, without forcing users or evaluators to traverse deep link hierarchies.

## Essential Commands & Toolchains

### Environment & Toolchain Prerequisites

Ensure the following tools are available in your path:
- Rust nightly toolchain (`cargo`, `rustc`, `rustfmt`)
- Go toolchain (`go >= 1.22`)
- GNU Make, GCC, Clang/LLVM, QEMU (`qemu-system-x86_64`)
- Nix package manager with `nixos-install-tools`

### Building SyzAster Rust Tools

Compile all Rust CLI binaries and stage them into `./bin/`:
```bash
make rust-tools
```

Or run in release mode via Cargo directly:
```bash
cargo build --release
```

### Managing Syzkaller Submodule & Patches

Apply Asterinas-specific patches to `syzkaller`:
```bash
make patch-syzkaller
```

Build syzkaller binaries (`syz-manager`, `syz-executor`, `syz-fuzzer`, etc.):
```bash
make syzkaller
```

Revert all applied patches and clean the `syzkaller` submodule:
```bash
make unpatch-syzkaller
```

### Running Test Suites

Execute unit and integration tests across ELF parsers, KCOV validators, and shell scripts:
```bash
cargo test
```

### Kernel Patching & Image Generation

Checkout Asterinas `v0.18.1` and apply kernel patches:
```bash
./scripts/update-kernel.sh -d /vol
```

Generate the Asterinas NixOS disk image, SSH key, and staged `vmlinux`:
```bash
./scripts/asterinas/create-image.sh -k /vol/asterinas -o ./.workdir
```

Boot the resulting VM in QEMU:
```bash
./scripts/asterinas/qemu.sh -i ./.workdir/asterinas.img -p 10022
```

### Crash Triage & Statistical Evaluation

Summarize fuzzing crash reports into a CSV spreadsheet:
```bash
./scripts/csum.sh -d ./.workdir/crashes -o ./.workdir/crashes.csv
```

Extract crash logs matching specific bug titles:
```bash
./scripts/logs.sh -d ./.workdir/crashes -i interest.txt
```

Run statistical comparisons across fuzzing campaigns (calculating median coverage, $\hat{A}_{12}$ effect size, and $p$-values):
```bash
./bin/eval-fuzz --expdir /vol/experiment --master SyzAster --format markdown
```

## Agent Checklist Before Submitting Changes

Before concluding any task or committing changes in SyzAster, verify:
- All Rust tests pass cleanly (`cargo test`).
- Code changes in `scripts/` remain pure Shell without unnecessary wrapper binaries.
- No `series` files have been introduced in `patch/`.
- Patch commit messages are wrapped at 80 columns.
- The target Asterinas version remains fixed to `v0.18.1`.
- Any updated documentation avoids numbered step prefixes (`Step 1`, `1.`, `2.`) and raw directory trees.
- Git commits follow `<module>: <concise description>` with author set to `Jiaming Zhang <772577952@qq.com>`.
- Commit message title and body lines never exceed 80 characters (and total message under 80 lines).
- Commits are kept locally and never pushed to remote without explicit user instructions.
