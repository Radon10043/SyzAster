#!/usr/bin/env bash

# Clone or checkout the target Asterinas v0.18.1 source tree and apply SyzAster patches.
#
# usage:
#   ./scripts/update-kernel.sh -d <KERNEL_DIR>

set -euo pipefail

SYZASTER=$(realpath "$(dirname "$0")/..")
ASTERINAS_REPO_URL="https://github.com/asterinas/asterinas.git"
ASTERINAS_VERSION="v0.18.1"
ASTERINAS_COMMIT="d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5"

KERNDIR=
APPLY_PATCHES=1

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (required):"
    echo "    -d, --dir <DIR>         path to the directory to store Asterinas kernel source"
    echo "  args (optional):"
    echo "    --no-patch              do not apply SyzAster compatibility and KCOV patches"
    echo "    -h, --help              print help message"
}

while [[ $# -gt 0 ]]; do
    case $1 in
    -d | --dir)
        if [[ $# -lt 2 || -z "$2" ]]; then
            echo "Error: Missing directory for '$1'."
            print_help
            exit 1
        fi
        if [[ ! -d "$2" ]]; then
            echo "Error: Directory '$2' does not exist."
            exit 1
        fi
        KERNDIR=$(realpath "$2")
        shift 2
        ;;
    --no-patch)
        APPLY_PATCHES=0
        shift
        ;;
    -h | --help)
        print_help
        exit 0
        ;;
    *)
        echo "unknown arg: $1"
        exit 1
        ;;
    esac
done

if [[ -z "$KERNDIR" ]]; then
    print_help
    exit 1
fi

DEST="$KERNDIR/asterinas-$ASTERINAS_VERSION"
if [[ ! -d "$DEST" ]]; then
    echo "Cloning Asterinas ($ASTERINAS_VERSION) into $DEST ..."
    git clone "$ASTERINAS_REPO_URL" "$DEST"
fi

echo "Checking out $ASTERINAS_VERSION ($ASTERINAS_COMMIT) ..."
git -C "$DEST" checkout --detach "$ASTERINAS_COMMIT"

if [[ "$APPLY_PATCHES" -eq 1 ]]; then
    for patch_file in "$SYZASTER"/patch/asterinas/"$ASTERINAS_VERSION"/*.patch; do
        if ! git -C "$DEST" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
            echo "Applying $(basename "$patch_file") ..."
            git -C "$DEST" apply "$patch_file"
        else
            echo "Already applied: $(basename "$patch_file")"
        fi
    done
fi

echo "Done! Asterinas source ready at $DEST"
