#!/usr/bin/env bash
# Run Blargg CPU instruction tests across all adapters and report results.
#
# Usage:
#   ./scripts/run-blargg-tests.sh [adapter...]
#
# If no adapters specified, runs all three: gambatte sameboy mgba
# Parquet traces are saved next to the ROMs for web access:
#   docs/tests/blargg/cpu_instrs/individual/<testname>_<adapter>.gbtrace.parquet

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

ROM_DIR="$PROJECT_DIR/docs/tests/blargg/cpu_instrs/individual"
PROFILE="$PROJECT_DIR/profiles/blargg_cpu.toml"
CLI="$PROJECT_DIR/target/release/gbtrace-cli"

# Serial stop: 4th newline = after "Passed\n" or "Failed\n"
STOP_SERIAL_BYTE="0A"
STOP_SERIAL_COUNT=4
MAX_FRAMES=3000

ADAPTERS=("${@:-gambatte sameboy mgba}")
if [[ $# -eq 0 ]]; then
    ADAPTERS=(gambatte sameboy mgba)
fi

# Adapter binary paths and any required env
declare -A ADAPTER_BIN ADAPTER_ENV
ADAPTER_BIN[gambatte]="$PROJECT_DIR/adapters/gambatte/build/gbtrace-gambatte"
ADAPTER_BIN[sameboy]="$PROJECT_DIR/adapters/sameboy/gbtrace-sameboy"
ADAPTER_BIN[mgba]="$PROJECT_DIR/adapters/mgba/gbtrace-mgba"
ADAPTER_ENV[sameboy]="LD_LIBRARY_PATH=$PROJECT_DIR/adapters/sameboy/SameBoy/build/lib"

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: gbtrace-cli not found at $CLI"
    echo "Build with: cargo build --release -p gbtrace-cli"
    exit 1
fi

# Counters
PASS=0
FAIL=0
ERROR=0

extract_serial() {
    # Extract serial output by detecting SC bit 7 rising edges.
    local trace_file="$1"
    awk -F'"sc":' '{
        split($2, a, /[,}]/); sc = a[1] + 0
        split($0, b, "\"sb\":"); split(b[2], c, /[,}]/); sb = c[1] + 0
        if (sc == 255 && prev_sc != 255) printf "%c", sb
        prev_sc = sc
    }' "$trace_file"
}

for adapter in "${ADAPTERS[@]}"; do
    bin="${ADAPTER_BIN[$adapter]}"
    if [[ ! -x "$bin" ]]; then
        echo "SKIP $adapter (binary not found: $bin)"
        continue
    fi

    printf "\n=== %s ===\n\n" "$adapter"

    for rom in "$ROM_DIR"/*.gb; do
        name="$(basename "$rom" .gb)"
        jsonl="$ROM_DIR/${name}_${adapter}.gbtrace"
        parquet="$ROM_DIR/${name}_${adapter}.gbtrace.parquet"

        # Print test name immediately so user sees progress
        printf "  %-30s  " "$name"

        # Run the adapter
        env_prefix="${ADAPTER_ENV[$adapter]:-}"
        stderr_out=$(env $env_prefix "$bin" \
            --rom "$rom" \
            --profile "$PROFILE" \
            --output "$jsonl" \
            --stop-on-serial "$STOP_SERIAL_BYTE" \
            --stop-serial-count "$STOP_SERIAL_COUNT" \
            --frames "$MAX_FRAMES" \
            2>&1) || true

        # Extract frame count
        frame_info=$(echo "$stderr_out" | grep -oP 'frame \K[0-9]+' | tail -1)

        # Extract serial output to check pass/fail
        serial=$(extract_serial "$jsonl" 2>/dev/null || echo "")

        if echo "$serial" | grep -qi "passed"; then
            status="PASS"
            ((PASS++)) || true
        elif echo "$serial" | grep -qi "failed"; then
            status="FAIL"
            ((FAIL++)) || true
        else
            status="????"
            ((ERROR++)) || true
        fi

        # Convert to parquet and delete JSONL
        jsonl_size=$(du -h "$jsonl" 2>/dev/null | cut -f1)
        "$CLI" convert "$jsonl" -o "$parquet" >/dev/null 2>&1 && rm -f "$jsonl"
        parquet_size=$(du -h "$parquet" 2>/dev/null | cut -f1)

        printf "%-4s  frame %-5s  %s -> %s\n" "$status" "${frame_info:-?}" "$jsonl_size" "$parquet_size"
    done
done

# Summary
printf "\n=== Summary ===\n"
printf "  Pass: %d  Fail: %d  Unknown: %d  Total: %d\n" "$PASS" "$FAIL" "$ERROR" "$((PASS + FAIL + ERROR))"
