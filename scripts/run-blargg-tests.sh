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

BLARGG_DIR="$PROJECT_DIR/docs/tests/blargg"
PROFILE="$BLARGG_DIR/blargg_cpu.toml"
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

# Build CLI if needed
if [[ ! -x "$CLI" ]]; then
    echo "Building gbtrace-cli..."
    cargo build --release -p gbtrace-cli --manifest-path "$PROJECT_DIR/Cargo.toml"
fi

# Build requested adapters
for adapter in "${ADAPTERS[@]}"; do
    adapter_dir="$PROJECT_DIR/adapters/$adapter"
    if [[ -f "$adapter_dir/Makefile" ]]; then
        echo "Building $adapter adapter..."
        if ! make -C "$adapter_dir" -j"$(nproc)" > /dev/null 2>&1; then
            echo "  WARNING: $adapter build failed, skipping"
        fi
    fi
done

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

    while IFS= read -r rom; do
        name="$(basename "$rom" .gb)"
        rom_dir="$(dirname "$rom")"
        jsonl="${rom_dir}/${name}_${adapter}.gbtrace"

        printf "  %-40s  " "$name"

        stderr_out=$(env "$bin" \
            --rom "$rom" \
            --profile "$PROFILE" \
            --output "$jsonl" \
            --stop-on-serial "$STOP_SERIAL_BYTE" \
            --stop-serial-count "$STOP_SERIAL_COUNT" \
            --frames "$MAX_FRAMES" \
            2>&1) || true

        frame_info=$(echo "$stderr_out" | grep -oP 'frame \K[0-9]+' | tail -1)

        # Determine pass/fail from serial output
        serial=$(extract_serial "$jsonl" 2>/dev/null || echo "")

        if echo "$serial" | grep -qi "passed"; then
            status="PASS"; suffix="_pass"
            ((PASS++)) || true
        elif echo "$serial" | grep -qi "failed"; then
            status="FAIL"; suffix="_fail"
            ((FAIL++)) || true
        else
            status="????"; suffix="_fail"
            ((ERROR++)) || true
        fi

        # Convert to parquet with pass/fail suffix
        parquet="${rom_dir}/${name}_${adapter}${suffix}.gbtrace.parquet"
        jsonl_size=$(du -h "$jsonl" 2>/dev/null | cut -f1)
        "$CLI" convert "$jsonl" -o "$parquet" >/dev/null 2>&1 && rm -f "$jsonl"
        parquet_size=$(du -h "$parquet" 2>/dev/null | cut -f1)

        printf "%-4s  frame %-5s  %s -> %s\n" "$status" "${frame_info:-?}" "$jsonl_size" "$parquet_size"

        # Clean up sav files
        rm -f "${rom%.gb}.sav"
    done < <(find "$BLARGG_DIR" -name "*.gb" | sort)
done

printf "\n=== Summary ===\n"
printf "  Pass: %d  Fail: %d  Unknown: %d  Total: %d\n" "$PASS" "$FAIL" "$ERROR" "$((PASS + FAIL + ERROR))"

# Generate manifest
echo "Generating manifest..."
python3 -c "
import json, os

blargg_dir = '$BLARGG_DIR'
emus = ['gateboy', 'gambatte', 'sameboy', 'mgba']

manifest = []
for dirpath, dirnames, filenames in sorted(os.walk(blargg_dir)):
    for fname in sorted(filenames):
        if not fname.endswith('.gb'):
            continue
        name = fname[:-3]
        rom_rel = os.path.relpath(os.path.join(dirpath, fname), blargg_dir)
        entry = {'name': name, 'rom': rom_rel, 'emulators': {}}
        for emu in emus:
            for status in ['pass', 'fail']:
                trace = f'{name}_{emu}_{status}.gbtrace.parquet'
                if os.path.exists(os.path.join(dirpath, trace)):
                    entry['emulators'][emu] = status
                    break
        manifest.append(entry)

out_path = os.path.join(blargg_dir, 'manifest.json')
with open(out_path, 'w') as f:
    json.dump(manifest, f)
print(f'  {len(manifest)} tests, {sum(1 for t in manifest for e in t[\"emulators\"])} traces')
"
