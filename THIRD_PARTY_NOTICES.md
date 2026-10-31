# Licenses and Third-Party Notices

SyzAster combines original code, original documentation, and patches for two upstream open-source projects. Copyright (c) 2026 RustTrace Team, University of Science and Technology Beijing, unless stated otherwise. Each part is distributed under the license listed below.

## License Map

| Path | License | License text |
|---|---|---|
| Source code not listed below, including `src/`, `tests/`, `scripts/`, `configs/`, `docker/`, and the configurations, orchestration files, and scripts in `experiment/` | MIT | [`LICENSE`](LICENSE) |
| Documentation: `report/` (technical report), `docs/`, `experiment/docs/`, and all Markdown files (`*.md`) such as `README.md` and `CONTRIBUTING.md` | CC BY-SA 4.0 | [`LICENSE-CC-BY-SA`](LICENSE-CC-BY-SA) |
| `patch/asterinas/` | MPL-2.0, following Asterinas | [`LICENSE-MPL`](LICENSE-MPL) |
| `patch/syzkaller/` | Apache-2.0, following syzkaller | [`LICENSE-APACHE`](LICENSE-APACHE) |
| `syzkaller/` (Git submodule) | Apache-2.0, unmodified upstream source | [`syzkaller/LICENSE`](https://github.com/google/syzkaller/blob/master/LICENSE) |

## Third-Party Sources

### syzkaller

- **Source**: <https://github.com/google/syzkaller>, included as the `syzkaller/` Git submodule.
- **License**: Apache License 2.0.
- **SyzAster changes**: the patch series in `patch/syzkaller/` modifies the executor, coverage symbolization, VM information collection, crash report parsing, manager configuration, and VM lifecycle management. New files added by these patches carry the syzkaller copyright header and are distributed under Apache-2.0.

### Asterinas

- **Source**: <https://github.com/asterinas/asterinas>, tag `v0.18.1` (`d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5`). The Asterinas source is not vendored in this repository; `scripts/update-kernel.sh` downloads it.
- **License**: Mozilla Public License 2.0.
- **SyzAster changes**: the patches in `patch/asterinas/v0.18.1/` add the task-scoped KCOV runtime, the `/dev/kcov` character device, `/proc/kallsyms`, the `KCOV=1` build switch, NixOS image installation fixes, and supporting tools. New files added by these patches carry `SPDX-License-Identifier: MPL-2.0`.

### Build-Time Downloads

The Docker image (`docker/Dockerfile`) and the setup scripts download additional tools at build time, including the Rust toolchain, Go, LLVM, FlatBuffers, Nix and nixpkgs, the Asterinas `linux_vdso` library, MiKTeX with the ElegantPaper class, and draw.io. These tools are not redistributed in this repository and remain under their respective licenses.
