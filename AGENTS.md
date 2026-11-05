# AGENTS.md

This document defines the development, collaboration, and commit conventions for
SyzAster, to be followed by all AI agents and developers.

---

## Git Commit & Self-Containment Principles

- **Atomic Commits (Keep Commits Small)**:
  - Each commit should focus strictly on a single logical change (e.g., split
    changes to Makefile, Rust tools, syzkaller patches, and shell scripts into
    separate commits).
  - Avoid combining unrelated modifications into a single commit.

- **Self-Containment & Compilability**:
  - **The repository must remain self-contained and compilable after every
    commit**.
  - Checking out any historical commit at any point in time must build cleanly
    via `make` (or `make rust-tools` / `cargo build --release`) and pass
    verification (`make test` / `cargo test`, `./bin/check-kcov-inst`) without
    breaking existing compilation or test pipelines.

- **No Push Without Explicit Consent**:
  - Never push commits or tags to the remote repository unless explicitly
    instructed by the user. Keep commits local.

---

## Commit Message Conventions

Commit messages must strictly adhere to the following structure:

```
[LOC]: [BRIEF]

[DETAILS]
```

### LOC (Location of Changes)
- **Root-level files**: Use the file name directly (e.g., `Makefile:`,
  `Cargo.toml:`, `AGENTS.md:`).
- **Subdirectory files**: Use the relative folder path (e.g., `src/bin:`,
  `src/pkg:`, `patch/syzkaller:`, `patch/asterinas:`, `scripts:`, `configs:`,
  `docker:`, `experiment:`).
- **Common parent rollup**: When modifications are distributed across multiple
  subdirectories under the same top-level directory (e.g., `src/bin` and
  `src/pkg` all under `src/`, or `patch/syzkaller` and `patch/asterinas` all
  under `patch/`), use their common ancestor directory as LOC (e.g., `src:` or
  `patch:`). Never use `all:` if changes are confined within a single top-level
  directory.
- **Path depth limit**: Directory depth must not exceed three levels; truncate
  at the third level if deeper (or roll up to their common parent), explaining
  specifics in BRIEF or DETAILS.
- **Multiple distinct locations**:
  - One to three distinct top-level locations: Separate with commas (e.g.,
    `Makefile, src:`, `scripts, configs:`).
  - More than three distinct top-level locations, or repository-wide changes:
    Use `all: [BRIEF]`.

### BRIEF (Summary)
- Succinctly summarize the core change of the commit.
- **Line length limit**: The entire first line (`[LOC]: [BRIEF]`) **must not
  exceed 80 columns**.

### DETAILS (Detailed Description)
- Leave one empty line after BRIEF, then provide a detailed list of changes,
  reasons, and implementation details.
- **Line length limit**: **Every line in DETAILS must not exceed 80 columns**;
  wrap lines manually when necessary.
- **Total length limit**: The total commit message must not exceed 80 lines.

---

## Technology Stack & Environment Constraints

- **Rust Toolchain & Build**: Rust codebase under `src/` (Rust 2021 edition,
  zero external crate dependencies in `Cargo.toml`, relying solely on `std`).
  Compiled binaries belong in `bin/` (e.g., `syzaster`, `check-kcov-inst`). Must
  build cleanly via `make` or `cargo build --release` and pass `make test`
  (`cargo test`).
- **Go Toolchain & Syzkaller Submodule**: Upstream `syzkaller` is tracked as a
  Git submodule in `syzkaller/`. Built via `make syzkaller` (requires Go and GNU
  Make). Never commit modified files directly inside the `syzkaller/` submodule
  directory.
- **Submodule & Patch Lifecycle**: Syzkaller adaptations reside in
  `patch/syzkaller/`, applied via `make patch-syzkaller` and reverted via
  `make unpatch-syzkaller`. Asterinas kernel patches reside in
  `patch/asterinas/<version>/`. Patches must follow numerical prefixes
  (`0001-...patch`, `0002-...patch`) without `series` files and must apply
  cleanly via `git apply`. Patch commit messages must wrap at 80 columns.
- **Framekernel KCOV Architecture & Boundary**: Asterinas divides the OS into a
  minimal, unsafe TCB (`ostd`) and a safe OS services layer (`aster_core`).
  Coverage instrumentation boundary strictly coincides with the TCB boundary:
  - Only `aster_core` is instrumented via `bin/kcov-rustc` using LLVM
    SanitizerCoverage (`-C passes=sancov-module` and trace-pc level 3) for
    target `x86_64-unknown-none`.
  - `ostd` must never be instrumented.
  - Cross-crate LTO is explicitly forbidden (`CARGO_PROFILE_DEV_LTO=false`,
    `CARGO_PROFILE_RELEASE_LTO=false`) to prevent compiler inlining across the
    TCB boundary.
  - Trace collection in `ostd::kcov` is restricted to task context
    (`InterruptLevel::current().is_task_context()`) with non-reentrant
    recording via `try_lock()`.
  - Kernel symbols are exposed via `/proc/kallsyms` (`_stext`, `__stext`,
    `_etext`) for `syz-manager` symbolization.
- **Logic Division & Scripting Standards**:
  - Scripts under `scripts/` must remain pure Shell (Bash/POSIX) for host
    automation, image creation, VM launching, and crash reproduction. Do not
    create Rust wrapper binaries for trivial shell tasks, nor shell wrappers
    around existing CLI tools.
  - Rust under `src/` is strictly reserved for type-safe logic, ELF inspection,
    KCOV validation, compiler wrapping, and statistical evaluation.
- **Configuration & Workspace Management**:
  - Configuration templates in `configs/` must retain clear `__XXX__`
    placeholders (e.g., `__SYZASTER__`, `__IMAGE__`, `__ASTERINAS__`); scripts
    must never auto-generate or overwrite `syz-manager` configs.
  - Always use `.workdir` as the standard working directory across scripts,
    configs, and documentation. Never use ad-hoc names.
  - Keep only `configs/repro/reprosh.cfg`; avoid duplicate configurations.
- **Docker Environment & Optimization**: Self-contained container environment
  in `docker/Dockerfile` provides Rust nightly, Go, LLVM, NixOS install tools,
  and QEMU. Package installations must be broken down into discrete
  `RUN apt-get install -y ...` layers rather than monolithic commands to
  maximize layer caching.

---

## Verification Workflow & Quality Standards

- **Build & Test Verification**:
  - Compile Rust tools: `make` (or `make rust-tools`).
  - Run regression tests: `make test` (or `cargo test`).
  - Validate KCOV instrumentation boundary: `./bin/check-kcov-inst`.
- **Syzkaller & Asterinas Patch Validation**:
  - Syzkaller patch verification:
    `make unpatch-syzkaller && make patch-syzkaller`.
  - Asterinas kernel update & image verification: `./scripts/update-kernel.sh`
    and `./scripts/asterinas/create-image.sh`.
- **Documentation Conventions**:
  - Avoid numbered step prefixes or numbered headings in documentation; use
    descriptive headings and bullet points instead.
  - Do not include raw ASCII directory trees in documentation.
  - Ensure documentation is self-contained.

---

## Agent Checklist Before Submitting Changes

Before concluding any task or committing changes in SyzAster, verify:
- Rust codebase compiles cleanly (`make`) and passes tests (`make test`).
- KCOV instrumentation boundary check passes (`./bin/check-kcov-inst`).
- Scripts in `scripts/` remain pure Shell without unnecessary wrapper binaries.
- No `series` files have been introduced in `patch/`.
- Patch commit messages are wrapped at 80 columns.
- Configuration templates retain `__XXX__` placeholders.
- Patches apply cleanly to their corresponding kernel target versions.
- Documentation follows formatting rules (no numbered steps, no ASCII trees).
- Commit messages strictly adhere to the `[LOC]: [BRIEF]` convention.
- Subject line and all body lines in commit messages never exceed 80 columns.
- Commits are kept locally; never push without explicit user consent.
