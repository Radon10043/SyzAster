#!/usr/bin/env bash

# Clone or checkout target Asterinas source tree and apply SyzAster patches.
#
# usage:
#   ./scripts/update-kernel.sh -d <KERNEL_DIR> [-v <VERSION>]
#   ./scripts/update-kernel.sh -o <SOURCE_DIR> [-v <VERSION>]

set -euo pipefail

SYZASTER=$(realpath "$(dirname "$0")/..")
ASTERINAS_REPO_URL="https://github.com/asterinas/asterinas.git"
ASTERINAS_VERSION="v0.18.1"
ASTERINAS_COMMIT=""

KERNDIR=
DEST=
APPLY_PATCHES=1

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (one of them is required):"
    echo "    -d, --dir <DIR>         path to directory storing Asterinas source;"
    echo "                            the tree is placed at <DIR>/asterinas-<VER>"
    echo "    -o, --dest <DIR>        exact path of the Asterinas source tree"
    echo "  args (optional):"
    echo "    -v, --version <VER>     Asterinas version (v0.17.1, v0.17.2, v0.18.0,"
    echo "                            v0.18.1, or v0.19.0; default: v0.18.1)"
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
    -o | --dest)
        if [[ $# -lt 2 || -z "$2" ]]; then
            echo "Error: Missing directory for '$1'."
            print_help
            exit 1
        fi
        DEST=$(realpath -m "$2")
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

if [[ -z "$KERNDIR" && -z "$DEST" ]]; then
    print_help
    exit 1
fi

if [[ "$ASTERINAS_VERSION" != v* ]]; then
    ASTERINAS_VERSION="v$ASTERINAS_VERSION"
fi

case "$ASTERINAS_VERSION" in
    v0.17.1)
        ASTERINAS_COMMIT="9ea44ed2b60bc81a5efb18af79e41fc07bf3d523"
        ;;
    v0.17.2)
        ASTERINAS_COMMIT="23adfdfd72b05cee8d232809caea81a4b33d3488"
        ;;
    v0.18.0)
        ASTERINAS_COMMIT="253be4750d69810af7b7b020fe2fee40a8547e15"
        ;;
    v0.18.1)
        ASTERINAS_COMMIT="d924a9635a66c7c3bb43e563eaafa2c61d6ee9d5"
        ;;
    v0.19.0)
        ASTERINAS_COMMIT="c4b7ed575b7b32773475b194d2ad41fb6d03765b"
        ;;
    *)
        echo "Error: Unsupported Asterinas version '$ASTERINAS_VERSION'."
        echo "Supported versions: v0.17.1, v0.17.2, v0.18.0, v0.18.1, v0.19.0"
        exit 1
        ;;
esac

if [[ -z "$DEST" ]]; then
    DEST="$KERNDIR/asterinas-$ASTERINAS_VERSION"
fi
mkdir -p "$(dirname "$DEST")"
if [[ ! -d "$DEST" ]]; then
    echo "Cloning Asterinas ($ASTERINAS_VERSION) into $DEST ..."
    git clone "$ASTERINAS_REPO_URL" "$DEST"
fi

if ! git -C "$DEST" cat-file -e "${ASTERINAS_COMMIT}^{commit}" 2>/dev/null; then
    git -C "$DEST" fetch --depth 1 origin "$ASTERINAS_COMMIT" \
        || git -C "$DEST" fetch origin "refs/tags/${ASTERINAS_VERSION}:refs/tags/${ASTERINAS_VERSION}"
fi

echo "Checking out $ASTERINAS_VERSION ($ASTERINAS_COMMIT) ..."
CUR_HEAD="$(git -C "$DEST" rev-parse HEAD 2>/dev/null || true)"
if [[ "$CUR_HEAD" != "$ASTERINAS_COMMIT" ]]; then
    for old_patch in $(ls -r "$SYZASTER"/patch/asterinas/*/*.patch 2>/dev/null); do
        if git -C "$DEST" apply --reverse --check "$old_patch" >/dev/null 2>&1; then
            git -C "$DEST" apply --reverse --whitespace=nowarn "$old_patch"
        fi
    done
    git -C "$DEST" checkout -f --detach "$ASTERINAS_COMMIT"
else
    git -C "$DEST" checkout --detach "$ASTERINAS_COMMIT"
fi

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
