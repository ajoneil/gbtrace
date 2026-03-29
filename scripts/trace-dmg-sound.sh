#!/usr/bin/env bash
# Generate a single dmg_sound trace: adapter + ROM → .gbtrace
#
# Pass/fail: screenshot comparison against reference .pix files,
# same as the regular blargg trace script.
#
# Usage: trace-dmg-sound.sh <adapter-binary> <rom> <profile> <output-dir> [<rom-dir>]
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

# Tests take ~36 emulated seconds (~2150 frames) to complete
MAX_FRAMES=3000

TMP="/tmp/gbtrace_dmg_sound_${NAME}_${ADAPTER}_$$"
TRACE="${TMP}.gbtrace"
stderr_file="${TMP}.stderr"

cleanup() { rm -f "$TRACE" "$stderr_file" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# Capture — use reference screenshot for stop condition
EXTRA_ARGS=()
if [[ -f "$PIX_REF" ]]; then
    EXTRA_ARGS+=(--reference "$PIX_REF")
fi

(
    set +eo pipefail
    "$BIN" --rom "$ROM" --profile "$PROFILE" --output "$TRACE" \
        --frames "$MAX_FRAMES" \
        --extra-frames 2 \
        "${EXTRA_ARGS[@]}" \
        2>"$stderr_file" </dev/null
) || true

if [[ ! -s "$TRACE" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    exit 1
fi

# Determine pass/fail from reference match
status="fail"
if grep -q "Reference match" "$stderr_file" 2>/dev/null; then
    status="pass"
fi

# Move to output
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$TRACE" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
