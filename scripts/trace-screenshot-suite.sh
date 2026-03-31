#!/usr/bin/env bash
# Generate a trace for a screenshot-based test suite: adapter + ROM → .gbtrace
#
# The adapter compares its framebuffer against a .pix reference file next to
# the ROM and stops when it matches. Pass/fail is determined by whether the
# adapter reported a reference match.
#
# Usage: trace-screenshot-suite.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir> [max-frames]
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
ROM_DIR="${5:-$(dirname "$ROM")}"
MAX_FRAMES="${6:-300}"
CLI="${CLI:-target/release/gbtrace}"

ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# Use relative path from ROM_DIR as the test name, flattening subdirs with __
ROM_REL="$(realpath --relative-to="$ROM_DIR" "$ROM")"
ROM_REL="${ROM_REL%.gb}"
NAME="${ROM_REL//\//__}"

BASENAME="$(basename "$ROM" .gb)"
PIX_REF="$(dirname "$ROM")/${BASENAME}.pix"

TMP="/tmp/gbtrace_screenshot_${NAME}_${ADAPTER}_$$"
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
        --frames "$MAX_FRAMES" \
        "${EXTRA_ARGS[@]}" \
        --output "$tmp_trace" 2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$tmp_trace" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-50s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# --- Determine pass/fail ---
status="fail"
if grep -q "Reference match" "$stderr_file" 2>/dev/null; then
    status="pass"
fi

# --- Output ---
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$tmp_trace" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-50s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
