#!/usr/bin/env bash
# Generate a single gbmicrotest trace: adapter + ROM → .gbtrace
# Usage: trace-gbmicrotest.sh <adapter-binary> <rom> <profile> <output-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
CLI="${CLI:-target/release/gbtrace}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"
FRAMES=30

TMP="/tmp/gbtrace_micro_${NAME}_${ADAPTER}_$$"
TRACE="${TMP}.gbtrace"
stderr_file="${TMP}.stderr"

cleanup() { rm -f "$TRACE" "$stderr_file" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# Capture — adapter stops when test_pass is set
if ! "$BIN" --rom "$ROM" --profile "$PROFILE" --output "$TRACE" \
    --frames "$FRAMES" \
    --stop-when FF82=01 --stop-when FF82=FF >/dev/null 2>"$stderr_file" </dev/null; then
    printf "%-40s %-10s ERROR\n" "$NAME" "$ADAPTER"
    exit 1
fi

if [[ ! -s "$TRACE" ]]; then
    printf "%-40s %-10s ERROR (empty)\n" "$NAME" "$ADAPTER"
    exit 1
fi

# Determine pass/fail from the trace data
status="fail"
match_count=$("$CLI" query "$TRACE" -w "test_pass=01" --max 1 2>&1 | grep -oP '^\d+(?= match)' || echo "0")
if [ "$match_count" -gt 0 ]; then
    status="pass"
fi

# Move to output
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$TRACE" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
