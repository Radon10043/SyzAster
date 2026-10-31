# Contributing to SyzAster

Thank you for your interest in SyzAster. This guide explains how to set up a development environment, the conventions the project follows, and how to submit changes.

## Getting Started

- Build the Docker image described in [README.md](README.md#build-the-docker-image) and work inside the container. It provides the pinned Rust nightly toolchain, Go, LLVM, Nix, and QEMU required by every component.
- Clone the repository together with the `syzkaller` submodule:
  ```bash
  git clone --recurse-submodules https://github.com/Radon10043/SyzAster
  ```
- Build the Rust tools and run the regression tests before making any change, so that you start from a known-good state:
  ```bash
  make
  make test
  ```

## Repository Layout and Responsibilities

- `src/` holds the Rust library (`src/pkg/`) and command-line entry points (`src/bin/`). Compiled tools are staged into `bin/`.
- `patch/syzkaller/` holds the syzkaller patch series, and `patch/asterinas/v0.18.1/` holds the Asterinas kernel patches.
- `scripts/` holds pure Shell automation for kernel patching, image creation, VM launching, and crash triage.
- `configs/` holds configuration templates, `docs/` holds design references, `tests/` holds integration tests, and `experiment/` holds the evaluation harness.

Keep the division of responsibility between Shell and Rust:

- Use pure Shell (Bash/POSIX) for automation in `scripts/`. Do not add Rust wrapper binaries for trivial shell tasks.
- Use Rust for logic that needs type safety, ELF inspection, KCOV validation, compiler wrapping, or statistical computation.

## Engineering Conventions

- **Framekernel boundary**: only `aster_core` may be instrumented. `ostd` must never be instrumented, and cross-crate LTO must stay disabled in coverage builds.
- **Target kernel**: kernel patches live exclusively in `patch/asterinas/v0.18.1/`. Do not split code, configs, or patches into parallel version branches.
- **Patches**: order patches by numerical prefix (`0001-...patch`, `0002-...patch`) without `series` files. Every patch must apply with `git apply`, and commit messages inside patch files must wrap at 80 columns.
- **Configurations**: keep the `__XXX__` placeholders in `configs/` templates, and do not let scripts generate or overwrite `syz-manager` configs automatically.
- **Working directory**: use `.workdir` as the working directory name in scripts, configs, and docs.
- **Dependencies**: the Rust crate depends only on the standard library. Discuss any new dependency in an issue first.
- **Documentation**: do not use numbered lists or numbered step headings, and do not add raw ASCII directory trees to `README.md`.

## Making Changes

- Create a topic branch from `main` for each change.
- Add or update tests in `tests/` (Rust tools and scripts) or in the corresponding syzkaller/Asterinas test files carried by the patches.
- Run the checks below before submitting:
  ```bash
  cargo test
  ./bin/check-kcov-inst
  ```
- When you modify a syzkaller patch, re-apply the whole series with `make unpatch-syzkaller && make patch-syzkaller` and run the affected Go package tests inside `syzkaller/`.
- When you modify an Asterinas patch, verify that `git apply` succeeds on a clean Asterinas checkout and rebuild the image with `scripts/asterinas/create-image.sh`.

## Commit Messages

- Use the module-prefixed format `<module>: <concise description>`, for example `scripts: add helper scripts for Asterinas` or `patch/syzkaller: fix panic title parsing`.
- Keep the subject and every body line within 80 characters, and the whole message within 80 lines.
- Separate unrelated changes into distinct, topic-focused commits.

## Submitting Changes

- Open a pull request against `main` describing the motivation, the change, and how it was tested.
- Make sure all checks above pass and that the pull request contains only the commits related to its topic.
- Report bugs and feature requests through GitHub issues. For kernel crashes found by SyzAster, attach the crash title, the reproducer (if any), and the kernel commit.

## License

By contributing, you agree that your contributions are licensed under the license of the part you modify, as listed in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md): original SyzAster code is licensed under MIT, documentation (Markdown files, `docs/`, `experiment/docs/`, and `report/`) is licensed under CC BY-SA 4.0, patches in `patch/asterinas/` follow the MPL-2.0 license of Asterinas, and patches in `patch/syzkaller/` follow the Apache-2.0 license of syzkaller.
