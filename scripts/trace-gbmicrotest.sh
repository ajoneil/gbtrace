#!/usr/bin/env bash
# Generate a single gbmicrotest trace: adapter + ROM → parquet
# Usage: trace-gbmicrotest.sh <adapter-binary> <rom> <profile> <output-dir>
set -euo pipefail

BIN="$1"
ROM="$2"
PROFILE="$3"
OUT_DIR="$4"
CLI="${CLI:-target/release/gbtrace-cli}"

NAME="$(basename "$ROM" .gb)"
ADAPTER="$(basename "$BIN" | sed 's/gbtrace-//')"
FRAMES=2

TMP="/tmp/gbtrace_micro_${NAME}_${ADAPTER}_$$"
RAW="${TMP}.gbtrace"

# Capture
if ! "$BIN" --rom "$ROM" --profile "$PROFILE" --output "$RAW" \
    --frames "$FRAMES" \
    --stop-when FF82=01 --stop-when FF82=FF 2>/dev/null; then
    printf "%-40s %-10s ERROR\n" "$NAME" "$ADAPTER"
    rm -f "$RAW" "${ROM%.gb}.sav"
    exit 1
fi

# Strip boot ROM entries if present
STRIPPED="${TMP}.stripped"
boot_rom=$("$CLI" info "$RAW" 2>/dev/null | grep 'Boot ROM' | awk '{print $3}')
if [ "$boot_rom" = "skip" ] || [ "$boot_rom" = "none" ] || [ "$boot_rom" = "built-in" ]; then
    cp "$RAW" "$STRIPPED"
elif "$CLI" strip-boot "$RAW" --output "$STRIPPED" >/dev/null 2>&1; then
    : # stripped successfully
else
    cp "$RAW" "$STRIPPED"
fi

# Trim to the instruction where test_pass is set
TRIMMED="${TMP}.trimmed"
total_entries=$("$CLI" info "$STRIPPED" 2>/dev/null | grep Entries | awk '{print $2}')
"$CLI" trim "$STRIPPED" --output "$TRIMMED" --until "test_pass=01" >/dev/null 2>&1
trimmed_entries=$("$CLI" info "$TRIMMED" 2>/dev/null | grep Entries | awk '{print $2}')
if [ "$trimmed_entries" = "$total_entries" ]; then
    "$CLI" trim "$STRIPPED" --output "$TRIMMED" --until "test_pass=FF" >/dev/null 2>&1
fi

# Determine pass/fail
result_pass=$("$CLI" query "$TRIMMED" -w "test_pass=01" --max 1 2>&1 | grep -cP '^\d+ match' || true)
result_fail=$("$CLI" query "$TRIMMED" -w "test_pass=FF" --max 1 2>&1 | grep -cP '^\d+ match' || true)

if [ "$result_pass" -gt 0 ]; then
    status="pass"
elif [ "$result_fail" -gt 0 ]; then
    status="fail"
else
    status="fail"
fi

# Convert to parquet
mkdir -p "$OUT_DIR"
out="${OUT_DIR}/${NAME}_${ADAPTER}_${status}.gbtrace.parquet"
"$CLI" convert "$TRIMMED" --output "$out" >/dev/null 2>&1

entries=$("$CLI" info "$out" 2>/dev/null | grep Entries | awk '{print $2}')
printf "%-40s %-10s %-4s %6s entries\n" "$NAME" "$ADAPTER" "${status^^}" "${entries:-?}"

rm -f "$RAW" "$STRIPPED" "$TRIMMED" "${ROM%.gb}.sav"
