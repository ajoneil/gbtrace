#!/usr/bin/env bash
# Generate a trace for a screenshot test: adapter + ROM → parquet + rendered frame
# Captures a small number of frames, then uses the CLI to trim the trace to the
# frame matching the reference and verify the match.
#
# Usage: trace-screenshot.sh <adapter-binary> <rom> <profile> <reference.pix> <output-dir> [max-frames]
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
REFERENCE="$4"
OUT_DIR="$5"
MAX_FRAMES="${6:-5}"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

TMP="/tmp/gbtrace_screenshot_${NAME}_${ADAPTER}_$$"
tmp_pipe="${TMP}.pipe"

cleanup() { rm -f "$tmp_pipe" "${TMP}_trimmed.parquet"; rm -rf "${TMP}_frames"; }
trap cleanup EXIT

# Capture + convert via named pipe
mkfifo "$tmp_pipe"

"$BIN" --rom "$ROM" --profile "$PROFILE" --output "$tmp_pipe" \
    --reference "$REFERENCE" \
    --frames "$MAX_FRAMES" \
    2>/dev/null &
adapter_pid=$!

"$CLI" convert "$tmp_pipe" --output "${TMP}.parquet" >/dev/null 2>&1 || true

wait "$adapter_pid" || true

if [[ ! -s "${TMP}.parquet" ]]; then
    printf "%-30s %-10s ERROR (capture)\n" "$NAME" "$ADAPTER"
    exit 1
fi

# Trim to the frame matching the reference
if "$CLI" trim "${TMP}.parquet" --reference "$REFERENCE" \
    --output "${TMP}_trimmed.parquet" >/dev/null 2>&1; then
    mv "${TMP}_trimmed.parquet" "${TMP}.parquet"

    mkdir -p "${TMP}_frames"
    "$CLI" render "${TMP}.parquet" --output "${TMP}_frames/" >/dev/null 2>&1
    TOTAL_FRAMES=$(ls "${TMP}_frames/"*.png 2>/dev/null | wc -l)

    status="pass"
    printf "%-30s %-10s PASS  (frame %s)\n" "$NAME" "$ADAPTER" "$TOTAL_FRAMES"

    LAST=$(ls "${TMP}_frames/"*.png 2>/dev/null | tail -1)
    if [ -n "$LAST" ]; then
        mkdir -p "$OUT_DIR"
        cp "$LAST" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
    fi
else
    status="fail"

    mkdir -p "${TMP}_frames"
    "$CLI" render "${TMP}.parquet" --output "${TMP}_frames/" >/dev/null 2>&1
    TOTAL_FRAMES=$(ls "${TMP}_frames/"*.png 2>/dev/null | wc -l)

    printf "%-30s %-10s FAIL  (%s frames, no match)\n" "$NAME" "$ADAPTER" "$TOTAL_FRAMES"

    LAST=$(ls "${TMP}_frames/"*.png 2>/dev/null | tail -1)
    if [ -n "$LAST" ]; then
        mkdir -p "$OUT_DIR"
        cp "$LAST" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
    fi
fi

mkdir -p "$OUT_DIR"
mv "${TMP}.parquet" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
