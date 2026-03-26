#!/usr/bin/env bash
# Generate a single mooneye trace: adapter + ROM → .gbtrace
#
# Pass/fail detection:
#   Mooneye tests execute LD B, B (opcode 0x40) when complete.
#   Pass: B=3 C=5 D=8 E=13 H=21 L=34 (Fibonacci sequence)
#   Fail: all registers = 0x42
#
# Usage: trace-mooneye.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
ROM_DIR="${5:-$(dirname "$ROM")}"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# Compute relative subdir from ROM_DIR to ROM
ROM_REL="$(realpath --relative-to="$ROM_DIR" "$(dirname "$ROM")")"
if [ "$ROM_REL" = "." ]; then
    TRACE_SUBDIR="$OUT_DIR"
else
    TRACE_SUBDIR="$OUT_DIR/$ROM_REL"
fi

# Check for .pix reference next to the ROM
PIX_REF="$(dirname "$ROM")/${NAME}.pix"

MAX_FRAMES=200
TMP="/tmp/gbtrace_mooneye_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"
tmp_trace="${TMP}.gbtrace"

cleanup() { rm -f "$stderr_file" "$tmp_trace" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# --- Capture ---
EXTRA_ARGS=()
if [[ -f "$PIX_REF" ]]; then
    EXTRA_ARGS+=(--reference "$PIX_REF")
fi

(
    set +eo pipefail
    "$BIN" --rom "$ROM" --profile "$PROFILE" \
        --stop-opcode 40 --extra-frames 2 --frames "$MAX_FRAMES" \
        "${EXTRA_ARGS[@]}" \
        --output "$tmp_trace" 2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$tmp_trace" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# --- Determine pass/fail ---
# Check registers from the last entry: Fibonacci sequence = pass
status=$("$CLI" query "$tmp_trace" --last 1 2>&1 | \
    grep -qP 'b=03\b.*c=05\b.*d=08\b.*e=0d\b.*h=15\b.*l=22\b' \
    && echo "pass" || echo "fail")

# --- Output ---
mkdir -p "$TRACE_SUBDIR"
out="${TRACE_SUBDIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$tmp_trace" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
