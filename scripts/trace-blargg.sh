#!/usr/bin/env bash
# Generate a single blargg trace: adapter + ROM → parquet
#
# Pass/fail detection:
#   1. If adapter supports serial, stop on serial output and check for "Passed"
#   2. If a .pix reference exists next to the ROM, use screenshot matching
#
# Usage: trace-blargg.sh <adapter-binary> <rom> <profile> <output-dir> <rom-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
ROM_DIR="${5:-$(dirname "$ROM")}"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"

# Compute relative subdir from ROM_DIR to ROM (e.g. cpu_instrs/)
ROM_REL="$(realpath --relative-to="$ROM_DIR" "$(dirname "$ROM")")"
if [ "$ROM_REL" = "." ]; then
    TRACE_SUBDIR="$OUT_DIR"
else
    TRACE_SUBDIR="$OUT_DIR/$ROM_REL"
fi

# Check for .pix reference next to the ROM
PIX_REF="$(dirname "$ROM")/${NAME}.pix"

MAX_FRAMES=1200
TMP="/tmp/gbtrace_blargg_${NAME}_${ADAPTER}_$$"
stderr_file="${TMP}.stderr"
tmp_parquet="${TMP}.parquet"
tmp_gbtrace="${TMP}.gbtrace"

# --- Capture ---
# Build adapter-specific args.
# All adapters get --stop-on-serial for serial-based pass/fail.
# If a .pix reference exists, pass --reference for screenshot-based early stop.
EXTRA_ARGS=()
if [[ -f "$PIX_REF" ]]; then
    EXTRA_ARGS+=(--reference "$PIX_REF")
fi

(
    set +eo pipefail
    "$BIN" \
        --rom "$ROM" \
        --profile "$PROFILE" \
        --stop-on-serial "0A" \
        --stop-serial-count 4 \
        --extra-frames 2 \
        --frames "$MAX_FRAMES" \
        "${EXTRA_ARGS[@]}" \
        --output "$tmp_gbtrace" \
        2>"$stderr_file" \
        < /dev/null
) || true

if [[ ! -s "$tmp_gbtrace" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    rm -f "$tmp_gbtrace" "$stderr_file" "${ROM%.gb}.sav"
    exit 1
fi

# --- Convert to parquet ---
"$CLI" convert "$tmp_gbtrace" -o "$tmp_parquet" >/dev/null 2>&1

if [[ ! -s "$tmp_parquet" ]]; then
    printf "%-40s %-10s ERROR (convert)\n" "$NAME" "$ADAPTER"
    rm -f "$tmp_gbtrace" "$tmp_parquet" "$stderr_file" "${ROM%.gb}.sav"
    exit 1
fi

# --- Determine pass/fail ---
status="fail"

# Method 1: Serial output (check if trace contains serial data)
serial=$("$CLI" query "$tmp_parquet" -w "sc changes to FF" --max 100 2>&1 | \
    grep -oP 'sb=\K[0-9a-f]+' | while read hex; do printf "\\x$hex"; done) || serial=""

if echo "$serial" | grep -qi "passed"; then
    status="pass"
elif echo "$serial" | grep -qi "failed"; then
    status="fail"
# Method 2: Screenshot matching against .pix reference
elif [[ -f "$PIX_REF" ]]; then
    if "$CLI" trim "$tmp_parquet" --reference "$PIX_REF" \
        --output "${TMP}_trimmed.parquet" >/dev/null 2>&1; then
        status="pass"
        mv "${TMP}_trimmed.parquet" "$tmp_parquet"
    fi
fi

# --- Output ---
mkdir -p "$TRACE_SUBDIR"
out="${TRACE_SUBDIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
mv "$tmp_parquet" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"

rm -f "$tmp_gbtrace" "${TMP}_trimmed.parquet" "$stderr_file" "${ROM%.gb}.sav"
