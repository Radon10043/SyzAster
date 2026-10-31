# SyzAster Experiment & Evaluation Suite

This directory contains all instructions, configurations, Docker Compose orchestration files, and statistical evaluation tooling for building KCOV-instrumented Asterinas kernels and evaluating **SyzAster**.

## Directory Structure

- [`docs/setup-env.md`](docs/setup-env.md): End-to-end experiment environment preparation.
- [`docs/setup-kernel_asterinas.md`](docs/setup-kernel_asterinas.md): Building Asterinas `v0.18.1` with `KCOV=1` and `KCOV=0`.
- [`docs/setup-image_asterinas.md`](docs/setup-image_asterinas.md): Generating Asterinas NixOS VM images and validating KCOV ABI.
- [`docs/setup-fuzzer_SyzAster.md`](docs/setup-fuzzer_SyzAster.md): Building SyzAster Rust tools and patched `syzkaller`.
- [`docs/setup-env_file.md`](docs/setup-env_file.md): Configuring `compose.env` for container-based evaluation.
- [`docs/docker-compose.md`](docs/docker-compose.md): Launching parallel fuzzing campaigns via Docker Compose.
- [`docker-compose/`](docker-compose/): Docker Compose files for `SyzAster` (`KCOV=1`) and `SyzAster-nocover` (`KCOV=0` baseline).
- [`configs/asterinas/`](configs/asterinas/): Subsystem-targeted fuzzing configurations (`fs`, `net`, `process`, `mm`).
- [`viz/fuzz/`](viz/fuzz/): Statistical evaluation runner powered by the Rust `bin/eval-fuzz` tool (Mann-Whitney U test p-value & Vargha-Delaney $\hat{A}_{12}$ effect size).
