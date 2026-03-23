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

# Capture trace
if ! "$BIN" --rom "$ROM" --profile "$PROFILE" --output "${TMP}.gbtrace" \
    --frames "$MAX_FRAMES" \
    2>/dev/null; then
    printf "%-30s %-10s ERROR (capture)\n" "$NAME" "$ADAPTER"
    rm -f "${TMP}.gbtrace"
    exit 1
fi

# Convert to parquet
"$CLI" convert "${TMP}.gbtrace" --output "${TMP}.parquet" >/dev/null 2>&1

# Trim to the frame matching the reference
if "$CLI" trim "${TMP}.parquet" --reference "$REFERENCE" \
    --output "${TMP}_trimmed.parquet" >/dev/null 2>&1; then
    # Trim succeeded — reference matched
    mv "${TMP}_trimmed.parquet" "${TMP}.parquet"

    # Render frames to find which one matched (for the status message)
    mkdir -p "${TMP}_frames"
    "$CLI" render "${TMP}.parquet" --output "${TMP}_frames/" >/dev/null 2>&1
    TOTAL_FRAMES=$(ls "${TMP}_frames/"*.png 2>/dev/null | wc -l)

    status="pass"
    printf "%-30s %-10s PASS  (frame %s)\n" "$NAME" "$ADAPTER" "$TOTAL_FRAMES"

    # Save the last rendered frame
    LAST=$(ls "${TMP}_frames/"*.png 2>/dev/null | tail -1)
    if [ -n "$LAST" ]; then
        mkdir -p "$OUT_DIR"
        cp "$LAST" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
    fi
else
    # Trim failed — no matching frame
    status="fail"

    # Render frames for debugging
    mkdir -p "${TMP}_frames"
    "$CLI" render "${TMP}.parquet" --output "${TMP}_frames/" >/dev/null 2>&1
    TOTAL_FRAMES=$(ls "${TMP}_frames/"*.png 2>/dev/null | wc -l)

    printf "%-30s %-10s FAIL  (%s frames, no match)\n" "$NAME" "$ADAPTER" "$TOTAL_FRAMES"

    # Save last frame for debugging
    LAST=$(ls "${TMP}_frames/"*.png 2>/dev/null | tail -1)
    if [ -n "$LAST" ]; then
        mkdir -p "$OUT_DIR"
        cp "$LAST" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
    fi
fi

# Move parquet to final location
mkdir -p "$OUT_DIR"
mv "${TMP}.parquet" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"

rm -f "${TMP}.gbtrace" "${TMP}_trimmed.parquet"
rm -rf "${TMP}_frames"
