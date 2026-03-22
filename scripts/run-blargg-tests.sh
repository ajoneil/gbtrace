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
PROFILE="$BLARGG_DIR/blargg.toml"
CLI="$PROJECT_DIR/target/release/gbtrace-cli"

# Serial stop: 4th newline = after "Passed\n" or "Failed\n"
STOP_SERIAL_BYTE="0A"
STOP_SERIAL_COUNT=4
MAX_FRAMES=3000

ADAPTERS=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --emu) ADAPTERS="$2"; shift 2 ;;
        *) ADAPTERS="${ADAPTERS:+$ADAPTERS }$1"; shift ;;
    esac
done
if [[ -z "$ADAPTERS" ]]; then
    ADAPTERS="gambatte sameboy mgba"
fi

export LD_LIBRARY_PATH="$PROJECT_DIR/adapters/sameboy/SameBoy/build/lib:${LD_LIBRARY_PATH:-}"

adapter_bin() {
    echo "$PROJECT_DIR/adapters/$1/gbtrace-$1"
}

# Build CLI if needed
if [[ ! -x "$CLI" ]]; then
    echo "Building gbtrace-cli..."
    cargo build --release -p gbtrace-cli --manifest-path "$PROJECT_DIR/Cargo.toml"
fi

# Build requested adapters
for adapter in $ADAPTERS; do
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

for adapter in $ADAPTERS; do
    bin="$(adapter_bin "$adapter")"
    if [[ ! -x "$bin" ]]; then
        echo "SKIP $adapter (binary not found: $bin)"
        continue
    fi

    printf "\n=== %s ===\n\n" "$adapter"

    while IFS= read -r rom; do
        name="$(basename "$rom" .gb)"
        rom_dir="$(dirname "$rom")"
        tmp_parquet="/tmp/gbtrace_blargg_${name}_${adapter}.gbtrace.parquet"

        printf "  %-40s  " "$name"

        # Stream adapter output directly to parquet (no temp JSONL)
        stderr_file="/tmp/gbtrace_blargg_${name}_stderr"
        (
            set +eo pipefail
            "$bin" \
                --rom "$rom" \
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
            printf "ERROR (%s)\n" "$err_msg"
            ((ERROR++)) || true
            rm -f "$tmp_parquet" "${rom%.gb}.sav" "$stderr_file"
            continue
        fi

        frame_info=$(grep -oP 'frame \K[0-9]+' "$stderr_file" 2>/dev/null | tail -1)
        rm -f "$stderr_file"

        # Determine pass/fail by querying the parquet for serial output
        # SC=255 (bit 7 set) means a byte was sent. Check SB values.
        serial=$("$CLI" query "$tmp_parquet" -w "sc changes to FF" --max 100 2>&1 | \
            grep -oP 'sb=\K[0-9a-f]+' | while read hex; do printf "\\x$hex"; done) || serial=""

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

        # Move to final location with pass/fail suffix
        parquet="${rom_dir}/${name}_${adapter}${suffix}.gbtrace.parquet"
        mv "$tmp_parquet" "$parquet"
        parquet_size=$(du -h "$parquet" 2>/dev/null | cut -f1)
        entries=$("$CLI" info "$parquet" 2>/dev/null | grep Entries | awk '{print $2}')

        printf "%-4s  frame %-5s  %s entries  %s\n" "$status" "${frame_info:-?}" "${entries:-?}" "$parquet_size"

        # Clean up sav files
        rm -f "${rom%.gb}.sav" /tmp/gbtrace_blargg_stderr
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
