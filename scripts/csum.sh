#!/usr/bin/env bash

# Summarize the titles of crashes from a directory of syzkaller crash logs into a CSV file.
# If the CSV file already exists, existing status annotations are preserved.
#
# usage:
#   bash scripts/csum.sh -d <CRASH_DIR> -o <OUTPUT_CSV>

set -euo pipefail

CRASH_DIR=
OUTPUT_CSV=

print_help() {
    echo "usage: $0 [ARGS]"
    echo "  args (required):"
    echo "    -d, --dir <DIR>        path to the directory containing syzkaller crash logs"
    echo "    -o, --output <OUTPUT>  path to the output csv file"
    echo "  args (optional):"
    echo "    -h, --help             print help message"
}

while [[ $# -gt 0 ]]; do
    case $1 in
    -d | --dir)
        if [[ ! -d "$2" ]]; then
            echo "Error: Directory '$2' does not exist."
            exit 1
        fi
        CRASH_DIR=$(realpath "$2")
        shift 2
        ;;
    -o | --output)
        if command -v realpath >/dev/null 2>&1; then
            OUTPUT_CSV=$(realpath -m "$2")
        else
            OUTPUT_CSV="$2"
        fi
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

if [[ -z "$CRASH_DIR" ]] || [[ -z "$OUTPUT_CSV" ]]; then
    echo "Error: Missing required arguments."
    print_help
    exit 1
fi

declare -A EXISTING_TITLES
if [[ -f "$OUTPUT_CSV" ]]; then
    while IFS=, read -r status title; do
        clean_title="${title%\"}"
        clean_title="${clean_title#\"}"
        EXISTING_TITLES["$clean_title"]=$status
    done < <(tail -n +2 "$OUTPUT_CSV")
fi

TEMP_CSV="${OUTPUT_CSV}.tmp"

{
    echo "status,title"
    find "$CRASH_DIR" -name description | while read -r desc; do
        if [[ -f "$desc" ]]; then
            title=$(head -n 1 "$desc")
            if [[ -n "${EXISTING_TITLES[$title]:-}" ]]; then
                status=${EXISTING_TITLES[$title]}
            else
                status="unknown"
            fi
            echo "$status,\"$title\""
        fi
    done | sort
} > "$TEMP_CSV"

mv "$TEMP_CSV" "$OUTPUT_CSV"
echo "Done. Summary saved to $OUTPUT_CSV"
