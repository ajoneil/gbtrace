#!/usr/bin/env bash
# Generate a trace for a screenshot test: adapter + ROM → parquet
# The adapter compares its framebuffer against the reference and stops
# when it matches. Pass/fail is determined by whether the adapter
# reported a reference match.
#
# Usage: trace-screenshot.sh <adapter-binary> <rom> <profile> <reference.pix> <output-dir> [max-frames]
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
REFERENCE="$4"
OUT_DIR="$5"
MAX_FRAMES="${6:-200}"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

TMP="/tmp/gbtrace_screenshot_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"

cleanup() { rm -f "$stderr_file"; }
trap cleanup EXIT

# Capture — adapter stops when framebuffer matches reference
"$BIN" --rom "$ROM" --profile "$PROFILE" --output "${TMP}.parquet" \
    --reference "$REFERENCE" \
    --frames "$MAX_FRAMES" \
    2>"$stderr_file" || true

if [[ ! -s "${TMP}.parquet" ]]; then
    printf "%-30s %-10s ERROR (capture)\n" "$NAME" "$ADAPTER"
    exit 1
fi

# Pass/fail: check if the adapter reported a reference match
if grep -q "Reference match" "$stderr_file"; then
    status="pass"
    printf "%-30s %-10s PASS\n" "$NAME" "$ADAPTER"
else
    status="fail"
    printf "%-30s %-10s FAIL\n" "$NAME" "$ADAPTER"
fi

mkdir -p "$OUT_DIR"
mv "${TMP}.parquet" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
