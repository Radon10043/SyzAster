#!/usr/bin/env bash
# Wrapper around the Rust `eval-fuzz` binary.
set -euo pipefail
ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)
if [[ ! -x "${ROOT_DIR}/bin/eval-fuzz" ]]; then
    make -C "${ROOT_DIR}" rust-tools >/dev/null
fi
exec "${ROOT_DIR}/bin/eval-fuzz" "$@"
