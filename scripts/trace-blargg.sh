#!/usr/bin/env bash
# Generate a single blargg trace: adapter + ROM → parquet
# Usage: trace-blargg.sh <adapter-binary> <rom> <profile> <output-dir>
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

STOP_SERIAL_BYTE="0A"
STOP_SERIAL_COUNT=4
MAX_FRAMES=3000

TMP="/tmp/gbtrace_blargg_${NAME}_${ADAPTER}_$$"

# Stream adapter output directly to parquet
stderr_file="${TMP}.stderr"
tmp_parquet="${TMP}.parquet"

(
    set +eo pipefail
    "$BIN" \
        --rom "$ROM" \
        --profile "$PROFILE" \
        --stop-on-serial "$STOP_SERIAL_BYTE" \
        --stop-serial-count "$STOP_SERIAL_COUNT" \
        --frames "$MAX_FRAMES" \
        2>"$stderr_file" \
        < /dev/null \
    | "$CLI" convert - -o "$tmp_parquet" >/dev/null 2>&1
) || true

if [[ ! -s "$tmp_parquet" ]]; then
    err_msg=$(head -1 "$stderr_file" 2>/dev/null || echo "unknown")
    printf "%-40s %-10s ERROR (%s)\n" "$NAME" "$ADAPTER" "$err_msg"
    rm -f "$tmp_parquet" "$stderr_file" "${ROM%.gb}.sav"
    exit 1
fi

# Determine pass/fail from serial output
serial=$("$CLI" query "$tmp_parquet" -w "sc changes to FF" --max 100 2>&1 | \
    grep -oP 'sb=\K[0-9a-f]+' | while read hex; do printf "\\x$hex"; done) || serial=""

if echo "$serial" | grep -qi "passed"; then
    status="pass"
elif echo "$serial" | grep -qi "failed"; then
    status="fail"
else
    status="fail"
fi

# Move to final location
mkdir -p "$TRACE_SUBDIR"
out="${TRACE_SUBDIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
mv "$tmp_parquet" "$out"

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"

rm -f "$stderr_file" "${ROM%.gb}.sav"
