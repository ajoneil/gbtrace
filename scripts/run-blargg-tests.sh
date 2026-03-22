#!/usr/bin/env bash
# Run Blargg CPU instruction tests across all adapters and report results.
# Filenames include _pass or _fail suffix based on test result.
#
# Usage:
#   ./scripts/run-blargg-tests.sh [adapter...]
#
# If no adapters specified, runs all: gambatte sameboy mgba gateboy

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

ROM_DIR="$PROJECT_DIR/docs/tests/blargg/cpu_instrs/individual"
PROFILE="$PROJECT_DIR/docs/tests/blargg/blargg_cpu.toml"
CLI="$PROJECT_DIR/target/release/gbtrace-cli"

# Serial stop: 4th newline = after "Passed\n" or "Failed\n"
STOP_SERIAL_BYTE="0A"
STOP_SERIAL_COUNT=4
MAX_FRAMES=3000

ADAPTERS=("${@:-gambatte sameboy mgba}")
if [[ $# -eq 0 ]]; then
    ADAPTERS=(gambatte sameboy mgba)
fi

# Adapter binary paths
declare -A ADAPTER_BIN
ADAPTER_BIN[gambatte]="$PROJECT_DIR/adapters/gambatte/gbtrace-gambatte"
ADAPTER_BIN[sameboy]="$PROJECT_DIR/adapters/sameboy/gbtrace-sameboy"
ADAPTER_BIN[mgba]="$PROJECT_DIR/adapters/mgba/gbtrace-mgba"
ADAPTER_BIN[gateboy]="$PROJECT_DIR/adapters/gateboy/gbtrace-gateboy"

export LD_LIBRARY_PATH="$PROJECT_DIR/adapters/sameboy/SameBoy/build/lib:${LD_LIBRARY_PATH:-}"

if [[ ! -x "$CLI" ]]; then
    echo "ERROR: gbtrace-cli not found at $CLI"
    echo "Build with: cargo build --release -p gbtrace-cli"
    exit 1
fi

PASS=0
FAIL=0
ERROR=0

extract_serial() {
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

        printf "  %-30s  " "$name"

        stderr_out=$(env "$bin" \
            --rom "$rom" \
            --profile "$PROFILE" \
            --output "$jsonl" \
            --stop-on-serial "$STOP_SERIAL_BYTE" \
            --stop-serial-count "$STOP_SERIAL_COUNT" \
            --stop-when A001=DE \
            --frames "$MAX_FRAMES" \
            2>&1) || true

        frame_info=$(echo "$stderr_out" | grep -oP 'frame \K[0-9]+' | tail -1)

        # Determine pass/fail: try serial output first, fall back to memory status
        serial=$(extract_serial "$jsonl" 2>/dev/null || echo "")

        if echo "$serial" | grep -qi "passed"; then
            status="PASS"; suffix="_pass"
            ((PASS++)) || true
        elif echo "$serial" | grep -qi "failed"; then
            status="FAIL"; suffix="_fail"
            ((FAIL++)) || true
        else
            # No serial — check test_status field (0xA000): 0=pass, other=fail
            mem_status=$("$CLI" query "$jsonl" -w "test_sig1=DE" --max 1 2>&1 || true)
            if echo "$mem_status" | grep -qP '^\d+ match'; then
                # Signature found — check test_status value
                if echo "$mem_status" | grep -qP 'test_status=0\b'; then
                    status="PASS"; suffix="_pass"
                    ((PASS++)) || true
                else
                    status="FAIL"; suffix="_fail"
                    ((FAIL++)) || true
                fi
            else
                status="????"; suffix="_fail"
                ((ERROR++)) || true
            fi
        fi

        # Convert to parquet with pass/fail suffix
        parquet="$ROM_DIR/${name}_${adapter}${suffix}.gbtrace.parquet"
        jsonl_size=$(du -h "$jsonl" 2>/dev/null | cut -f1)
        "$CLI" convert "$jsonl" -o "$parquet" >/dev/null 2>&1 && rm -f "$jsonl"
        parquet_size=$(du -h "$parquet" 2>/dev/null | cut -f1)

        printf "%-4s  frame %-5s  %s -> %s\n" "$status" "${frame_info:-?}" "$jsonl_size" "$parquet_size"

        # Clean up sav files
        rm -f "${rom%.gb}.sav"
    done
done

printf "\n=== Summary ===\n"
printf "  Pass: %d  Fail: %d  Unknown: %d  Total: %d\n" "$PASS" "$FAIL" "$ERROR" "$((PASS + FAIL + ERROR))"

# Generate manifest
echo "Generating manifest..."
python3 -c "
import json, glob, os, re

rom_dir = '$ROM_DIR'
roms = sorted(glob.glob(os.path.join(rom_dir, '*.gb')))
emus = ['gateboy', 'gambatte', 'sameboy', 'mgba']

manifest = []
for rom in roms:
    name = os.path.splitext(os.path.basename(rom))[0]
    rom_rel = 'cpu_instrs/individual/' + os.path.basename(rom)
    entry = {'name': name, 'rom': rom_rel, 'emulators': {}}
    for emu in emus:
        for status in ['pass', 'fail']:
            fname = f'{name}_{emu}_{status}.gbtrace.parquet'
            if os.path.exists(os.path.join(rom_dir, fname)):
                entry['emulators'][emu] = status
                break
    manifest.append(entry)

out_path = os.path.join(os.path.dirname(rom_dir), 'manifest.json')
with open(out_path, 'w') as f:
    json.dump(manifest, f)
print(f'  {len(manifest)} tests, {sum(1 for t in manifest for e in t[\"emulators\"])} traces')
"
