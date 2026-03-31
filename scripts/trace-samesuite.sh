#!/usr/bin/env bash
# Generate a single SameSuite trace: adapter + ROM → .gbtrace
#
# Pass/fail detection:
#   SameSuite tests enter an infinite loop when complete.
#   Pass: B=3 C=5 D=8 E=13 H=21 L=34 (Fibonacci sequence)
#   Fail: all registers = 0x42, or non-Fibonacci values
#
# Usage: trace-samesuite.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
ROM_DIR="${5:-$(dirname "$ROM")}"
CLI="${CLI:-target/release/gbtrace}"

ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# Use relative path from ROM_DIR as the test name, flattening subdirs with __
ROM_REL="$(realpath --relative-to="$ROM_DIR" "$ROM")"
ROM_REL="${ROM_REL%.gb}"
NAME="${ROM_REL//\//__}"

MAX_FRAMES=7200
TMP="/tmp/gbtrace_samesuite_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"
tmp_trace="${TMP}.gbtrace"

cleanup() { rm -f "$stderr_file" "$tmp_trace" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# --- Capture ---
(
    set +eo pipefail
    timeout 120 "$BIN" --rom "$ROM" --profile "$PROFILE" \
        --frames "$MAX_FRAMES" \
        --output "$tmp_trace" >/dev/null 2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$tmp_trace" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-60s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# --- Determine pass/fail ---
# Check registers from the last entry: Fibonacci sequence = pass
status=$("$CLI" query "$tmp_trace" --last 1 2>&1 | \
    grep -qP 'b=03\b.*c=05\b.*d=08\b.*e=0d\b.*h=15\b.*l=22\b' \
    && echo "pass" || echo "fail")

# --- Output ---
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$tmp_trace" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-60s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
