#!/usr/bin/env bash
# Generate a single Gambatte test trace: adapter + ROM → .gbtrace
#
# All Gambatte tests run for exactly 15 LCD frames (1,053,360 T-cycles).
# Pass/fail is determined by:
#   1. _out<hex> in filename → render last frame, check screen matches hex pattern
#   2. .png reference next to ROM → screenshot comparison
#   3. _xout in filename → expected failure, skip
#
# Usage: trace-gambatte-tests.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
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

# Skip expected-failure tests
if echo "$NAME" | grep -q "_xout"; then
    printf "%-50s %-10s SKIP (xout)\n" "$NAME" "$ADAPTER"
    exit 0
fi

MAX_FRAMES=15
TMP="/tmp/gbtrace_gambatte_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"
tmp_trace="${TMP}.gbtrace"

cleanup() { rm -f "$stderr_file" "$tmp_trace" "${ROM%.gb}.sav"; }
trap cleanup EXIT

# Check for .pix reference — Gambatte names DMG refs as {test}_dmg08.pix
PIX_REF="$(dirname "$ROM")/${NAME}_dmg08.pix"
EXTRA_ARGS=()
if [[ -f "$PIX_REF" ]]; then
    EXTRA_ARGS+=(--reference "$PIX_REF")
fi

# Capture — run for exactly 15 frames
(
    set +eo pipefail
    "$BIN" --rom "$ROM" --profile "$PROFILE" \
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

# Method 1: Reference match (adapter stopped early on screenshot match)
if grep -q "Reference match" "$stderr_file" 2>/dev/null; then
    status="pass"
# Method 2: Hex output check (_out<hex> in filename)
elif echo "$NAME" | grep -qP '_out[0-9A-Fa-f]+$'; then
    expected_hex=$(echo "$NAME" | grep -oP '(?<=_out)[0-9A-Fa-f]+$')
    tmp_render="/tmp/gbtrace_render_${NAME}_${ADAPTER}_$$"
    mkdir -p "$tmp_render"
    # Gambatte tests show result on frame 15 — render all and check the last
    num_frames=$("$CLI" info "$tmp_trace" 2>/dev/null | grep Frames | awk '{print $2}')
    if [[ -n "$num_frames" ]] && [[ "$num_frames" -gt 0 ]]; then
        "$CLI" render "$tmp_trace" --output "$tmp_render" 2>/dev/null
        # Check frames from last to first — some adapters produce an extra frame
        for png in $(ls "$tmp_render"/*.png 2>/dev/null | sort -r); do
            if python3 "$(dirname "$0")/check-gambatte-hex.py" "$expected_hex" "$png" 2>/dev/null; then
                status="pass"
                break
            fi
        done
    fi
    rm -rf "$tmp_render"
fi

# --- Output ---
mkdir -p "$TRACE_SUBDIR"
out="${TRACE_SUBDIR}/${NAME}_${ADAPTER}_${status}.gbtrace"
mv "$tmp_trace" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-50s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"
