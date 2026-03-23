#!/usr/bin/env bash
# Generate a trace for a screenshot test: adapter + ROM → parquet + rendered frame
# Stops when rendered frame matches reference, or after max frames.
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

# Render all frames and find the one matching the reference
mkdir -p "${TMP}_frames"
"$CLI" render "${TMP}.parquet" --output "${TMP}_frames/" >/dev/null 2>&1

# Check each rendered frame's pixels against reference
MATCH_FRAME=""
TOTAL_FRAMES=0
for png in "${TMP}_frames/"*.png; do
    TOTAL_FRAMES=$((TOTAL_FRAMES + 1))
    # Convert frame to pix format and compare
    FRAME_PIX="${png%.png}.pix"
    python3 scripts/png-to-pix.py "$png" "$FRAME_PIX" >/dev/null 2>&1
    if diff -q "$FRAME_PIX" "$REFERENCE" >/dev/null 2>&1; then
        MATCH_FRAME="$TOTAL_FRAMES"
        break
    fi
done

if [ -n "$MATCH_FRAME" ]; then
    status="pass"
    printf "%-30s %-10s PASS  (frame %s/%s)\n" "$NAME" "$ADAPTER" "$MATCH_FRAME" "$TOTAL_FRAMES"
else
    status="fail"
    printf "%-30s %-10s FAIL  (%s frames, no match)\n" "$NAME" "$ADAPTER" "$TOTAL_FRAMES"
fi

# Move parquet to final location
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
mv "${TMP}.parquet" "$out"

# Also save the last rendered frame for visual inspection
if [ -n "$MATCH_FRAME" ]; then
    cp "${TMP}_frames/${NAME}_frame$(printf '%03d' "$MATCH_FRAME").png" \
       "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
else
    # Save the last frame for debugging
    ls "${TMP}_frames/"*.png 2>/dev/null | tail -1 | while read f; do
        cp "$f" "${OUT_DIR}/${NAME}_${ADAPTER}_${status}.png" 2>/dev/null || true
    done
fi

rm -f "${TMP}.gbtrace" "${TMP}.parquet"
rm -rf "${TMP}_frames"
