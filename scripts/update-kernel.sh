#!/usr/bin/env bash

# Clone or checkout target Asterinas source tree and apply SyzAster patches.
#
# usage:
#   ./scripts/update-kernel.sh -d <KERNEL_DIR> [-v <VERSION>]

set -euo pipefail

SYZASTER=$(realpath "$(dirname "$0")/..")
ASTERINAS_REPO_URL="https://github.com/asterinas/asterinas.git"
ASTERINAS_VERSION="v0.18.1"
ASTERINAS_COMMIT=""

KERNDIR=
APPLY_PATCHES=1

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (required):"
    echo "    -d, --dir <DIR>         path to directory storing Asterinas source"
    echo "  args (optional):"
    echo "    -v, --version <VER>     Asterinas version (v0.18.1 or v0.19.0;"
    echo "                            default: v0.18.1)"
    echo "    --no-patch              do not apply SyzAster KCOV patches"
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
    -v | --version)
        if [[ $# -lt 2 || -z "$2" ]]; then
            echo "Error: Missing version for '$1'."
            print_help
            exit 1
        fi
        ASTERINAS_VERSION="$2"
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

if [[ "$ASTERINAS_VERSION" != v* ]]; then
    ASTERINAS_VERSION="v$ASTERINAS_VERSION"
fi

case "$ASTERINAS_VERSION" in
    v0.18.1)
        ASTERINAS_COMMIT="d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5"
        ;;
    v0.19.0)
        ASTERINAS_COMMIT="c4b7ed575b7b32773475b194d2ad41fb6d03765b"
        ;;
    *)
        echo "Error: Unsupported Asterinas version '$ASTERINAS_VERSION'."
        echo "Supported versions: v0.18.1, v0.19.0"
        exit 1
        ;;
esac

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
