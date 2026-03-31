#!/usr/bin/env bash
# Generate a single AGE trace: adapter + ROM → .gbtrace
#
# Pass/fail detection:
#   AGE tests execute LD B, B (opcode 0x40) when complete.
#   Register tests: B=3 C=5 D=8 E=13 H=21 L=34 (Fibonacci sequence)
#   Screenshot tests: .pix reference comparison (adapter reports "Reference match")
#
# Usage: trace-age.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
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

# Check for .pix reference next to the ROM
BASENAME="$(basename "$ROM" .gb)"
PIX_REF="$(dirname "$ROM")/${BASENAME}.pix"

MAX_FRAMES=1200
TMP="/tmp/gbtrace_age_${NAME}_${ADAPTER}_$$"
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
    timeout 120 "$BIN" --rom "$ROM" --profile "$PROFILE" \
        --stop-opcode 40 --extra-frames 2 --frames "$MAX_FRAMES" \
        "${EXTRA_ARGS[@]}" \
        --output "$tmp_trace" 2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$tmp_trace" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-50s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# --- Determine pass/fail ---
if [[ -f "$PIX_REF" ]]; then
    # Screenshot test: check if the adapter reported a reference match
    if grep -q "Reference match" "$stderr_file" 2>/dev/null; then
        status="pass"
    else
        status="fail"
    fi
else
    # Register test: check Fibonacci sequence
    status=$("$CLI" query "$tmp_trace" --last 1 2>&1 | \
        grep -qP 'b=03\b.*c=05\b.*d=08\b.*e=0d\b.*h=15\b.*l=22\b' \
        && echo "pass" || echo "fail")
fi

# --- Output ---
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$tmp_trace" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-50s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
