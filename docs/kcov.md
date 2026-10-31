# Framekernel-Aware KCOV Architecture & Verification

Asterinas adopts a **framekernel** architecture that separates the OS into a minimal, unsafe Trusted Computing Base (**OSTD**) and a safe OS services layer (**`aster_core`**) written in safe Rust. **SyzAster** designs its coverage feedback mechanism around this boundary.

## Selective `aster_core` SanitizerCoverage Instrumentation

Instrumenting the low-level runtime (`ostd`)—which implements context switching, page tables, interrupt handling, spinlocks, and the KCOV ring-buffer recorder itself—would cause infinite recursion or deadlocks inside `__sanitizer_cov_trace_pc`.

SyzAster solves this via `kcov-rustc` (`src/pkg/kcov.rs` / `tools/kcov-rustc.sh`) and `Makefile` switches (`KCOV=1`):
- Inspects `rustc` invocation arguments for `--crate-name aster_core` and `--target x86_64-unknown-none`.
- Appends `-C passes=sancov-module -C llvm-args=-sanitizer-coverage-level=3 -C llvm-args=-sanitizer-coverage-trace-pc` **only** to `aster_core`.
- Disables cross-crate LTO (`CARGO_PROFILE_DEV_LTO=false`, `CARGO_PROFILE_RELEASE_LTO=false`) so LLVM cannot inline instrumented functions into uninstrumented OSTD routines or vice versa.

## Task-Scoped Coverage Recording in `ostd::kcov`

- Each `Task` in `ostd` holds a `SpinLock<Option<kcov::Attachment>>`.
- `__sanitizer_cov_trace_pc` is a `#[unsafe(naked)]` function in `ostd::kcov` that reads the caller return address from `[rsp]` into `rdi` and tail-jumps to `record_pc`.
- `record_pc` verifies that:
  - `IN_BOOTSTRAP_CONTEXT` is false.
  - `InterruptLevel::current().is_task_context()` is true (ignoring hardware interrupts).
  - `task.kcov.try_lock()` succeeds (preventing any re-entrancy deadlock).
- The PC is written into the shared `Vmo` page array using `VmIoOnce::write_once`, with strict bounds checking against hostile userspace modifications of the shared count word `area[0]`.

## `/dev/kcov` Character Device & `/proc/kallsyms`

- `kernel/core/src/device/misc/kcov.rs` registers `/dev/kcov` supporting:
  - `KCOV_INIT_TRACE`: allocates an anonymous `Vmo`, eagerly commits pages, and initializes `KcovBuffer`.
  - `KCOV_ENABLE` / `KCOV_DISABLE`: attaches/detaches the buffer to the current `Task`.
  - `mmap`: validates a `MAP_SHARED` mapping at offset 0 of the exact buffer size and maps the `Vmo`.
- `kernel/core/src/fs/fs_impls/procfs/kallsyms.rs` exposes `/proc/kallsyms` with `_stext` and `_etext` from the linker script (`osdk/src/base_crate/x86_64.ld.template`).
- In `syzkaller` (`patch/syzkaller/0001-*.patch`), `open_kcov()` tries `/sys/kernel/debug/kcov` first and falls back to `/dev/kcov` on `ENOENT`.
