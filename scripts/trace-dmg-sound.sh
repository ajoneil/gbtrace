#!/usr/bin/env bash
# Generate a single dmg_sound trace: adapter + ROM → .gbtrace
#
# Pass/fail: test writes result to $A000 (0x00 = pass, 0x80 = still running)
#
# Usage: trace-dmg-sound.sh <adapter-binary> <rom> <profile> <output-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# dmg_sound tests take longer than microtests
MAX_FRAMES=3000

TMP="/tmp/gbtrace_dmg_sound_${NAME}_${ADAPTER}_$$"
TRACE="${TMP}.gbtrace"
stderr_file="${TMP}.stderr"

cleanup() { rm -f "$TRACE" "$stderr_file" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# Capture — stop when test_status changes from 0x80 (running) to a result
(
    set +eo pipefail
    "$BIN" --rom "$ROM" --profile "$PROFILE" --output "$TRACE" \
        --frames "$MAX_FRAMES" \
        --stop-when A000=00 \
        2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$TRACE" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# Determine pass/fail from the trace data
status="fail"
match_count=$("$CLI" query "$TRACE" -w "test_status=00" --max 1 2>&1 | grep -oP '^\d+(?= match)' || echo "0")
if [ "$match_count" -gt 0 ]; then
    status="pass"
fi

# Move to output
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$TRACE" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
