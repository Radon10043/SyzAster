#!/usr/bin/env bash

# Find logs related to specific crash titles from a directory of syzkaller crash logs.
#
# usage:
#   bash scripts/logs.sh -d <CRASH_DIR> -i <INTEREST_CRASH>

set -euo pipefail

CRASH_DIR=
INTEREST_CRASH=

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (required):"
    echo "    -d, --dir <DIR>            path to the directory containing syzkaller crash logs"
    echo "    -i, --interest <INTEREST>  path to the file containing crash titles of interest (one per line)"
    echo "  args (optional):"
    echo "    -h, --help                 print help message"
}

while [[ $# -gt 0 ]]; do
    case $1 in
    -d | --dir)
        CRASH_DIR=$(realpath "$2")
        shift 2
        ;;
    -i | --interest)
        INTEREST_CRASH=$(realpath "$2")
        shift 2
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

if [[ -z "$CRASH_DIR" ]] || [[ -z "$INTEREST_CRASH" ]]; then
    echo "Error: Missing required arguments."
    print_help
    exit 1
fi

logs=()
while IFS= read -r title; do
    [[ -z "$title" ]] && continue
    while IFS= read -r desc; do
        [[ -z "$desc" ]] && continue
        while IFS= read -r log_file; do
            [[ -n "$log_file" ]] && logs+=("$log_file")
        done < <(find "$(dirname "$desc")" -maxdepth 1 -name "log*")
    done < <(find "$CRASH_DIR" -name description -exec grep -Fl "$title" {} + || true)
done < "$INTEREST_CRASH"

if [[ ${#logs[@]} -gt 0 ]]; then
    printf "%s\n" "${logs[@]}" | sort -u
fi
